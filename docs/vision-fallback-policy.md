# Vision Fallback Policy

Issue: #38

## Default

The default model-facing lane is structured observation:

```text
observe -> desktop/window objects -> semantic adapters -> action
```

`observe` must not embed screenshots or crops. It returns enough metadata for a
caller to decide whether pixels are required before spending context on visual
payloads.

## Escalation Order

Use this order for agent observation:

1. `observe`
2. `desktop_scene`
3. `accessibility_tree`
4. browser semantic adapter data when connected
5. focused-window crop when crop tooling is available
6. `changed_regions`
7. `screenshot`

Full screenshots are the last resort. Dirty PNG deltas are preferred over full
screenshots when the model needs pixels, because the benchmarked payload is much
smaller than a full-frame observation.

## Pixel Triggers

Request pixels only when at least one of these is true:

- the target control or content is visually ambiguous after structured
  observation;
- the agent must inspect canvas, image, video, or another non-semantic surface;
- recent damage overlaps the intended action target and semantic state may be
  stale;
- an action failed and visual confirmation is needed for recovery;
- accessibility or browser semantic adapters are unavailable or report low
  coverage.

## Crop Target

When pixels are needed, target the smallest useful region:

1. focused window;
2. pointer window;
3. best actionable window;
4. dirty region from local framebuffer cache;
5. full screenshot only if no narrower target is available.

`observe.vision_fallback_policy.crop_target` reports the current best target.
The current implementation reports window geometry. Future crop tools should use
that metadata to render a focused crop without requiring the model to inspect the
whole root framebuffer.

## Benchmark Relationship

The 2026-07-01 dirty-region benchmark showed:

| Path | Median payload | Median latency | Intended use |
| --- | ---: | ---: | --- |
| `observe` structured JSON | small, state-dependent | local object query | default model lane |
| focused crop | future framebuffer tool | target-dependent | first pixel fallback |
| `changed_regions` PNG delta | `43 KB` | `25.5ms` | model-facing pixel fallback |
| `changed_regions_fast` | `124 KB` | `10ms` | local low-latency control loop |
| full screenshot | `1.6 MB+` | `1s+` warmed baseline | last resort |

The policy favors PNG dirty deltas over fast deltas for model context because
context bytes compound across long sessions. Fast deltas remain useful for local
control loops where the payload is consumed near the display.

## MCP Contract

`observe` exposes:

- `vision_fallback_policy.pixels_required`: `false` by default;
- `vision_fallback_policy.recommended_next`: normally
  `act_from_structured_state`;
- `vision_fallback_policy.crop_target`: focused/pointer/actionable window
  geometry when available;
- `vision_fallback_policy.escalation_order`: the ordered fallback ladder;
- `vision_fallback_policy.request_pixels_when`: the explicit trigger list.

This lets remote models avoid asking for pixels reflexively while still having a
clear path to request visual evidence when structured state is insufficient.
