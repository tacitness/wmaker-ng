#!/usr/bin/env sh
# Install wmaker-ng static release binaries from repos.tacitsoft.dev.
set -eu

BASE_URL="${WMAKER_NG_STATIC_URL:-https://repos.tacitsoft.dev/releases/wmaker-ng}"
VERSION="${WMAKER_NG_VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local}"
TMPDIR="${TMPDIR:-/tmp}"

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "error: required command not found: $1" >&2
		exit 1
	fi
}

fetch() {
	url="$1"
	out="$2"
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL "$url" -o "$out"
	elif command -v wget >/dev/null 2>&1; then
		wget -qO "$out" "$url"
	else
		echo "error: curl or wget is required" >&2
		exit 1
	fi
}

case "$(uname -m)" in
	x86_64) arch="amd64" ;;
	aarch64 | arm64) arch="arm64" ;;
	*)
		echo "error: unsupported architecture: $(uname -m)" >&2
		exit 1
		;;
esac

need sha256sum
need tar
need zstd

work="$(mktemp -d "$TMPDIR/wmaker-ng-install.XXXXXX")"
trap 'rm -rf "$work"' EXIT

if [ "$VERSION" = "latest" ]; then
	fetch "$BASE_URL/latest/VERSION" "$work/VERSION"
	VERSION="$(tr -d '[:space:]' <"$work/VERSION")"
fi

file="wmaker-ng-$VERSION-$arch-musl.tar.zst"
url="$BASE_URL/$VERSION/$file"

fetch "$url" "$work/$file"
fetch "$url.sha256" "$work/$file.sha256"
(cd "$work" && sha256sum -c "$file.sha256")

mkdir -p "$work/extract"
zstd -dc "$work/$file" | tar -xf - -C "$work/extract"
stage="$work/extract/wmaker-ng-$VERSION-$arch-musl"

if [ ! -d "$stage" ]; then
	echo "error: expected stage directory not found: $stage" >&2
	exit 1
fi

install -d "$PREFIX/bin"
for bin in ai-mcp ng-automount ng-notify ng-power; do
	if [ -f "$stage/$bin" ]; then
		install -m 0755 "$stage/$bin" "$PREFIX/bin/$bin"
	fi
done

# MCP client config + agent guidance (also emitted by `ai-mcp print-config`).
if [ -d "$stage/integrations" ]; then
	share="$PREFIX/share/wmaker-ai/integrations"
	find "$stage/integrations" -type f | while IFS= read -r f; do
		rel="${f#"$stage/integrations/"}"
		install -d "$share/$(dirname "$rel")"
		case "$rel" in
		*install-agents-md.sh) install -m 0755 "$f" "$share/$rel" ;;
		*) install -m 0644 "$f" "$share/$rel" ;;
		esac
	done
	echo "wmaker-ai integrations installed into $share"
fi

echo "wmaker-ng $VERSION installed into $PREFIX/bin"
