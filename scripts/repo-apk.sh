#!/usr/bin/env bash
# ============================================================================
# repo-apk.sh — assemble a signed Alpine (apk) repository, lineage-first
# (dagobah-infra#305): <repo-dir>/<branch>/<component>/<arch>/APKINDEX.tar.gz.
#
# Usage: scripts/repo-apk.sh <repo-dir> <pkg-dir>
#   repo-dir: output base for the apk repo (e.g. dist/repo/apk)
#   pkg-dir:  directory containing the built *.apk files
#
# Env: ABUILD_KEY    — path to the abuild RSA *private* key. If set, each per-arch
#                      APKINDEX is signed with abuild-sign and the matching .pub
#                      is exported. If unset, the index is built unsigned (dev).
#      APK_BRANCH    — repo branch segment (default v3.20). apk treats it as an
#                      opaque path element; our musl-static builds are version-
#                      agnostic, so one branch serves current Alpine releases.
#      APK_COMPONENT — repo component segment (default main).
#      KEYS_DIR      — shared trust-anchor dir (default: dist/repo/keys).
#
# apk appends "/<arch>/APKINDEX.tar.gz" to the repo line itself, so the client
# URL ends at <branch>/<component>. Must run where apk + abuild-sign exist (an
# Alpine container in CI). apk uses RSA index signing — a *separate* key from
# the GPG key used for apt/rpm.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: repo-apk.sh <repo-dir> <pkg-dir>}"
PKG_DIR="${2:?usage: repo-apk.sh <repo-dir> <pkg-dir>}"
ABUILD_KEY="${ABUILD_KEY:-}"
APK_BRANCH="${APK_BRANCH:-v3.20}"
APK_COMPONENT="${APK_COMPONENT:-main}"

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

# nfpm names apk files <pkg>_<ver>_<arch>.apk (arch: x86_64 | aarch64). Both
# the version (e.g. 0.1.0_rc.4) and x86_64 contain underscores, so match the
# known arch suffixes instead of splitting on '_'.
arches="$(for f in "${apks[@]}"; do
	case "$f" in
	*_x86_64.apk) echo x86_64 ;;
	*_aarch64.apk) echo aarch64 ;;
	*)
		echo "error: cannot determine arch of $f" >&2
		exit 1
		;;
	esac
done | sort -u)"

for arch in $arches; do
	dest="$REPO_DIR/$APK_BRANCH/$APK_COMPONENT/$arch"
	mkdir -p "$dest"
	cp -f "$PKG_DIR"/*_"$arch".apk "$dest/"
	echo "==> apk index ($APK_BRANCH/$APK_COMPONENT/$arch)" >&2
	(
		cd "$dest"
		# --allow-untrusted: the .apk files are unsigned by design; the trust
		# anchor is the abuild-signed APKINDEX below.
		apk index --allow-untrusted --rewrite-arch "$arch" -o APKINDEX.tar.gz ./*.apk
		if [[ -n "$ABUILD_KEY" ]]; then
			abuild-sign -k "$ABUILD_KEY" APKINDEX.tar.gz
			echo "==> apk index ($arch) signed" >&2
		else
			echo "==> apk index ($arch) UNSIGNED (ABUILD_KEY unset)" >&2
		fi
	)
done

# The RSA public half is published once, centrally, to the shared /keys/ root by
# the release workflow. apk verifies by matching the signature's embedded key
# name to a file in /etc/apk/keys/, so the published name equals the signing
# key's basename — name the key "tacitsoft-apk.rsa" for keys/tacitsoft-apk.rsa.pub.
