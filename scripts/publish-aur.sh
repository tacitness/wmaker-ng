#!/usr/bin/env bash
# ============================================================================
# publish-aur.sh — render and push AUR -bin packages from release tarballs.
#
# Usage:
#   scripts/publish-aur.sh <tarball-dir> <version>
#
# Required tarballs:
#   wmaker-ng-<version>-amd64-gnu.tar.gz[.sha256]
#   wmaker-ng-<version>-arm64-gnu.tar.gz[.sha256]
#
# Set AUR_DRY_RUN=1 to render packages under dist/aur without cloning/pushing.
# ============================================================================
set -euo pipefail

TARBALL_DIR="${1:?usage: publish-aur.sh <tarball-dir> <version>}"
PKGVER="${2:?missing version}"

ROOT_DIR="$(git rev-parse --show-toplevel)"
AUR_BASE="${AUR_BASE:-ssh://aur@aur.archlinux.org}"
AUR_DRY_RUN="${AUR_DRY_RUN:-0}"

amd64_tar="wmaker-ng-$PKGVER-amd64-gnu.tar.gz"
arm64_tar="wmaker-ng-$PKGVER-arm64-gnu.tar.gz"
amd64_sum="$TARBALL_DIR/$amd64_tar.sha256"
arm64_sum="$TARBALL_DIR/$arm64_tar.sha256"

[[ -f "$TARBALL_DIR/$amd64_tar" ]] || {
	echo "error: missing tarball: $TARBALL_DIR/$amd64_tar" >&2
	exit 1
}
[[ -f "$TARBALL_DIR/$arm64_tar" ]] || {
	echo "error: missing tarball: $TARBALL_DIR/$arm64_tar" >&2
	exit 1
}
[[ -f "$amd64_sum" ]] || {
	echo "error: missing checksum: $amd64_sum" >&2
	exit 1
}
[[ -f "$arm64_sum" ]] || {
	echo "error: missing checksum: $arm64_sum" >&2
	exit 1
}

AMD64_SHA="$(awk '{print $1}' "$amd64_sum")"
ARM64_SHA="$(awk '{print $1}' "$arm64_sum")"

render_package() {
	local pkg="$1"
	local src="$ROOT_DIR/packaging/aur/$pkg"
	local dst="$2"

	mkdir -p "$dst"
	cp "$src/PKGBUILD" "$dst/PKGBUILD"
	cp "$src/.SRCINFO" "$dst/.SRCINFO"

	sed -i \
		-e "s/^pkgver=.*/pkgver=$PKGVER/" \
		-e "s#wmaker-ng-[0-9][^-/]*-amd64-gnu.tar.gz#wmaker-ng-$PKGVER-amd64-gnu.tar.gz#g" \
		-e "s#wmaker-ng-[0-9][^-/]*-arm64-gnu.tar.gz#wmaker-ng-$PKGVER-arm64-gnu.tar.gz#g" \
		-e "s#/v[0-9][^/]*/#/v$PKGVER/#g" \
		-e "s/sha256sums_x86_64=(.*/sha256sums_x86_64=('$AMD64_SHA')/" \
		-e "s/sha256sums_aarch64=(.*/sha256sums_aarch64=('$ARM64_SHA')/" \
		"$dst/PKGBUILD"

	sed -i \
		-e "s/^\tpkgver = .*/\tpkgver = $PKGVER/" \
		-e "s#wmaker-ng-[0-9][^-/]*-amd64-gnu.tar.gz#wmaker-ng-$PKGVER-amd64-gnu.tar.gz#g" \
		-e "s#wmaker-ng-[0-9][^-/]*-arm64-gnu.tar.gz#wmaker-ng-$PKGVER-arm64-gnu.tar.gz#g" \
		-e "s#/v[0-9][^/]*/#/v$PKGVER/#g" \
		-e "s/^\tsha256sums_x86_64 = .*/\tsha256sums_x86_64 = $AMD64_SHA/" \
		-e "s/^\tsha256sums_aarch64 = .*/\tsha256sums_aarch64 = $ARM64_SHA/" \
		"$dst/.SRCINFO"
}

if [[ "$AUR_DRY_RUN" == "1" ]]; then
	out="$ROOT_DIR/dist/aur"
	rm -rf "$out"
	for pkg in wmaker-ng-bin wmaker-ai-bin; do
		render_package "$pkg" "$out/$pkg"
		echo "==> rendered $out/$pkg" >&2
	done
	exit 0
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

git config --global user.name "${GIT_AUTHOR_NAME:-wmaker-ng release bot}"
git config --global user.email "${GIT_AUTHOR_EMAIL:-ops@tacitsoft.dev}"

for pkg in wmaker-ng-bin wmaker-ai-bin; do
	repo="$workdir/$pkg"
	git clone "$AUR_BASE/$pkg.git" "$repo"
	render_package "$pkg" "$repo"
	(
		cd "$repo"
		git add PKGBUILD .SRCINFO
		if git diff --cached --quiet; then
			echo "==> $pkg unchanged" >&2
		else
			git commit -m "Update to v$PKGVER"
			git push origin HEAD:master
		fi
	)
done
