#!/usr/bin/env bash
# ============================================================================
# build.sh — cross-compile the workspace binaries for one target and stage them.
#
# Usage: scripts/build.sh <rust-target> <stage-subdir>
#   e.g. scripts/build.sh x86_64-unknown-linux-gnu.2.34  amd64-gnu
#        scripts/build.sh aarch64-unknown-linux-musl       arm64-musl
#
# Uses cargo-zigbuild so a single Linux host can target both arches and pin the
# glibc floor (the `.2.34` suffix → EL9). musl targets link static. The same
# binary then feeds nfpm (glibc → deb/rpm, musl → apk).
# ============================================================================
set -euo pipefail

TARGET="${1:?usage: build.sh <rust-target> <stage-subdir>}"
STAGE_SUB="${2:?usage: build.sh <rust-target> <stage-subdir>}"

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

# The shipped binaries (PLAN §4): three ng-* daemons + the ai-mcp server.
BINS=(ng-automount ng-power ng-notify ai-mcp)

# cargo-zigbuild writes to target/<base-triple>/release (the glibc suffix is
# stripped from the directory name).
BASE_TRIPLE="${TARGET%%.*}"
STAGE_DIR="$ROOT_DIR/dist/bin/$STAGE_SUB"

echo "==> building $TARGET → dist/bin/$STAGE_SUB" >&2
rustup target add "$BASE_TRIPLE" >/dev/null 2>&1 || true

pkg_args=()
for bin in "${BINS[@]}"; do pkg_args+=(-p "$bin"); done
cargo zigbuild --release --target "$TARGET" "${pkg_args[@]}"

mkdir -p "$STAGE_DIR"
for bin in "${BINS[@]}"; do
	src="$ROOT_DIR/target/$BASE_TRIPLE/release/$bin"
	[[ -f "$src" ]] || {
		echo "error: expected binary not found: $src" >&2
		exit 1
	}
	install -m 0755 "$src" "$STAGE_DIR/$bin"
done

echo "==> staged ${#BINS[@]} binaries → $STAGE_DIR" >&2
