#!/usr/bin/env bash
# ============================================================================
# tarball.sh — package staged binaries into a portable tarball + checksum.
#
# Usage: scripts/tarball.sh <arch> <libc> <stage-subdir> [out-dir]
#   e.g. scripts/tarball.sh amd64 gnu amd64-gnu
#
# Env: PKG_VERSION (required). Produces:
#   dist/tarballs/wmaker-ng-<ver>-<arch>-<libc>.tar.gz (+ .sha256)
#
# These raw-binary tarballs are the "compiled binaries" release artifact and the
# seed for the static-tarball / AUR-bin channels (tracked as backlog issues).
# ============================================================================
set -euo pipefail

ARCH="${1:?usage: tarball.sh <arch> <libc> <stage-subdir> [out-dir]}"
LIBC="${2:?missing libc}"
STAGE_SUB="${3:?missing stage-subdir}"

ROOT_DIR="$(git rev-parse --show-toplevel)"
OUT_DIR="${4:-$ROOT_DIR/dist/tarballs}"
: "${PKG_VERSION:?PKG_VERSION must be set}"

STAGE_DIR="$ROOT_DIR/dist/bin/$STAGE_SUB"
[[ -d "$STAGE_DIR" ]] || {
	echo "error: stage dir not found: $STAGE_DIR" >&2
	exit 1
}

name="wmaker-ng-$PKG_VERSION-$ARCH-$LIBC"
mkdir -p "$OUT_DIR"
stagetmp="$(mktemp -d)"
trap 'rm -rf "$stagetmp"' EXIT

mkdir -p "$stagetmp/$name"
cp "$STAGE_DIR"/* "$stagetmp/$name/"
for doc in README.md ARCHITECTURE.md ROADMAP.md; do
	[[ -f "$ROOT_DIR/$doc" ]] && cp "$ROOT_DIR/$doc" "$stagetmp/$name/"
done

tar -C "$stagetmp" -czf "$OUT_DIR/$name.tar.gz" "$name"
(cd "$OUT_DIR" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
echo "==> $OUT_DIR/$name.tar.gz" >&2
