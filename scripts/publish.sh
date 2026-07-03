#!/usr/bin/env bash
# ============================================================================
# publish.sh — sync the assembled repos to the repos.tacitsoft.dev S3 bucket.
#
# Usage: scripts/publish.sh <repo-dir>
#   repo-dir: the assembled repo tree (e.g. dist/repo) containing apt/ rpm/ apk/
#
# Env: REPOS_BUCKET — S3 bucket backing repos.tacitsoft.dev (required; CI gates
#                     the publish step on the repo variable of the same name).
#      REPOS_PREFIX (default wmaker-ng) — key prefix inside the bucket.
#
# Per SDD-126 the package tree is served from a private S3 bucket via
# CloudFront (OAC); CI reaches it with `aws s3 sync` under the same OIDC role
# used for signing — no SSH host, no deploy key. Namespaced under /wmaker-ng/
# so it never collides with other product trees in the shared bucket.
# --delete prunes stale packages, matching the house pattern.
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: publish.sh <repo-dir>}"
REPOS_BUCKET="${REPOS_BUCKET:?REPOS_BUCKET not set — infra has not activated the repo bucket yet}"
REPOS_PREFIX="${REPOS_PREFIX:-wmaker-ng}"

[[ -d "$REPO_DIR" ]] || {
	echo "error: repo dir not found: $REPO_DIR" >&2
	exit 1
}

echo "==> publishing $REPO_DIR → s3://$REPOS_BUCKET/$REPOS_PREFIX/" >&2
aws s3 sync --delete --no-progress \
	"$REPO_DIR/" "s3://$REPOS_BUCKET/$REPOS_PREFIX/"
echo "==> published" >&2
