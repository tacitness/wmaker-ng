#!/usr/bin/env bash
# ============================================================================
# publish.sh — hand packages to the repos.tacitsoft.dev repo-indexer.
#
# Usage: scripts/publish.sh <pkg-dir> <releases-dir>
#   pkg-dir:      built packages (dist/pkg: *.deb, *.el{8,9}.*.rpm, *.apk)
#   releases-dir: the static tar.zst channel (dist/repo/releases/wmaker-ng)
#
# Env: REPOS_BUCKET  — S3 bucket backing repos.tacitsoft.dev (required).
#      APK_BRANCH    (default v3.20) / APK_COMPONENT (default main)
#
# INCOMING MODEL (dagobah-infra#306, SDD-305 §5): this producer does NOT write
# shared repo metadata and holds NO signing keys. It stages packages into the
# _incoming/ contract shape and uploads them; the infra-owned repo-indexer —
# the single writer — folds them into the shared pool, rebuilds and signs all
# indices (including rpm --addsign), and publishes the lineage roots.
#
#   _incoming/wmaker-ng/apt/<pkg>_<ver>_<arch>.deb          (flat; pooled)
#   _incoming/wmaker-ng/rpm/el/<major>/<basearch>/<pkg>.rpm
#   _incoming/wmaker-ng/apk/<branch>/<component>/<arch>/<pkg>.apk
#
# The releases/ lineage is product-scoped (no shared metadata), so it is
# synced directly to releases/wmaker-ng/ — additive only, never --delete.
# The IAM grant covers exactly these two prefixes (wmaker_ng_publish_mode =
# "incoming" in dagobah-infra prod).
# ============================================================================
set -euo pipefail

PKG_DIR="${1:?usage: publish.sh <pkg-dir> <releases-dir>}"
RELEASES_DIR="${2:?usage: publish.sh <pkg-dir> <releases-dir>}"
REPOS_BUCKET="${REPOS_BUCKET:?REPOS_BUCKET not set}"
APK_BRANCH="${APK_BRANCH:-v3.20}"
APK_COMPONENT="${APK_COMPONENT:-main}"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

shopt -s nullglob

# apt: flat — the indexer's reprepro pools them.
debs=("$PKG_DIR"/*.deb)
if [[ ${#debs[@]} -gt 0 ]]; then
	mkdir -p "$STAGE/apt"
	cp -f "${debs[@]}" "$STAGE/apt/"
fi

# rpm: el/<major>/<basearch>/ per the incoming contract (arch from filename).
for rpm in "$PKG_DIR"/*.el*.rpm; do
	base="$(basename "$rpm")"
	major="${base##*.el}"
	major="${major%%.*}"
	case "$base" in
	*.x86_64.rpm) arch=x86_64 ;;
	*.aarch64.rpm) arch=aarch64 ;;
	*.noarch.rpm) arch=noarch ;;
	*)
		echo "error: cannot determine arch of $base" >&2
		exit 1
		;;
	esac
	mkdir -p "$STAGE/rpm/el/$major/$arch"
	cp -f "$rpm" "$STAGE/rpm/el/$major/$arch/"
done

# apk: <branch>/<component>/<arch>/ (arch suffix match — version strings and
# x86_64 both contain underscores, so never split on '_').
for apk in "$PKG_DIR"/*.apk; do
	case "$apk" in
	*_x86_64.apk) arch=x86_64 ;;
	*_aarch64.apk) arch=aarch64 ;;
	*)
		echo "error: cannot determine arch of $(basename "$apk")" >&2
		exit 1
		;;
	esac
	mkdir -p "$STAGE/apk/$APK_BRANCH/$APK_COMPONENT/$arch"
	cp -f "$apk" "$STAGE/apk/$APK_BRANCH/$APK_COMPONENT/$arch/"
done

echo "==> handing off packages → s3://$REPOS_BUCKET/_incoming/wmaker-ng/" >&2
aws s3 sync --no-progress "$STAGE/" "s3://$REPOS_BUCKET/_incoming/wmaker-ng/"

if [[ -d "$RELEASES_DIR" ]]; then
	echo "==> publishing releases → s3://$REPOS_BUCKET/releases/wmaker-ng/ (additive)" >&2
	aws s3 sync --no-progress "$RELEASES_DIR/" "s3://$REPOS_BUCKET/releases/wmaker-ng/"
fi

echo "==> handoff complete — the repo-indexer sweeps hourly; for immediacy:" >&2
echo "    gh workflow run repo-indexer.yml -R tacitness/dagobah-infra" >&2
