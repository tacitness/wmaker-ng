#!/bin/sh
# ============================================================================
# wmaker-ai-browser launcher (#20)
# ----------------------------------------------------------------------------
# Exec'd by the base `wmaker-headless` entrypoint AFTER Xvfb + wmaker are up.
# Starts the browser as a background X client on the live desktop, then execs
# `ai-mcp` so the container's main process is the MCP transport (stdio).
#
# Env:
#   BROWSER          browser binary (default: brave-browser)
#   START_URL        page to open on boot (default: about:blank)
#   USER_DATA_DIR    persistent browser profile root (default: /profile)
#   FIREFOX_PROFILE_DIR Firefox profile selected with native -profile
#                    (default: $USER_DATA_DIR/firefox)
#   DISPOSABLE_PROFILE if "1", copy/seed into /tmp/profile and discard on exit
#   PROFILE_SEED_TARBALL optional tar/tar.gz seed mounted from a secret store
#   AUTH_ALLOWED_DOMAINS optional comma-separated START_URL host allowlist
#   WMAKER_AI_BROWSER_ENABLE_ADAPTER if "1", install native messaging and load
#                    the in-image DOM/ARIA adapter extension.
#   WMAKER_AI_BROWSER_EXTENSION_DIR optional unpacked extension path.
#   WMAKER_AI_BROWSER_EXTENSION_ID extension id allowed by native messaging.
#   CLEAR_SINGLETON  if "1", remove stale Singleton{Lock,Socket,Cookie} from a
#                    bind-mounted profile so a container Brave can claim it.
#                    Off by default — it mutates the mounted (possibly host)
#                    profile, so opt in only when the host browser is closed.
# ============================================================================
set -eu

: "${BROWSER:=brave-browser}"
: "${START_URL:=about:blank}"
: "${USER_DATA_DIR:=/profile}"
: "${WMAKER_AI_BROWSER_EXTENSION_ID:=dlnkidcfokpkpcpbpkgoiklfjilaijbl}"

log() { echo "[wmaker-ai-browser] $*" >&2; }

url_host() {
	echo "$1" | sed -n 's,^[a-zA-Z][a-zA-Z0-9+.-]*://\([^/:?]*\).*,\1,p'
}

if [ -n "${AUTH_ALLOWED_DOMAINS:-}" ]; then
	host="$(url_host "$START_URL")"
	case ",$AUTH_ALLOWED_DOMAINS," in
		*,"$host",*) ;;
		*)
			log "START_URL host '$host' is outside AUTH_ALLOWED_DOMAINS=$AUTH_ALLOWED_DOMAINS"
			exit 64
			;;
	esac
fi

if [ "${DISPOSABLE_PROFILE:-0}" = "1" ]; then
	USER_DATA_DIR=/tmp/wmaker-ai-browser-profile
	export USER_DATA_DIR
fi

# Strip trailing slashes (except for /) so the containment check below cannot
# be bypassed with a sibling path such as /profile-other.
if [ "$USER_DATA_DIR" != "/" ]; then
	USER_DATA_DIR=${USER_DATA_DIR%/}
	export USER_DATA_DIR
fi

mkdir -p "$USER_DATA_DIR"

if [ -n "${PROFILE_SEED_TARBALL:-}" ]; then
	if [ ! -r "$PROFILE_SEED_TARBALL" ]; then
		log "PROFILE_SEED_TARBALL is not readable: $PROFILE_SEED_TARBALL"
		exit 66
	fi
	log "seeding disposable browser profile from $PROFILE_SEED_TARBALL"
	tar -xf "$PROFILE_SEED_TARBALL" -C "$USER_DATA_DIR"
fi

if [ "${CLEAR_SINGLETON:-0}" = "1" ]; then
	log "clearing stale Singleton locks in $USER_DATA_DIR"
	rm -f "$USER_DATA_DIR"/Singleton* 2>/dev/null || true
fi

browser_native_host_dirs() {
	case "$BROWSER" in
		*brave*)
			echo "${HOME:-/root}/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
			;;
		*chromium*)
			echo "${HOME:-/root}/.config/chromium/NativeMessagingHosts"
			;;
		*chrome*)
			echo "${HOME:-/root}/.config/google-chrome/NativeMessagingHosts"
			;;
		*)
			echo "${HOME:-/root}/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
			echo "${HOME:-/root}/.config/chromium/NativeMessagingHosts"
			echo "${HOME:-/root}/.config/google-chrome/NativeMessagingHosts"
			;;
	esac
}

extension_args=""
if [ "${WMAKER_AI_BROWSER_ENABLE_ADAPTER:-0}" = "1" ]; then
	extension_dir="${WMAKER_AI_BROWSER_EXTENSION_DIR:-/usr/share/wmaker-ai-browser/extension}"
	native_template_dir="${WMAKER_AI_BROWSER_NATIVE_TEMPLATE_DIR:-/usr/share/wmaker-ai-browser/native-messaging}"
	native_runtime_dir="${WMAKER_AI_BROWSER_NATIVE_RUNTIME_DIR:-/tmp/wmaker-ai-browser-native}"
	host_wrapper="$native_runtime_dir/wmaker-ai-browser-host.sh"
	host_manifest="$native_runtime_dir/wmaker_ai_browser.json"
	ai_mcp_path="$(command -v ai-mcp)"

	if [ ! -d "$extension_dir" ]; then
		log "browser adapter extension dir is missing: $extension_dir"
		exit 66
	fi
	mkdir -p "$native_runtime_dir"
	sed \
		-e "s#__AI_MCP_PATH__#$ai_mcp_path#g" \
		-e "s#__EXTENSION_ID__#$WMAKER_AI_BROWSER_EXTENSION_ID#g" \
		"$native_template_dir/wmaker-ai-browser-host.sh.in" >"$host_wrapper"
	chmod 0755 "$host_wrapper"
	sed \
		-e "s#__BROWSER_HOST_PATH__#$host_wrapper#g" \
		-e "s#__EXTENSION_ID__#$WMAKER_AI_BROWSER_EXTENSION_ID#g" \
		"$native_template_dir/wmaker_ai_browser.json.in" >"$host_manifest"
	for dir in $(browser_native_host_dirs); do
		mkdir -p "$dir"
		cp "$host_manifest" "$dir/wmaker_ai_browser.json"
	done
	extension_args="--load-extension=$extension_dir --disable-extensions-except=$extension_dir"
	log "browser semantic adapter enabled (extension: $WMAKER_AI_BROWSER_EXTENSION_ID)"
fi

case "$BROWSER" in
	*firefox*)
		: "${FIREFOX_PROFILE_DIR:=$USER_DATA_DIR/firefox}"
		case "$FIREFOX_PROFILE_DIR" in
			"$USER_DATA_DIR" | "$USER_DATA_DIR"/*) ;;
			*)
				log "FIREFOX_PROFILE_DIR must be inside USER_DATA_DIR"
				exit 64
				;;
		esac
		mkdir -p "$FIREFOX_PROFILE_DIR"
		log "launching $BROWSER on $DISPLAY (profile: $FIREFOX_PROFILE_DIR, url: $START_URL)"
		# Firefox does not implement Chromium's --user-data-dir contract. Using
		# its native -profile selector prevents a new install identity after an
		# image upgrade from silently choosing a different profile directory.
		"$BROWSER" \
			--no-remote \
			-profile "$FIREFOX_PROFILE_DIR" \
			--new-window "$START_URL" >/tmp/browser.log 2>&1 &
		;;
	*)
		log "launching $BROWSER on $DISPLAY (profile: $USER_DATA_DIR, url: $START_URL)"
		# Container-appropriate Chromium flags: no zygote sandbox (no userns),
		# fixed geometry matching Xvfb, and quiet first-run UX.
		"$BROWSER" \
			--no-sandbox \
			--no-first-run \
			--no-default-browser-check \
			--disable-features=Translate \
			--user-data-dir="$USER_DATA_DIR" \
			--window-position=0,0 \
			--window-size=1280,800 \
			--start-maximized \
			$extension_args \
			"$START_URL" >/tmp/browser.log 2>&1 &
		;;
esac

log "exec ai-mcp (MCP over stdio)"
exec ai-mcp
