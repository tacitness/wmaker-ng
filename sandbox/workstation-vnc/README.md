# wmaker-ai-workstation-vnc (#92)

`wmaker-ai-workstation-vnc` is a thin opt-in overlay above
`wmaker-ai-workstation`:

```text
wmaker-crm:headless
  -> wmaker-ai-sandbox
     -> wmaker-ai-browser
        -> wmaker-ai-workstation
           -> wmaker-ai-workstation-vnc
```

The overlay exists so Kubernetes or bridge workloads can add supervised VNC
without rebuilding the multi-gigabyte workstation application layer.

## Build

```bash
make sandbox-workstation-vnc-image
```

## Runtime contract

- requires `VNC_PASSWORD_FILE` pointing at a readable mounted secret
- serves the inherited X display (`DISPLAY`, default `:99`) through `x11vnc`
  on port `5900`
- serves the co-located streamable HTTP MCP bridge on port `8090` by default
- keeps the workstation desktop and app launchers in the same container so
  routed launches still see `/profile` and `/workspace`
- does not log the VNC password

The base workstation stdin MCP contract remains unchanged; use the overlay only
for the VNC/HTTP bridge lane.

## Run

```bash
docker run -d --rm \
  -p 5900:5900 \
  -p 8090:8090 \
  -v "$PWD/profile:/profile" \
  -v "$PWD/workspace:/workspace" \
  -v "$PWD/vnc-passwd:/run/secrets/wmaker-vnc-password:ro" \
  -e VNC_PASSWORD_FILE=/run/secrets/wmaker-vnc-password \
  wmaker-ai-workstation-vnc
```

## Smoke

```bash
make smoke-workstation-vnc
```
