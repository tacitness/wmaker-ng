#!/usr/bin/env bash
# ============================================================================
# repo-rpm.sh — assemble a signed RPM/YUM repository with createrepo_c.
#
# Usage: scripts/repo-rpm.sh <repo-dir> <pkg-dir>
#   repo-dir: output base for the rpm repo (e.g. dist/repo/rpm)
#   pkg-dir:  directory containing the built *.rpm files
#
# Env: GPG_KEY_ID — if set, each package is GPG-signed (rpm --addsign) and the
#                   repomd.xml is detached-signed; the public key is exported.
#                   If unset, packages/repo are unsigned (local/dev).
#
# Covers EL9, EL10, and Fedora (latest-2) — one glibc-2.34 build is forward
# compatible across all of them.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: repo-rpm.sh <repo-dir> <pkg-dir>}"
PKG_DIR="${2:?usage: repo-rpm.sh <repo-dir> <pkg-dir>}"
GPG_KEY_ID="${GPG_KEY_ID:-}"

command -v createrepo_c >/dev/null 2>&1 || {
	echo "error: createrepo_c not found" >&2
	exit 1
}

mkdir -p "$REPO_DIR"
shopt -s nullglob
rpms=("$PKG_DIR"/*.rpm)
[[ ${#rpms[@]} -gt 0 ]] || {
	echo "error: no .rpm files in $PKG_DIR" >&2
	exit 1
}
cp -f "${rpms[@]}" "$REPO_DIR/"

if [[ -n "$GPG_KEY_ID" ]]; then
	echo "==> signing rpm packages with $GPG_KEY_ID" >&2
	rpm --define "_gpg_name $GPG_KEY_ID" \
		--define "_gpg_sign_cmd_extra_args --pinentry-mode loopback" \
		--addsign "$REPO_DIR"/*.rpm
fi

echo "==> createrepo_c $REPO_DIR" >&2
createrepo_c --update "$REPO_DIR"

if [[ -n "$GPG_KEY_ID" ]]; then
	gpg --batch --yes --detach-sign --armor "$REPO_DIR/repodata/repomd.xml"
	gpg --batch --yes --armor --export "$GPG_KEY_ID" >"$REPO_DIR/RPM-GPG-KEY-wmaker-ng"
	echo "==> rpm repo signed" >&2
else
	echo "==> rpm repo built UNSIGNED (GPG_KEY_ID unset)" >&2
fi
