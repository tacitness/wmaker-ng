#!/usr/bin/env bash
# ============================================================================
# publish.sh — rsync the assembled repos to repos.tacitsoft.dev.
#
# Usage: scripts/publish.sh <repo-dir>
#   repo-dir: the assembled repo tree (e.g. dist/repo) containing apt/ rpm/ apk/
#
# Env: REPOS_HOST (default repos.tacitsoft.dev), REPOS_USER (default deploy),
#      REPOS_PATH (default /srv/repos/wmaker-ng).
#
# Namespaced under /wmaker-ng/ so it never collides with tsctl's repo tree on
# the shared host. SSH auth only (deploy key in CI). --delete prunes stale
# packages, matching the house pattern.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: publish.sh <repo-dir>}"
REPOS_HOST="${REPOS_HOST:-repos.tacitsoft.dev}"
REPOS_USER="${REPOS_USER:-deploy}"
REPOS_PATH="${REPOS_PATH:-/srv/repos/wmaker-ng}"

[[ -d "$REPO_DIR" ]] || {
	echo "error: repo dir not found: $REPO_DIR" >&2
	exit 1
}

echo "==> publishing $REPO_DIR → $REPOS_USER@$REPOS_HOST:$REPOS_PATH/" >&2
rsync -avz --delete \
	-e "ssh -o StrictHostKeyChecking=accept-new" \
	"$REPO_DIR/" "$REPOS_USER@$REPOS_HOST:$REPOS_PATH/"
echo "==> published" >&2
