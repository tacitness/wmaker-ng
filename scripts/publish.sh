#!/usr/bin/env bash
# ============================================================================
# publish.sh — sync the assembled repos to the repos.tacitsoft.dev S3 bucket.
#
# Usage: scripts/publish.sh <repo-dir>
#   repo-dir: the assembled repo tree (e.g. dist/repo) with lineage subtrees
#             apt/ rpm/ apk/ releases/ and a shared keys/ dir.
#
# Env: REPOS_BUCKET — S3 bucket backing repos.tacitsoft.dev (required; CI gates
#                     the publish step on the repo variable of the same name).
#      REPOS_PREFIX (default empty) — optional key prefix inside the bucket.
#
# LINEAGE-FIRST layout (dagobah-infra#305 / SDD-305): packages publish to shared
# lineage roots at the bucket root — /apt, /rpm, /apk, /releases, /keys — NOT
# under a /wmaker-ng/ product prefix. The product is selected by package name,
# not by a path segment (HashiCorp/Grafana/Docker model).
#
# SINGLE-PRODUCER BRIDGE: while wmaker-ng is the only producer it may write the
# shared lineage roots directly. `aws s3 sync --delete` is scoped PER-LINEAGE
# (never the bucket root) so keys/ and index.html are never pruned. Once a
# SECOND producer (e.g. tsctl) shares the pool, this must move to the
# infra-owned _incoming/ + repo-indexer model — a producer's --delete would
# otherwise prune the other producer's packages from the shared apt pool /
# rpm repodata.
#
# reprepro's conf/ + db/ working state is excluded — only dists/ + pool/ ship.
# CloudFront (OAC) fronts the private bucket; CI reaches it with `aws s3 sync`
# under the release OIDC role (no SSH host, no deploy key).
# ============================================================================
set -euo pipefail

REPO_DIR="${1:?usage: publish.sh <repo-dir>}"
REPOS_BUCKET="${REPOS_BUCKET:?REPOS_BUCKET not set — infra has not activated the repo bucket yet}"
REPOS_PREFIX="${REPOS_PREFIX:-}"

[[ -d "$REPO_DIR" ]] || {
	echo "error: repo dir not found: $REPO_DIR" >&2
	exit 1
}

# Normalise an optional prefix to "" or "<prefix>/".
dest_base="s3://$REPOS_BUCKET/${REPOS_PREFIX:+$REPOS_PREFIX/}"

shopt -s nullglob
for lineage_dir in "$REPO_DIR"/*/; do
	lineage="$(basename "$lineage_dir")"
	case "$lineage" in
	keys)
		# Trust anchors are shared across all producers — never prune them.
		echo "==> publishing keys → ${dest_base}keys/ (no --delete)" >&2
		aws s3 sync --no-progress "$lineage_dir" "${dest_base}keys/"
		;;
	*)
		# Per-lineage --delete confines pruning to this producer's own lineage
		# tree; --exclude drops reprepro's private conf/ + db/ working state.
		echo "==> publishing $lineage → ${dest_base}${lineage}/" >&2
		aws s3 sync --delete --no-progress \
			--exclude "conf/*" --exclude "db/*" \
			"$lineage_dir" "${dest_base}${lineage}/"
		;;
	esac
done
echo "==> published" >&2
