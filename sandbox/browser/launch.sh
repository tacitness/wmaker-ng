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
#   USER_DATA_DIR    profile dir / --user-data-dir (default: /profile)
#   CLEAR_SINGLETON  if "1", remove stale Singleton{Lock,Socket,Cookie} from a
#                    bind-mounted profile so a container Brave can claim it.
#                    Off by default — it mutates the mounted (possibly host)
#                    profile, so opt in only when the host browser is closed.
# ============================================================================
set -eu

: "${BROWSER:=brave-browser}"
: "${START_URL:=about:blank}"
: "${USER_DATA_DIR:=/profile}"

log() { echo "[wmaker-ai-browser] $*" >&2; }

mkdir -p "$USER_DATA_DIR"

if [ "${CLEAR_SINGLETON:-0}" = "1" ]; then
	log "clearing stale Singleton locks in $USER_DATA_DIR"
	rm -f "$USER_DATA_DIR"/Singleton* 2>/dev/null || true
fi

log "launching $BROWSER on $DISPLAY (profile: $USER_DATA_DIR, url: $START_URL)"
# Container-appropriate flags: no zygote sandbox (no userns), fixed geometry to
# match the Xvfb screen, quiet first-run UX.
"$BROWSER" \
	--no-sandbox \
	--no-first-run \
	--no-default-browser-check \
	--disable-features=Translate \
	--user-data-dir="$USER_DATA_DIR" \
	--window-position=0,0 \
	--window-size=1280,800 \
	--start-maximized \
	"$START_URL" >/tmp/browser.log 2>&1 &

log "exec ai-mcp (MCP over stdio)"
exec ai-mcp
