#!/usr/bin/env sh
# ============================================================================
# install-agents-md.sh — idempotently add (or refresh) the wmaker-ai guidance
# block in an AGENTS.md, between the <!-- wmaker-ai:begin --> / :end markers.
#
#   Usage: install-agents-md.sh [TARGET_AGENTS_MD]
#     TARGET_AGENTS_MD  file to edit (default: ./AGENTS.md; created if absent)
#
# Behavior:
#   - target absent        → it is created holding just the block
#   - no markers present   → the block is appended
#   - markers present      → the block between them is replaced in place
# Safe to run repeatedly; the block is never duplicated.
#
# The block content is sourced from AGENTS.wmaker-ai.md next to this script,
# or from `ai-mcp print-agents-md` when that file is not co-located (e.g. a
# packaged install). Both emit the same marker-wrapped block.
# ============================================================================
set -eu

target="${1:-AGENTS.md}"
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
snippet="$here/AGENTS.wmaker-ai.md"

work="$(mktemp -d "${TMPDIR:-/tmp}/wmaker-ai-agents.XXXXXX")"
trap 'rm -rf "$work"' EXIT

block="$work/block.md"
if [ -r "$snippet" ]; then
	cat "$snippet" >"$block"
elif command -v ai-mcp >/dev/null 2>&1; then
	ai-mcp print-agents-md >"$block"
else
	echo "error: need AGENTS.wmaker-ai.md beside this script or an ai-mcp on PATH" >&2
	exit 1
fi

begin='<!-- wmaker-ai:begin -->'
end='<!-- wmaker-ai:end -->'

if [ ! -e "$target" ]; then
	cp "$block" "$target"
	echo "created $target with the wmaker-ai block"
	exit 0
fi

if grep -qF "$begin" "$target" && grep -qF "$end" "$target"; then
	# Split the file around the marked block, then re-splice the fresh block.
	awk -v b="$begin" -v e="$end" -v bf="$work/before" -v af="$work/after" '
		index($0, b) > 0 { section = "skip"; next }
		index($0, e) > 0 { section = "after"; next }
		section == "after" { print >af; next }
		{ print >bf }
	' "$target"
	: >"$target"
	[ -f "$work/before" ] && cat "$work/before" >>"$target"
	cat "$block" >>"$target"
	[ -f "$work/after" ] && cat "$work/after" >>"$target"
	echo "refreshed the wmaker-ai block in $target"
else
	printf '\n' >>"$target"
	cat "$block" >>"$target"
	echo "appended the wmaker-ai block to $target"
fi
