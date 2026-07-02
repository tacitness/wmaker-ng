# Agentic Screen Observation Strategy

Date: 2026-07-02

Related files:

- [2026-07-01 AI MCP Dirty-Region Benchmark](../measurement/benchmark-2026-07-01-ai-mcp-dirty-regions.md)
- [Agentic Screen Observation Brainstorm](agentic-screen-observation-brainstorm.md)

## Thesis

For remote agentic desktop control, `wmaker-ai` should treat pixels as an
expensive fallback, not the default representation.

The July 1 benchmark proves that XDamage-backed dirty regions already collapse
screen observation payload from full-frame megabytes to crop-sized kilobytes.
The next strategic step is to add a local observation shim that converts those
dirty regions into structured, text-first state before involving a remote
high-reasoning model.

## Why This Matters

The measured local paths are already fast enough that network and model latency
become the dominant loop costs.

Representative benchmark values:

| Observation lane | Median payload | Median local latency |
| --- | ---: | ---: |
| Standard full screenshot | `1.61 MB` | `1.12s` warmed |
| PNG dirty delta | `43 KB` | `25.5ms` |
| Fast compressed delta | `124 KB` | `10ms` |

For a remote model path with `80-120ms` network round-trip and additional model
vision/reasoning latency, the `15.5ms` median difference between PNG dirty delta
and fast compressed delta is usually less important than the recurring `81 KB`
payload difference.

Over long sessions, context and token pressure compound. A model-facing
observation path should therefore optimize for bytes, semantic clarity, and
state reuse before optimizing for sub-frame local capture deltas.

## Product Direction

Build a tiered observation pipeline:

```text
XDamage events
  -> dirty rect capture
  -> local observation shim
  -> structured screen delta
  -> remote model planning
  -> local action execution
```

The local shim should maintain a rolling view of desktop state and emit the
smallest action-preserving representation for each turn.

Preferred escalation order:

1. text-only structured delta;
2. structured delta with OCR;
3. structured delta with selected crops;
4. PNG dirty-region payload;
5. full-screen keyframe.

This keeps the remote model focused on reasoning and planning while local code
handles repetitive perception and compression.

## Observation Shim Responsibilities

The shim should:

- track screen keyframes and dirty-region deltas;
- attach window identity, focus state, z-order, and geometry;
- run OCR on changed text-bearing regions;
- detect likely UI controls such as buttons, menus, input fields, tabs, and
  list rows;
- classify changes as meaningful, cosmetic, repeated, or uncertain;
- suppress noise such as cursor-only movement, blinking carets, animation, and
  repaint churn when safe;
- send pixel crops only for uncertain or visually important regions;
- force a keyframe when cumulative change or confidence thresholds require it.

The output should be deterministic JSON or another compact schema suitable for
MCP transport and model input.

## Hardware Strategy

The pipeline should adapt to local hardware:

| Tier | Local capability | Behavior |
| --- | --- | --- |
| CPU-only | no GPU or unavailable accelerator | XDamage + PNG dirty deltas + lightweight OCR where affordable |
| Integrated GPU | common modern desktop/laptop | accelerate crop preprocessing, OCR, and layout detection |
| Discrete GPU | workstation/gaming/dev rig | run a small screen-specialized perception model locally |
| Remote-only fallback | local perception unavailable | send PNG dirty deltas directly |

GPU offload is valuable only if it reduces total loop cost. The design should
measure local preprocessing latency and disable expensive stages when they lose
to direct PNG deltas.

## Small Local Model Role

A local model, if used, should be narrow and cheap:

- screen OCR/layout interpretation;
- widget and affordance detection;
- region salience scoring;
- image-to-structured-state conversion;
- confidence scoring for whether remote pixels are needed.

It should not perform broad task planning. The remote high-reasoning model
remains the planner. This keeps local requirements modest and prevents
`wmaker-ai` from becoming a heavyweight ML runtime by default.

## Near-Term Implementation Path

1. Add observation schema for structured dirty-region events.
2. Extend `ai-mcp` to expose a text-first observation tool alongside existing
   pixel tools.
3. Add a local state cache with keyframe and delta tracking.
4. Add noise filtering for cursor/caret/repaint-only changes.
5. Add optional OCR for changed regions.
6. Benchmark CPU-only preprocessing against direct PNG dirty deltas.
7. Add GPU-accelerated experiments only after the CPU baseline is measured.
8. Run a scripted browser workflow and compare full screenshots, PNG deltas,
   fast deltas, and structured deltas.

## Measurement Requirements

Future benchmarks should report:

- bytes per completed desktop action;
- model-facing bytes per observation;
- image-token or equivalent model input cost;
- local observation latency p50, p95, p99, max;
- remote loop latency p50, p95, p99, max;
- forced keyframe count;
- percentage of observations resolved without pixels;
- accuracy of model action selection;
- number of corrective observations required after each action.

The important metric is not only observation speed. It is successful desktop
actions per unit of time, context, and model cost.

## Risks

- Local OCR/layout processing can become a new bottleneck.
- Structured deltas can omit visual detail the model needed.
- Dirty-region noise can cause excessive updates if not filtered.
- GPU paths can introduce portability and packaging complexity.
- A local model can quietly violate the lightweight product rule if it becomes
  mandatory.

Mitigation: keep the shim optional, measurable, tiered, and disabled or reduced
when it loses to simpler PNG dirty-region transport.

## Strategic Recommendation

Make PNG dirty delta the default model-facing pixel lane today. Treat fast
compressed delta as the local-control-loop lane. Then build the structured
observation shim as the next product layer.

The durable advantage is not just faster screenshots. It is a desktop control
plane that knows how to spend pixels only when pixels are actually needed.
