<!-- wmaker-ai:begin -->
## Driving the desktop with wmaker-ai (MCP)

This repository can be driven by an AI agent through the **wmaker-ai** MCP
server (`ai-mcp`), a model-agnostic computer-use control plane for a live
Window Maker / EWMH desktop. Register it with your MCP client using the
config in `integrations/mcp/` (or run `ai-mcp print-config`).

**Observe before you act — prefer structure over pixels.** The server exposes a
cheap, text-first observation ladder; climb it in order and only request pixels
when structure is insufficient:

1. `observe` — the preferred model-facing lane: compact JSON of focus,
   actionable windows, damage metadata, and the vision-fallback policy. No
   screenshot pixels.
2. `desktop_scene` — full scene graph (windows, stacking, pointer, per-window
   capabilities) when you need more than `observe` gives.
3. `accessibility_tree` — bounded AT-SPI semantic tree when an app exposes one.
4. Browser semantic adapter — richer control data when `observe` reports a
   connected browser adapter.
5. Pixels, last — `screenshot` (full PNG), or `changed_regions` /
   `changed_regions_fast` for XDamage dirty-region deltas. Follow the
   `vision_fallback_policy` in `observe`: crop to the focused window before
   asking for full-screen pixels.

**Act through the highest-level tool available.** Window control
(`focus`, `move_resize`, `tile`, `minimize`, `maximize`, `close_window`) goes
through EWMH; input synthesis (`move_mouse`, `click`, `scroll`, `drag`, `type`,
`key`, `key_combo`) through XTEST. Reach for raw input only when no
window-control or semantic path exists.

**Respect the command router's safety contract.** `route_command` returns a
`safety` block; when `confirmation_required` is true, do **not** re-call with
`confirmed: true` until a human has approved. Use `--dry-run` / `dry_run: true`
to plan without executing. `list_app_skills` and `app_skill_acquisition_plan`
describe known apps; acquisition plans are never auto-trusted.

**Settle before observing.** After an action that changes the screen, call
`wait_for_idle` (a quiet period with a timeout) before the next `observe` so you
read a settled frame, not a mid-repaint one.

The window manager never learns it is being driven — keep actions to what a
human could do at the same desktop.
<!-- wmaker-ai:end -->
