# Helm channel

The `charts/wmaker-ng` chart is the Kubernetes distribution lane for the
headed desktop automation stack.

Profiles:

- `sandbox`: CPU-only `Xvfb + Window Maker + ai-mcp`.
- `browser`: sandbox plus browser automation image.
- `gpu`: render-gated profile for future accelerated desktop/Blender work.

Render checks:

```bash
scripts/check-helm-chart.sh
```

When Helm is installed the script runs `helm lint` and renders all three
profiles. Without Helm it performs a repository-shape check so CI can still
protect the chart skeleton on minimal runners.

The GPU profile is intentionally dry-run only until the Kubernetes GPU/display
runtime contract is proven in `dagobah-infra#286`.
