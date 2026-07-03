#!/usr/bin/env bash
# ============================================================================
# static-channel.sh — build the distro-agnostic .tar.zst channel.
#
# Usage:
#   scripts/static-channel.sh <tarball-dir> <version> [out-dir]
#
# Consumes the musl release tarballs and emits:
#   dist/static/releases/<version>/*.tar.zst
#   dist/static/releases/<version>/*.sha256
#   dist/static/releases/<version>/manifest.json
#   dist/static/releases/<version>/VERSION
#   dist/static/latest -> releases/<version>
#   dist/static/install.sh
#   dist/static/manifest.json
# ============================================================================
set -euo pipefail

TARBALL_DIR="${1:?usage: static-channel.sh <tarball-dir> <version> [out-dir]}"
PKG_VERSION="${2:?missing version}"
# Repo root; falls back to cwd inside the CI assembly containers (no git,
# and root-vs-runner ownership would trip git's dubious-ownership check).
ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT_DIR="${3:-$ROOT_DIR/dist/static}"
VERSION_DIR="$OUT_DIR/releases/$PKG_VERSION"

require() {
	command -v "$1" >/dev/null 2>&1 || {
		echo "error: required command not found: $1" >&2
		exit 1
	}
}

require gzip
require zstd
require sha256sum

mkdir -p "$VERSION_DIR"

convert_one() {
	local arch="$1"
	local src="$TARBALL_DIR/wmaker-ng-$PKG_VERSION-$arch-musl.tar.gz"
	local dst="$VERSION_DIR/wmaker-ng-$PKG_VERSION-$arch-musl.tar.zst"

	[[ -f "$src" ]] || {
		echo "error: missing tarball: $src" >&2
		exit 1
	}

	gzip -dc "$src" | zstd -q -f -T0 -19 -o "$dst"
	(cd "$VERSION_DIR" && sha256sum "$(basename "$dst")" >"$(basename "$dst").sha256")
}

convert_one amd64
convert_one arm64

amd64_sha="$(awk '{print $1}' "$VERSION_DIR/wmaker-ng-$PKG_VERSION-amd64-musl.tar.zst.sha256")"
arm64_sha="$(awk '{print $1}' "$VERSION_DIR/wmaker-ng-$PKG_VERSION-arm64-musl.tar.zst.sha256")"

cat >"$VERSION_DIR/manifest.json" <<EOF
{
  "version": "$PKG_VERSION",
  "format": "tar.zst",
  "libc": "musl",
  "packages": {
    "x86_64": {
      "arch": "amd64",
      "file": "wmaker-ng-$PKG_VERSION-amd64-musl.tar.zst",
      "sha256": "$amd64_sha"
    },
    "aarch64": {
      "arch": "arm64",
      "file": "wmaker-ng-$PKG_VERSION-arm64-musl.tar.zst",
      "sha256": "$arm64_sha"
    }
  }
}
EOF

printf '%s\n' "$PKG_VERSION" >"$VERSION_DIR/VERSION"
cp "$ROOT_DIR/packaging/static/install.sh" "$OUT_DIR/install.sh"
chmod 0755 "$OUT_DIR/install.sh"
cp "$VERSION_DIR/manifest.json" "$OUT_DIR/manifest.json"
ln -sfn "releases/$PKG_VERSION" "$OUT_DIR/latest"

echo "==> static channel: $OUT_DIR" >&2
