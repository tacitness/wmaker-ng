# Copyright 2026 The wmaker-ng Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

DESCRIPTION="MCP AI control plane for GNU Window Maker"
HOMEPAGE="https://github.com/tacitness/wmaker-ng"
SRC_URI="
	amd64? ( https://github.com/tacitness/wmaker-ng/releases/download/v${PV}/wmaker-ng-${PV}-amd64-musl.tar.zst -> wmaker-ng-${PV}-amd64-musl.tar.zst )
	arm64? ( https://github.com/tacitness/wmaker-ng/releases/download/v${PV}/wmaker-ng-${PV}-arm64-musl.tar.zst -> wmaker-ng-${PV}-arm64-musl.tar.zst )
"

LICENSE="GPL-2+"
SLOT="0"
KEYWORDS="~amd64 ~arm64"

RDEPEND="~x11-wm/wmaker-ng-${PV}"
BDEPEND="app-arch/zstd"

src_unpack() {
	local archive
	case "${ARCH}" in
		amd64) archive="${DISTDIR}/wmaker-ng-${PV}-amd64-musl.tar.zst" ;;
		arm64) archive="${DISTDIR}/wmaker-ng-${PV}-arm64-musl.tar.zst" ;;
		*) die "unsupported ARCH=${ARCH}" ;;
	esac

	mkdir -p "${WORKDIR}" || die
	zstd -dc "${archive}" | tar -xf - -C "${WORKDIR}" || die
}

src_install() {
	local stage
	case "${ARCH}" in
		amd64) stage="${WORKDIR}/wmaker-ng-${PV}-amd64-musl" ;;
		arm64) stage="${WORKDIR}/wmaker-ng-${PV}-arm64-musl" ;;
		*) die "unsupported ARCH=${ARCH}" ;;
	esac

	dobin "${stage}"/ai-mcp
	dodoc "${stage}"/README.md "${stage}"/ARCHITECTURE.md
}
