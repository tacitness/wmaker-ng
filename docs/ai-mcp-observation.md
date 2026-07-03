# wmaker-ai MCP observation lanes

`observe` is the default model-facing observation tool. It returns compact
text/JSON desktop state without embedding screenshot pixels:

- focused window summary
- visible actionable window handles
- geometry, workspace, app identity, and available window actions
- recent XDamage rectangle metadata when available
- references to opt-in pixel tools

Pixel tools remain available, but callers should request them only when the
text/object state is insufficient:

- `screenshot` returns a full PNG image.
- `changed_regions` returns PNG dirty-region deltas.
- `changed_regions_fast` returns raw low-latency dirty-region data for local
  control loops.

The intended agent loop is:

1. Call `observe`.
2. Choose a target window/action from the returned handles.
3. Use control tools such as `focus`, `move_resize`, `key`, `type`, `click`, or
   `scroll`.
4. Request pixel fallbacks only for visual ambiguity, crop inspection, or
   debugging.

