#!/usr/bin/env bash
# ============================================================================
# secret-scan.sh — block secrets before they enter history (PLAN §7).
#
# Primary: gitleaks (industry-standard, low false-positive) over the staged
# change set. Fallback: a conservative built-in regex scan when gitleaks is not
# installed, so the gate still does *something* — but gitleaks is the supported
# path (CI enforces it; install with `make install-dev-tools`).
# ============================================================================
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

# ── Primary: gitleaks ────────────────────────────────────────────────────────
if command -v gitleaks >/dev/null 2>&1; then
	echo "secret-scan: gitleaks (staged)" >&2
	# Uses ./.gitleaks.toml automatically; --redact keeps any match out of logs.
	exec gitleaks protect --staged --redact --no-banner
fi

# ── Fallback: built-in regex scan ────────────────────────────────────────────
echo "secret-scan: gitleaks not found — using built-in fallback (install gitleaks: make install-dev-tools)" >&2

patterns=(
	'AKIA[0-9A-Z]{16}'
	'-----BEGIN [A-Z ]*PRIVATE KEY-----'
	'(secret|password|passwd|token|api[_-]?key)[[:space:]]*[:=][[:space:]]*['"'"'"][^'"'"'"]{8,}['"'"'"]'
)

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
