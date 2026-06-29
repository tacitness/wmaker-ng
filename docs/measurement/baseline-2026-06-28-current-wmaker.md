# 2026-06-28 Current Window Maker Baseline

This is the first control dataset for comparing the existing OpenClaw/Codex
desktop-driving path against future `wmaker-ng` + `wmaker-ai` tooling.

## Environment

- Host: `tacitbot`
- Window manager: `WindowMaker 0.96.0`
- Current binary: `/usr/local/bin/wmaker.real`
- Package ownership: local install, not owned by RPM
- `wmaker-crm` checkout: `/data/src/tacitsoft/infrastructure/wmaker-crm`
  at `0d294268`
- `wmaker-ng` checkout: `/data/src/tacitsoft/infrastructure/wmaker-ng`
  at `776cda8` plus local measurement docs/script
- Display server: X11
- Physical/session display: `:0`
- Remote/VNC display: `:9`
- Resolution: `1920x1080`
- Root depth: `24`
- Capture command: `ImageMagick import -window root`
- Raw screenshots stored: `false`

## S3 Dataset

```text
s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/screen-baselines/20260628T130808Z-tacitbot-0/
s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/screen-baselines/20260628T130819Z-tacitbot-9/
```

Each prefix contains:

- `summary.json`
- `captures.jsonl`
- `processes.json`
- `display.txt`
- `report.md`

## Display `:0` Baseline

Three full-screen samples:

| Metric | Average |
| --- | ---: |
| PNG payload | `45,113.67` bytes |
| Base64 payload | `60,153.33` bytes |
| Raw RGBA equivalent | `8,294,400` bytes |
| Capture wall time | `0.5467` seconds |
| Capture CPU time | `0.3000` seconds |
| Legacy vision high-token estimate | `1,105` tokens |
| OpenAI image-input high-fidelity estimate | `6,563` tokens |

Idle process snapshot highlights:

| Process | RSS |
| --- | ---: |
| `wmaker.real` wrappers/workers | about `1-6 MiB` each |
| `Xorg` | about `26 MiB` |
| `Xvnc` | about `46 MiB` |

## Display `:9` Baseline

Three full-screen samples:

| Metric | Average |
| --- | ---: |
| PNG payload | `246,533` bytes |
| Base64 payload | `328,712` bytes |
| Raw RGBA equivalent | `8,294,400` bytes |
| Capture wall time | `0.7933` seconds |
| Capture CPU time | `0.4467` seconds |
| Legacy vision high-token estimate | `1,105` tokens |
| OpenAI image-input high-fidelity estimate | `6,563` tokens |

## Interpretation

The current full-frame path is already byte-light when the desktop is visually
simple, but it still pays external screenshot process overhead and full-frame
image-token accounting. At `1920x1080`, the uncompressed frame is always about
`8.29 MiB`; compression varies with scene content, while token estimates remain
resolution/tile driven.

The first target for `wmaker-ai` is therefore not only smaller PNGs. The better
target is fewer full-frame observations:

- use direct XShm capture instead of shelling out to screenshot tools;
- use XDamage dirty rectangles after the first keyframe;
- send structured deltas and region crops;
- report bytes, capture CPU, latency, and token estimates per successful
  desktop action.

This baseline is intentionally conservative: no raw screenshots were retained,
so later comparisons can be shared and evaluated without exposing desktop
content.
