# Screen-Driving Baseline

This document defines the measurement lane for comparing today's desktop-driving
stack against `wmaker-ng` + `wmaker-ai`.

The goal is whitepaper-grade accounting: measure bytes, CPU, latency, memory,
and image-token estimates before optimizing. Keep raw observables separate from
provider-specific token estimates so the dataset remains useful when model
pricing or quota policy changes.

## Baseline Questions

1. How many bytes does one screen observation require today?
2. How much CPU and wall time does a screenshot capture cost?
3. How much resident memory is held by the window manager, X server, browser,
   and agent stack while the desktop is being driven?
4. How many estimated image tokens does a full-screen observation imply?
5. How much should XDamage/XShm deltas reduce bytes and estimated image tokens?

## Collector

Run the static screen baseline from the repository root:

```bash
scripts/collect-screen-baseline.sh \
  --display :0 \
  --samples 3 \
  --s3-uri s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/screen-baselines
```

The collector writes a local run directory under:

```text
out/screen-baselines/<UTC>-<host>-<display>/
```

and uploads the same files to:

```text
s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/screen-baselines/<UTC>-<host>-<display>/
```

Run the replayable browser activity baseline from the repository root:

```bash
scripts/collect-browser-activity-baseline.sh \
  --display :9 \
  --duration 60 \
  --s3-uri s3://tacitsoft-agent-artifacts-494111853453-us-west-2/wmaker-ng/browser-activity-baselines
```

That scenario activates the existing Brave/Facebook window, navigates to
Facebook home, observes the top feed, scrolls three pages with observations,
searches for `CV-25-0070-PR`, observes the search results, and samples process
resources at 1 Hz. It does not store raw screenshots, feed post text, or
private messages.

## Artifact Set

- `summary.json`: run metadata, aggregate measurements, and process snapshot.
- `captures.jsonl`: one row per screenshot sample.
- `processes.json`: process-level baseline for `wmaker.real`, X, browser, node,
  and Codex/OpenClaw processes when visible.
- `display.txt`: `xdpyinfo` output for the measured display.
- `report.md`: human-readable run summary.

Raw screenshots are intentionally not stored. The collector captures to a temp
file, records dimensions, byte counts, timing, and a SHA-256 hash, then deletes
the image before upload.

Browser activity baselines also include:

- `actions.jsonl`: one row per scripted browser action.
- `process-samples.jsonl`: 1 Hz process samples during the timed scenario.
- `windows.txt`: visible window list at run start.

## Metrics

Measured fields:

- `png_bytes`: compressed screenshot bytes.
- `base64_bytes`: estimated payload if the PNG is sent through a JSON/base64
  tool result.
- `raw_rgba_bytes`: width * height * 4, useful for comparing raw XShm frame
  movement.
- `capture_elapsed_sec`: wall-clock capture time.
- `capture_user_sec` and `capture_sys_sec`: capture process CPU seconds.
- `capture_maxrss_kib`: peak RSS for the capture command.
- `process_snapshot[].rss_kib`: resident memory for relevant desktop/agent
  processes.

Estimator fields:

- `legacy_vision_high_tokens`: 85 base tokens plus 170 tokens per 512px tile
  after the classic high-detail resize model.
- `openai_image_input_high_fidelity_tokens`: 65 base tokens plus 129 tokens per
  tile plus high-fidelity image-input overhead, based on the OpenAI Images and
  Vision guide as checked on 2026-06-28.

OpenAI's public docs describe image inputs as token-metered and document
base/tile accounting for image fidelity. Keep the estimator versioned because
model families do not all use identical accounting:

- <https://developers.openai.com/api/docs/guides/images-vision>
- <https://openai.com/api/pricing/>

## Current Baseline Scope

The first baseline intentionally measures the current non-`wmaker-ai` path:

- Window manager: `WindowMaker 0.96.0`
- Display: X11 root capture
- Capture command: `ImageMagick import -window root`
- Screen transport: full-screen PNG-equivalent observation
- Driver stack: OpenClaw/Codex process tree as visible from `ps`

This is not yet measuring MCP delta capture. It is the control case.

## Expected wmaker-ng Savings

`wmaker-ai` should be measured against this baseline with these target deltas:

1. Full-frame baseline capture should move from external screenshot process
   overhead to direct XShm capture inside `ai-mcp`.
2. Repeated observations should prefer XDamage dirty rectangles over full
   screenshots.
3. Tool payload should become a structured update:

```json
{
  "kind": "delta",
  "dirty_area": 12345,
  "regions": [
    {"x": 10, "y": 10, "width": 320, "height": 90, "png_base64": "..."}
  ]
}
```

4. Estimated image-token cost should be calculated per region and compared with
   the full-frame estimate.
5. CPU should be sampled during real driving loops, not only idle capture.

## Comparison Formula

For each future run:

```text
byte_savings_ratio = 1 - (delta_base64_bytes / full_frame_base64_bytes)
token_savings_ratio = 1 - (delta_estimated_tokens / full_frame_estimated_tokens)
capture_cpu_savings_ratio = 1 - (delta_capture_cpu_sec / full_frame_capture_cpu_sec)
capture_latency_savings_ratio = 1 - (delta_elapsed_sec / full_frame_elapsed_sec)
```

Report all ratios with the raw numerator and denominator. A savings percentage
without the raw values is not acceptable for this lane.

## Next Instrumentation

- Add an `ai-mcp` benchmark command or MCP tool that emits the same schema for
  XShm full frames and XDamage deltas.
- Add a real driving scenario: focus terminal, open root menu, launch a small
  app, move/resize a window, close it.
- Record action count and observation count so the metric can become bytes per
  successful desktop action, not only bytes per screenshot.
- Keep S3 as the durable dataset store; do not commit run artifacts.
