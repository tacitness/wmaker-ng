# AT-SPI Semantic Observation Spike

Issue: #37

## Goal

Evaluate AT-SPI as a semantic input source for object-first agent observation.
The default model-facing lane should stay text/object-first and should not embed
screenshots. AT-SPI can enrich that lane when applications expose useful
accessibility roles, labels, bounds, and tree structure.

## Implementation

`ai-mcp` exposes an explicit spike tool:

```text
accessibility_tree(max_depth?, max_children_per_node?)
```

The tool:

- resolves the private AT-SPI bus through `org.a11y.Bus`;
- samples from the AT-SPI desktop root;
- returns a bounded tree of semantic nodes;
- includes role/name/description, child count, and screen extents when exposed;
- reports per-node and top-level errors without embedding pixels;
- caps traversal depth and child fan-out to avoid context blowups.

AT-SPI remains separate from `observe` for now. That keeps default observation
stable while we measure which apps expose useful semantic data.

## Live Finding

On the local Tacitbot session, `org.a11y.Bus` is available and returns a private
AT-SPI bus address. The registry root itself may disconnect without replying,
so the sampler also probes a capped number of unique app names on the private
AT-SPI bus with short per-node timeouts.

In the live `:9` Window Maker session, the spike found real Nautilus nodes:

- `node_count=19`
- root app node named `nautilus`
- child nodes such as `out` and `Home`
- screen extents for several nodes
- a small number of timed-out or missing-property errors from non-responsive
  providers

This proves the integration path and shows the operational constraint: AT-SPI
providers can be partial or slow. The MCP tool must stay bounded and should
surface errors as data rather than blocking the whole observation loop.

## Product Position

AT-SPI belongs in the object-first stack as a supplemental semantic adapter:

- use it for native desktop apps that expose roles, names, and bounds;
- keep EWMH/X11 scene state as the durable window/object backbone;
- keep browser DOM/ARIA adapters as the primary high-value semantic lane for web
  workflows;
- use pixel crops only when structured state is missing or ambiguous.

## Close Gate Status

- A spike MCP tool can request an accessibility tree from a live session.
- The payload is bounded and contains semantic nodes when the app/session
  exposes them.
- Sparse or unavailable AT-SPI data is reported as structured errors rather than
  failing the whole observation plane.

AT-SPI should remain in M5 as an enrichment path, not a hard dependency for
v1.0.0 object-first observation.
