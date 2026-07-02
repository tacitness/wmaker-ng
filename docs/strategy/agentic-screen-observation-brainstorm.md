# Agentic Screen Observation Brainstorm

Date: 2026-07-02

Related measurement:
[2026-07-01 AI MCP Dirty-Region Benchmark](../measurement/benchmark-2026-07-01-ai-mcp-dirty-regions.md)

## Working Premise

The dirty-region benchmark shows that the major advantage is not only local
capture latency. The more strategically valuable result is reduced observation
payload, especially when screen state is shipped over a network to a remote,
high-reasoning model.

The key question is whether the benchmark transfers to actual agentic use:

- model endpoint is off-machine;
- network round-trip adds fixed latency;
- image tokens and context bytes create recurring cost;
- the model spends additional time interpreting visual input;
- long sessions compound every repeated screen-read penalty.

In that operating shape, the `15.5ms` median latency difference between PNG dirty
delta and fast delta looks minor compared with the `81 KB` payload difference per
observation. Context size becomes the more important resource once the capture
path is already below human-noticeable timing.

## Baseline Interpretation

The benchmark paths:

| Path | Payload | Median latency | Practical read |
| --- | ---: | ---: | --- |
| Standard full screenshot | `1.61 MB` | `1.12s` warmed | current raw baseline |
| MCP PNG dirty delta | `43 KB` | `25.5ms` | best model-facing default |
| MCP fast compressed delta | `124 KB` | `10ms` | best tight local loop path |

The raw standard method is dominated by full-frame transfer and slow screenshot
capture. Both dirty paths are far better than raw screenshots. Between the dirty
paths, PNG dirty delta saves roughly `81 KB` per observation compared with fast
delta at the cost of only `15.5ms` median local latency.

That trade strongly favors PNG dirty delta for model-facing observations.

## Transfer-Latency Context

On a real remote-model path, local capture is only one segment:

1. observe desktop state;
2. encode/transcode the changed screen state;
3. ship payload over network;
4. model receives and interprets pixels or text;
5. model reasons and selects an action;
6. action returns over network;
7. local MCP layer executes the action.

For a Starlink-like network path, round-trip latency can be roughly `80-120ms`
before model processing. A `10ms` vs `25.5ms` local capture delta is mostly lost
inside that larger loop. An extra `81 KB` per observation, however, can matter on
every turn because it increases transport bytes, model input size, storage
pressure, and possible image-token processing.

The core hypothesis: once local observation is under roughly one frame or one
human-visible beat, reducing model-facing payload should dominate shaving a few
more milliseconds from local capture.

## Local Semantic Shim Idea

The next layer should be a local observation shim that translates changed screen
regions into compact, model-legible state before a remote model is invoked.

Possible outputs:

- changed rectangles with coordinates, dimensions, z-order, and window identity;
- OCR text extracted from changed regions;
- UI element candidates such as buttons, fields, menus, tabs, and selected items;
- accessibility-derived metadata where available through AT-SPI or app-native
  APIs;
- salience markers: what changed, what likely matters, what can be ignored;
- small pixel crops only when text/structure is ambiguous;
- a rolling screen-state cache so the remote model receives deltas against a
  known local keyframe.

The shim should act as a translator, not as the primary agent. It should keep
fast local state, convert pixels to structured observations, and decide whether
the remote model needs:

- no visual payload;
- text-only structured delta;
- a few small crops;
- a forced keyframe.

## GPU Angle

The current measurement is likely close to a slow-path benchmark:

- no local GPU acceleration assumed for capture/encoding/vision;
- Brave or other screen resources are not the focus of GPU-accelerated rendering
  in the measurement;
- ImageMagick full-screen capture is especially poor as a control path;
- the tested system is not representative of a modern GPU-rich workstation.

Modern machines often have idle GPU capacity that can be used for local
preprocessing. That creates headroom for:

- accelerated image crop conversion;
- OCR or layout detection;
- tiny local vision models;
- region classification;
- image-to-structured-state conversion;
- confidence scoring before sending anything to the remote model.

The important caveat is that GPU offload must not become the bottleneck. If no
local GPU exists, or if the model is too large, the shim can cost more than it
saves. The design needs adaptive tiers rather than assuming high-end local
hardware.

## Local Small Model Possibility

A small, specialized local model trained for screen processing could sit close
to the graphical display and do the cheap perception work:

- identify widgets and text in dirty regions;
- classify changes as meaningful or cosmetic;
- summarize the local screen delta in a stable schema;
- detect when a remote high-reasoning model actually needs pixels;
- reduce repeated image-token tax over long sessions.

This model does not need broad reasoning. Its job is narrow perception and
compression. The remote endpoint remains the planner and high-reasoning agent.

## Design Principle

Use the smallest representation that preserves actionability.

Preferred order:

1. structured textual delta;
2. structured delta plus OCR;
3. structured delta plus selected crops;
4. PNG dirty delta;
5. full keyframe.

Fast compressed delta remains valuable for local control loops, but the default
remote-model lane should optimize context and token pressure first.

## Questions To Test

- How often does a real agentic workflow need pixels after OCR and structured
  deltas?
- What percentage of dirty regions are semantically irrelevant animation,
  cursor movement, blinking carets, or repaint noise?
- What is the break-even point where local OCR/layout processing saves more
  remote latency and token cost than it adds locally?
- Can the shim maintain a reliable rolling screen-state cache across multiple
  actions?
- How often are forced keyframes needed?
- What is the p95/p99 tail latency once GPU or small-model preprocessing is
  added?
- Does the remote model perform better with concise structured deltas than with
  repeated pixel crops?

## Raw Conclusion

The benchmark likely transfers, but not as a simple "faster screenshot" story.
The real product direction is an agentic observation pipeline:

- XDamage finds what changed;
- local code captures only that area;
- local shim converts it into the cheapest useful representation;
- remote model receives text-first state with pixels only when needed;
- high-reasoning inference is reserved for planning and judgment, not raw screen
  parsing.

Context save is the compounding win.
