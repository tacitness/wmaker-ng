#!/usr/bin/env bash
# ============================================================================
# repo-apk.sh — assemble a signed Alpine (apk) repository.
#
# Usage: scripts/repo-apk.sh <repo-dir> <pkg-dir>
#   repo-dir: output base for the apk repo (e.g. dist/repo/apk)
#   pkg-dir:  directory containing the built *.apk files
#
# Env: ABUILD_KEY — path to the abuild RSA *private* key. If set, each per-arch
#                   APKINDEX is signed with abuild-sign and the matching .pub is
#                   exported. If unset, the index is built unsigned (local/dev).
#
# Must run where apk + abuild-sign exist (an Alpine container in CI). apk uses
# RSA index signing — a *separate* key from the GPG key used for apt/rpm.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: repo-apk.sh <repo-dir> <pkg-dir>}"
PKG_DIR="${2:?usage: repo-apk.sh <repo-dir> <pkg-dir>}"
ABUILD_KEY="${ABUILD_KEY:-}"

command -v apk >/dev/null 2>&1 || {
	echo "error: apk not found — run this in an Alpine container" >&2
	exit 1
}

shopt -s nullglob
apks=("$PKG_DIR"/*.apk)
[[ ${#apks[@]} -gt 0 ]] || {
	echo "error: no .apk files in $PKG_DIR" >&2
	exit 1
}

# nfpm names apk files <pkg>_<ver>_<arch>.apk (arch: x86_64 | aarch64).
arches="$(for f in "${apks[@]}"; do b="${f%.apk}"; echo "${b##*_}"; done | sort -u)"

for arch in $arches; do
	dest="$REPO_DIR/$arch"
	mkdir -p "$dest"
	cp -f "$PKG_DIR"/*_"$arch".apk "$dest/"
	echo "==> apk index ($arch)" >&2
	(
		cd "$dest"
		apk index --rewrite-arch "$arch" -o APKINDEX.tar.gz ./*.apk
		if [[ -n "$ABUILD_KEY" ]]; then
			abuild-sign -k "$ABUILD_KEY" APKINDEX.tar.gz
			cp -f "${ABUILD_KEY}.pub" "$REPO_DIR/" 2>/dev/null || true
			echo "==> apk index ($arch) signed" >&2
		else
			echo "==> apk index ($arch) UNSIGNED (ABUILD_KEY unset)" >&2
		fi
	)
done
