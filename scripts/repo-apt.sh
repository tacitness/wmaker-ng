#!/usr/bin/env bash
# ============================================================================
# repo-apt.sh — assemble a signed, multi-arch APT repository with reprepro.
#
# Usage: scripts/repo-apt.sh <repo-dir> <pkg-dir>
#   repo-dir: output base for the apt repo (e.g. dist/repo/apt)
#   pkg-dir:  directory containing the built *.deb files
#
# Env: GPG_KEY_ID — if set, reprepro signs Release (→ InRelease + Release.gpg)
#                   and the public key is exported next to the repo.
#                   If unset, the repo is built unsigned (local/dev).
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: repo-apt.sh <repo-dir> <pkg-dir>}"
PKG_DIR="${2:?usage: repo-apt.sh <repo-dir> <pkg-dir>}"
GPG_KEY_ID="${GPG_KEY_ID:-}"

command -v reprepro >/dev/null 2>&1 || {
	echo "error: reprepro not found" >&2
	exit 1
}

mkdir -p "$REPO_DIR/conf"

{
	echo "Origin: TacitSoft"
	echo "Label: wmaker-ng"
	echo "Suite: stable"
	echo "Codename: stable"
	echo "Architectures: amd64 arm64"
	echo "Components: main"
	echo "Description: wmaker-ng / wmaker-ai APT repository"
	[[ -n "$GPG_KEY_ID" ]] && echo "SignWith: $GPG_KEY_ID"
} >"$REPO_DIR/conf/distributions"

shopt -s nullglob
debs=("$PKG_DIR"/*.deb)
[[ ${#debs[@]} -gt 0 ]] || {
	echo "error: no .deb files in $PKG_DIR" >&2
	exit 1
}

for deb in "${debs[@]}"; do
	echo "==> reprepro includedeb stable $(basename "$deb")" >&2
	reprepro -b "$REPO_DIR" includedeb stable "$deb"
done

if [[ -n "$GPG_KEY_ID" ]]; then
	gpg --batch --yes --armor --export "$GPG_KEY_ID" >"$REPO_DIR/wmaker-ng-archive-keyring.asc"
	echo "==> apt repo signed with $GPG_KEY_ID" >&2
else
	echo "==> apt repo built UNSIGNED (GPG_KEY_ID unset)" >&2
fi
