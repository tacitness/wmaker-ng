#!/usr/bin/env bash
# ============================================================================
# secret-scan.sh — block obvious secrets before they enter history (PLAN §7).
# Scans staged changes (or the whole tree when nothing is staged) for common
# credential shapes. Conservative by design: catches the obvious, defers deep
# entropy scanning to dedicated tooling.
# ============================================================================
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

# Patterns: AWS keys, private-key headers, and generic assigned secrets/tokens.
patterns=(
	'AKIA[0-9A-Z]{16}'
	'-----BEGIN [A-Z ]*PRIVATE KEY-----'
	'(secret|password|passwd|token|api[_-]?key)[[:space:]]*[:=][[:space:]]*['"'"'"][^'"'"'"]{8,}['"'"'"]'
)

# Prefer staged diff; fall back to tracked files for manual/`make` invocation.
if git diff --cached --quiet 2>/dev/null; then
	files=$(git ls-files)
	mode="tracked files"
else
	files=$(git diff --cached --name-only --diff-filter=ACM)
	mode="staged changes"
fi

[[ -z "$files" ]] && {
	echo "secret-scan: nothing to scan"
	exit 0
}

found=0
for pat in "${patterns[@]}"; do
	# -I skips binaries; this hook never edits, only reports.
	while IFS= read -r f; do
		[[ -f "$f" ]] || continue
		if grep -InEq "$pat" -- "$f" 2>/dev/null; then
			echo "secret-scan: possible secret in $f (pattern: ${pat:0:24}...)" >&2
			grep -InE "$pat" -- "$f" 2>/dev/null | sed 's/^/  /' >&2 || true
			found=1
		fi
	done <<<"$files"
done

if [[ "$found" -ne 0 ]]; then
	echo "secret-scan: blocked — remove the secret(s) above, or whitelist via .env.example" >&2
	exit 1
fi

echo "secret-scan: clean ($mode)"
