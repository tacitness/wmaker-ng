#!/usr/bin/env bash
# ============================================================================
# repo-rpm.sh — assemble signed RPM/YUM repositories with createrepo_c,
# one repodata/ PER basearch (lineage-first layout, dagobah-infra#305).
#
# Usage: scripts/repo-rpm.sh <repo-dir> <pkg-dir> [glob]
#   repo-dir: output base for one EL major (e.g. dist/repo/rpm/el/9); per-arch
#             subdirs <repo-dir>/<basearch>/ are created underneath.
#   pkg-dir:  directory containing the built *.rpm files
#   glob:     optional filename glob so ABI floors assemble separately
#             (e.g. '*.el8.*.rpm' → rpm/el/8, '*.el9.*.rpm' → rpm/el/9).
#
# Env: GPG_KEY_ID — if set, each package is GPG-signed (rpm --addsign) and every
#                   per-arch repomd.xml is detached-signed; the public key is
#                   exported to the shared keys dir. If unset, unsigned (dev).
#      KEYS_DIR   — shared trust-anchor dir (default: the sibling keys/ under the
#                   assembled repo root, e.g. dist/repo/keys).
#
# dnf substitutes $basearch at query time, so each arch needs its OWN dir +
# repodata (never mix arches in one index). noarch packages are folded into
# every arch tree.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: repo-rpm.sh <repo-dir> <pkg-dir> [glob]}"
PKG_DIR="${2:?usage: repo-rpm.sh <repo-dir> <pkg-dir> [glob]}"
RPM_GLOB="${3:-*.rpm}"
GPG_KEY_ID="${GPG_KEY_ID:-}"

command -v createrepo_c >/dev/null 2>&1 || {
	echo "error: createrepo_c not found" >&2
	exit 1
}

mkdir -p "$REPO_DIR"
shopt -s nullglob
rpms=("$PKG_DIR"/$RPM_GLOB)
[[ ${#rpms[@]} -gt 0 ]] || {
	echo "error: no rpms matching $RPM_GLOB in $PKG_DIR" >&2
	exit 1
}

# Sign the source rpms in place *before* copying, so every downstream consumer
# (GitHub Release assets and the yum repo alike) gets the same GPG-signed
# packages — not just the repo copies.
if [[ -n "$GPG_KEY_ID" ]]; then
	echo "==> signing rpm packages with $GPG_KEY_ID" >&2
	rpm --define "_gpg_name $GPG_KEY_ID" \
		--define "_gpg_sign_cmd_extra_args --pinentry-mode loopback" \
		--addsign "${rpms[@]}"
fi

# Fan out by basearch: <repo-dir>/<basearch>/ (nfpm names files
# <name>-<ver>-<rel>.<arch>.rpm, arch ∈ x86_64 | aarch64 | noarch).
for rpm in "${rpms[@]}"; do
	case "$rpm" in
	*.x86_64.rpm) arch=x86_64 ;;
	*.aarch64.rpm) arch=aarch64 ;;
	*.noarch.rpm) arch=noarch ;;
	*)
		echo "error: cannot determine arch of $rpm" >&2
		exit 1
		;;
	esac
	mkdir -p "$REPO_DIR/$arch"
	cp -f "$rpm" "$REPO_DIR/$arch/"
done

# noarch packages must appear in every basearch index (dnf won't consult a
# separate noarch/ dir). Fold them in, then drop the staging dir.
if [[ -d "$REPO_DIR/noarch" ]]; then
	for arch_dir in "$REPO_DIR"/*/; do
		[[ "$(basename "$arch_dir")" == noarch ]] && continue
		cp -f "$REPO_DIR/noarch"/*.rpm "$arch_dir" 2>/dev/null || true
	done
	rm -rf "$REPO_DIR/noarch"
fi

for arch_dir in "$REPO_DIR"/*/; do
	echo "==> createrepo_c $arch_dir" >&2
	createrepo_c --update "$arch_dir"
	if [[ -n "$GPG_KEY_ID" ]]; then
		gpg --batch --yes --detach-sign --armor "$arch_dir/repodata/repomd.xml"
	fi
done

# The GPG public key is published once, centrally, to the shared /keys/ root by
# the release workflow — not into each rpm tree.
if [[ -n "$GPG_KEY_ID" ]]; then
	echo "==> rpm repos signed with $GPG_KEY_ID" >&2
else
	echo "==> rpm repos built UNSIGNED (GPG_KEY_ID unset)" >&2
fi
