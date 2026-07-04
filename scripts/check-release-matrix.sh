#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

paths=(
	docs/release-package-matrix.md
	packaging/aur/wmaker-ng-bin/PKGBUILD
	packaging/aur/wmaker-ai-bin/PKGBUILD
	packaging/gentoo/x11-wm/wmaker-ng/wmaker-ng-0.1.0.ebuild
	packaging/gentoo/x11-wm/wmaker-ai/wmaker-ai-0.1.0.ebuild
	packaging/repo/wmaker-ng.repo
	charts/wmaker-ng/Chart.yaml
	scripts/check-helm-chart.sh
)

for path in "${paths[@]}"; do
	test -s "$path" || {
		echo "missing release matrix path: $path" >&2
		exit 1
	}
done

grep -q 'el8' Makefile
grep -q 'el9' Makefile
grep -q 'wmaker-ai-sandbox' sandbox/README.md
echo "release matrix shape check passed"
