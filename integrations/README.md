# integrations/ — MCP client config & agent guidance

Drop-in configuration for pointing any MCP client or coding agent at the
**wmaker-ai** control plane (`ai-mcp`). These are the same artifacts that ship
to `/usr/share/wmaker-ai/integrations/` in the packaged `wmaker-ai`.

> These are MCP **client** configs — how a model *connects to and drives* the
> desktop. They are unrelated to `../skills/*.json`, which are app-skill
> manifests consumed by the command router inside `ai-mcp`.

## The one thing that is universal: the invocation

There is no single config-file format every client reads. What *is* universal is
how the server is launched:

- **native** — `ai-mcp` on `PATH` (drives your real `DISPLAY`)
- **sandbox** — `docker run -i --rm wmaker-ai-sandbox` (a disposable Xvfb desktop)

`ai-mcp` speaks MCP over stdio. Everything below just wraps that invocation in
each client's preferred JSON shape.

## Files

| File | Shape | For |
|------|-------|-----|
| `mcp/mcp.json` | `{"mcpServers": …}` | Claude Code (`.mcp.json`), Claude Desktop, Cursor (`.cursor/mcp.json`), Windsurf, Cline, Zed |
| `mcp/mcp.sandbox.json` | `{"mcpServers": …}` | same clients, but the Docker sandbox server |
| `mcp/vscode-mcp.json` | `{"servers": …}` | VS Code (`.vscode/mcp.json`) — VS Code uses `servers`, not `mcpServers` |
| `agents/AGENTS.wmaker-ai.md` | Markdown block | append to a repo's `AGENTS.md` / `CLAUDE.md` — how to *drive well* |
| `agents/install-agents-md.sh` | script | idempotent marker-based install of the block above |
| `agents/AGENTS.wmaker-ai.md.patch` | unified diff | literal `git apply` patch that creates `AGENTS.md` from the block |

## Generate instead of copy (drift-proof)

The committed files are exactly what the binary emits, so you can regenerate
them for any client — with the right server name or an absolute command path:

```bash
ai-mcp print-config                         # mcpServers shape, native   → .mcp.json
ai-mcp print-config --sandbox               # mcpServers shape, sandbox
ai-mcp print-config --client vscode         # VS Code servers shape      → .vscode/mcp.json
ai-mcp print-config --name my-desktop       # custom server name
ai-mcp print-config --absolute              # emit the resolved ai-mcp path, not "ai-mcp"
ai-mcp print-agents-md                       # the AGENTS.md guidance block
```

## AGENTS.md guidance — two ways to apply it

```bash
# Robust: idempotent marker install (creates, appends, or refreshes in place)
integrations/agents/install-agents-md.sh ./AGENTS.md

# Literal patch: cleanly creates AGENTS.md in a repo that has none
git apply integrations/agents/AGENTS.wmaker-ai.md.patch
```

The patch is a new-file diff, so it applies cleanly wherever `AGENTS.md` does
not yet exist. For a repo that already has an `AGENTS.md`, use the installer (it
splices between the `wmaker-ai:begin/end` markers) or just append the block.

### Regenerating the patch

The patch is derived from `agents/AGENTS.wmaker-ai.md`; a parity test in
`crates/ai-mcp` fails if they drift. After editing the snippet, refresh it:

```bash
work="$(mktemp -d)"; cp integrations/agents/AGENTS.wmaker-ai.md "$work/AGENTS.md"
( cd "$work" && git diff --no-index -- /dev/null AGENTS.md ) \
  > integrations/agents/AGENTS.wmaker-ai.md.patch || true
rm -rf "$work"
```

See [../docs/mcp-clients.md](../docs/mcp-clients.md) for per-client walkthroughs
and the full tool surface.
