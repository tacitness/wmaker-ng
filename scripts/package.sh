#!/usr/bin/env bash
# ============================================================================
# package.sh — build deb/rpm/apk for both packages from staged binaries.
#
# Usage: scripts/package.sh <format> <arch> <stage-subdir> [out-dir]
#   format: deb | rpm | apk
#   arch:   amd64 | arm64        (nfpm maps to each packager's convention)
#   stage:  dist/bin/<stage-subdir> holding the compiled binaries
#
# Env: PKG_VERSION (required) — semver from the git tag, set by the Makefile.
#      PKG_DIST  (rpm only)  — ABI-floor dist tag (el8 | el9) → Release 1.<dist>.
#
# nfpm's env expansion is unreliable for `contents.src`, so we render the recipe
# with envsubst first (only our three known vars), then package.
# ============================================================================
set -euo pipefail

FORMAT="${1:?usage: package.sh <format> <arch> <stage-subdir> [out-dir]}"
PKG_ARCH="${2:?missing arch}"
STAGE_SUB="${3:?missing stage-subdir}"

# Repo root; falls back to cwd inside the CI assembly containers (no git,
# and root-vs-runner ownership would trip git's dubious-ownership check).
ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT_DIR="${4:-$ROOT_DIR/dist/pkg}"
: "${PKG_VERSION:?PKG_VERSION must be set (derive from git tag via the Makefile)}"

# rpm gets a dist-tagged Release (1.el8 / 1.el9 via PKG_DIST); deb/apk get
# none so their artifact names stay bare (nfpm treats empty as unset).
PKG_RELEASE=""
if [[ "$FORMAT" == rpm ]]; then
	PKG_RELEASE="1${PKG_DIST:+.$PKG_DIST}"
fi

export PKG_ARCH PKG_VERSION PKG_RELEASE
export WMNG_STAGE="$ROOT_DIR/dist/bin/$STAGE_SUB"

[[ -d "$WMNG_STAGE" ]] || {
	echo "error: stage dir not found: $WMNG_STAGE (run scripts/build.sh first)" >&2
	exit 1
}

command -v nfpm >/dev/null 2>&1 || {
	echo "error: nfpm not found (go install github.com/goreleaser/nfpm/v2/cmd/nfpm@latest)" >&2
	exit 1
}

mkdir -p "$OUT_DIR"
render_dir="$(mktemp -d)"
trap 'rm -rf "$render_dir"' EXIT

for pkg in wmaker-ng wmaker-ai; do
	rendered="$render_dir/$pkg.yaml"
	# Single-quoted arg is intentional: envsubst takes a literal var list.
	# shellcheck disable=SC2016
	envsubst '${PKG_VERSION} ${PKG_ARCH} ${PKG_RELEASE} ${WMNG_STAGE}' \
		<"$ROOT_DIR/packaging/nfpm/$pkg.yaml" >"$rendered"
	echo "==> nfpm $FORMAT: $pkg $PKG_VERSION ($PKG_ARCH)" >&2
	nfpm package --config "$rendered" --packager "$FORMAT" --target "$OUT_DIR/"
done
