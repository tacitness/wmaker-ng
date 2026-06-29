# Daily Driving Readiness

This is the operator checklist for making `wmaker-crm` + `wmaker-ng` +
`wmaker-ai` safe to install over the current local Window Maker setup and use as
the normal OpenClaw-driven desktop.

## Definition Of Ready

`wmaker-ng` is daily-driver ready when these gates pass on the target host:

1. `wmaker-crm` builds and can replace the current `wmaker.real` without changing
   upstream C behavior outside a documented patch series.
2. `wmaker-ng` packages install cleanly and can be removed cleanly.
3. `ai-mcp` starts on the active X display and exposes input, window, screenshot,
   and dirty-region tools over stdio.
4. A scripted smoke can move/focus/resize a benign test window and return both a
   full screenshot and at least one delta observation.
5. Baseline 1 and baseline 2 can be re-run after install with the same schemas
   used by the current Window Maker control data.
6. Idle overhead is visible and acceptable: no busy CPU, no unbounded memory
   climb, and no repeated full-frame capture unless a client requests it.

## Current State

- `wmaker-crm` exists at `/data/src/tacitsoft/infrastructure/wmaker-crm`.
- Current live Window Maker is `/usr/local/bin/wmaker.real`, not RPM-owned.
- Baseline data exists in S3 under:
  - `wmaker-ng/screen-baselines/`
  - `wmaker-ng/browser-activity-baselines/`
- `ai-mcp` has the core control surface:
  - `move_mouse`
  - `click`
  - `type`
  - `key`
  - `list_windows`
  - `focus`
  - `move_resize`
  - `tile`
  - `screenshot`
  - `changed_regions`
- `changed_regions` is the primary expected win over screenshot-only driving:
  it returns XDamage-backed PNG crops, with configurable keyframe re-baselines.
- Window movement prefers EWMH when the WM advertises it and falls back to a
  direct X11 configure request for older/lightweight WMs that do not expose
  `_NET_MOVERESIZE_WINDOW`.
- Local readiness checks on 2026-06-28:
  - `DISPLAY=:9 make mcp-smoke` passed against the live desktop.
  - `wmaker-crm:headless` built locally from `/data/src/tacitsoft/infrastructure/wmaker-crm`.
  - `wmaker-ai-sandbox` built locally from that base image.
  - `docker run --rm wmaker-ai-sandbox ai-mcp --check` passed on container
    display `:99`.
  - Native amd64 RPMs for `wmaker-ng` and `wmaker-ai` built with `nfpm`, and
    `rpm -Uvh --test` passed for the pair.
- Layer 2 daemon default is decided: the package installs the `ng-*` binaries
  only, with no systemd/user units and no automatic start. `ng-automount`,
  `ng-power`, and `ng-notify` stay opt-in developer services until their D-Bus
  reactors are implemented and separately smoke-tested.

## Local Gates

Run from `wmaker-ng`:

```bash
make pre-commit
make pre-push
cargo test -p ai-proto -p wmng-x11 -p wmng-ewmh -p ai-mcp
DISPLAY=:9 make mcp-smoke
```

Run from `wmaker-crm`:

```bash
./autogen.sh
./configure
make
make -f infra.mk image
```

The C fork must remain pristine: infra-only changes are fine, upstream source
changes need their own documented patch-series commit.

## MCP Runtime

`ai-mcp` serves MCP over stdio and logs to stderr. It should be launched by the
agent or sandbox process that will speak MCP:

```bash
DISPLAY=:9 ai-mcp --check
DISPLAY=:9 ai-mcp
```

Delta tuning is environment-driven:

```bash
WMAKER_AI_KEYFRAME_INTERVAL_MS=10000
WMAKER_AI_MAX_DIRTY_RATIO=0.35
WMAKER_AI_MAX_DIRTY_REGIONS=16
```

Daily-driver defaults should favor bounded payloads over perfect compression.
If the XDamage feed reports too many regions, the protocol coalesces them into a
bounded dirty crop and only emits a keyframe when the coalesced dirty area is too
large.

## Acceptance Smoke

Use a disposable desktop window, such as `xclock`, on the target display:

```bash
DISPLAY=:9 make mcp-smoke
```

The smoke script launches the first available disposable X client from
`xclock`, `xterm`, or `zenity`, then speaks MCP over stdio and verifies:

1. `ai-mcp --check` reports display size, depth, bytes-per-pixel, and SHM
   availability.
2. `list_windows` includes the test window.
3. `focus` succeeds for that window.
4. `move_resize` changes geometry without crashing the WM.
5. `screenshot` returns `image/png`.
6. `changed_regions` first returns a keyframe, then returns smaller deltas after
   visible window movement.

## Measurement Loop

Before replacing the live WM, keep the existing current-WM baselines untouched.
After installing `wmaker-crm` + `wmaker-ng`/`wmaker-ai`, run the same collectors:

```bash
scripts/collect-screen-baseline.sh \
  --display :9 \
  --samples 3 \
  --s3-uri s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/screen-baselines

scripts/collect-browser-activity-baseline.sh \
  --display :9 \
  --duration 60 \
  --s3-uri s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/browser-activity-baselines
```

Compare:

- capture wall time
- capture CPU time
- PNG/base64 bytes
- estimated image-input tokens
- process RSS/CPU for `wmaker`, X/VNC, browser, and agent processes
- `changed_regions` payload size versus full-frame screenshot payload size

## Remaining Blockers

These are the concrete blockers before replacing the live desktop:

1. Re-run baseline 1 and baseline 2 after install and publish the comparison
   artifact beside the existing S3 data.
