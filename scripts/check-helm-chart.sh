#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
CHART_DIR="$ROOT_DIR/charts/wmaker-ng"

required=(
	Chart.yaml
	values.yaml
	values-browser.yaml
	values-workstation.yaml
	values-gpu.yaml
	templates/deployment.yaml
	templates/serviceaccount.yaml
)

for path in "${required[@]}"; do
	test -s "$CHART_DIR/$path" || {
		echo "missing chart file: $CHART_DIR/$path" >&2
		exit 1
	}
done

grep -q 'automountServiceAccountToken: false' "$CHART_DIR/templates/deployment.yaml"
grep -q 'mountPath: /profile' "$CHART_DIR/templates/deployment.yaml"
grep -q 'mountPath: /workspace' "$CHART_DIR/templates/deployment.yaml"
grep -q 'name: {{ include "wmaker-ng.fullname" . }}-workspace' "$CHART_DIR/templates/pvc.yaml"

if ! command -v helm >/dev/null 2>&1; then
	echo "helm not installed; chart shape check passed"
	exit 0
fi

helm lint "$CHART_DIR"
helm template wmaker-ng "$CHART_DIR" >/dev/null
helm template wmaker-ng "$CHART_DIR" -f "$CHART_DIR/values-browser.yaml" >/dev/null
helm template wmaker-ng "$CHART_DIR" -f "$CHART_DIR/values-workstation.yaml" >/dev/null
helm template wmaker-ng "$CHART_DIR" -f "$CHART_DIR/values-gpu.yaml" >/dev/null
echo "helm chart render checks passed"
