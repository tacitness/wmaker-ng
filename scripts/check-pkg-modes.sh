#!/usr/bin/env bash
# ============================================================================
# check-pkg-modes.sh — fail if any packaged /usr/bin payload is not 0755.
#
# Usage: scripts/check-pkg-modes.sh <pkg-dir>
#
# Guards the nfpm file_info.mode regression (#68): RPM payloads landed
# non-executable because the recipes never set modes. Inspects every built
# deb (dpkg-deb), rpm (rpm2cpio|cpio) and apk (tar) in <pkg-dir>.
# Needs: dpkg-deb, rpm2cpio, cpio, tar — present in the CI packaging
# container (debian + rpm + cpio) and on any Debian dev box.
# ============================================================================
set -euo pipefail

PKG_DIR="${1:?usage: check-pkg-modes.sh <pkg-dir>}"
fail=0

check_line() { # <pkg-file> <mode-string> <path>
	local pkg="$1" mode="$2" path="$3"
	[[ "$mode" == d* || "$path" == */ ]] && return 0 # directories are fine
	if [[ "$mode" != -rwxr-xr-x* ]]; then
		echo "error: $path in $(basename "$pkg") has mode $mode (want -rwxr-xr-x)" >&2
		fail=1
	fi
}

shopt -s nullglob
found=0
for pkg in "$PKG_DIR"/*.deb; do
	found=1
	while read -r mode _ _ _ _ path; do
		[[ "$path" == *usr/bin/* ]] && check_line "$pkg" "$mode" "$path"
	done < <(dpkg-deb -c "$pkg")
done
for pkg in "$PKG_DIR"/*.rpm; do
	found=1
	while read -r mode _ _ _ _ _ _ _ path; do
		[[ "$path" == *usr/bin/* ]] && check_line "$pkg" "$mode" "$path"
	done < <(rpm2cpio "$pkg" | cpio -tv --quiet)
done
for pkg in "$PKG_DIR"/*.apk; do
	found=1
	while read -r mode _ _ path; do
		[[ "$path" == usr/bin/* ]] && check_line "$pkg" "$mode" "$path"
	done < <(tar -tzvf "$pkg" 2>/dev/null | awk '{print $1, $2, $3, $NF}')
done

[[ $found -eq 1 ]] || {
	echo "error: no packages found in $PKG_DIR" >&2
	exit 1
}
[[ $fail -eq 0 ]] && echo "==> package modes OK: all /usr/bin payloads 0755" >&2
exit "$fail"
