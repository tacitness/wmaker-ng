# Helm channel

The `charts/wmaker-ng` chart is the Kubernetes distribution lane for the
headed desktop automation stack.

Profiles:

- `sandbox`: CPU-only `Xvfb + Window Maker + ai-mcp`.
- `browser`: sandbox plus browser automation image.
- `workstation`: browser plus terminal, Chromium, Blender, office/image tools,
  and the `/profile` + `/workspace` persistence split.
- `gpu`: render-gated profile for future accelerated desktop/Blender work.

Workstation chart contract:

- pod tokens stay disabled with `automountServiceAccountToken: false`
- pod seccomp stays on `RuntimeDefault`
- containers drop all Linux capabilities and disallow privilege escalation
- `/profile` remains the single-writer browser identity PVC
- `/workspace` is a separate writable PVC for app state and artifacts
- the optional `wmaker-ai-workstation-vnc` overlay requires
  `VNC_PASSWORD_FILE`, exposes the existing X display through `x11vnc` on port
  5900, and exposes its co-located streamable-HTTP MCP bridge on port 8090 so
  routed app launches execute in the workstation filesystem; the base
  workstation's normal stdin MCP contract remains unchanged
- the workstation values file raises requests/limits to `2/6` vCPU,
  `4/12Gi` memory, and `8/16Gi` ephemeral storage

Render checks:

```bash
scripts/check-helm-chart.sh
```

When Helm is installed the script runs `helm lint` and renders the sandbox,
browser, workstation, and gpu variants. Without Helm it performs a
repository-shape check so CI can still
protect the chart skeleton on minimal runners.

The GPU profile is intentionally dry-run only until the Kubernetes GPU/display
runtime contract is proven in `dagobah-infra#286`.
