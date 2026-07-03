# Multi-Monitor Observation Model

Issue: #40

## Goal

Treat outputs/monitors as first-class geometry in the model-facing observation
surface. Agents should not have to infer monitor layout from one giant root
framebuffer.

## Source

`wmng-x11` discovers monitor geometry through RandR `GetMonitors` when the X
server supports it. If RandR is absent, too old, or returns no monitors, the
fallback is one synthetic `root` output covering the full root window.

## MCP Contract

`desktop_scene` and `observe` include:

- `outputs[]`: monitor name, root-coordinate geometry, physical size when
  reported, primary/automatic flags, and output count;
- per-window `output`: selected output name plus output-local window geometry;
- crop policy metadata that carries the selected output with the current crop
  target.

Window-to-output assignment uses largest intersection area. If a window has no
intersection but its center is inside an output, that output is used. This keeps
straddling windows deterministic while still handling edge cases near monitor
boundaries.

## Crop Targeting

Future crop tools should accept either root coordinates or output-local
coordinates. The current observation payload gives both:

- `window.geometry` is root-relative;
- `window.output.local_geometry` is relative to the selected output.

That lets an agent request a focused crop without guessing which monitor owns
the target.

## Test Coverage

Pure geometry tests cover:

- single output;
- dual horizontal outputs;
- dual vertical outputs;
- mixed-resolution side-by-side outputs with a straddling window.

The MCP smoke also asserts that live `desktop_scene` and `observe` payloads
include output metadata and that the smoke window is associated with an output.
