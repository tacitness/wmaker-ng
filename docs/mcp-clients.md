# Connecting an MCP client to wmaker-ai

`ai-mcp` is a model-agnostic [Model Context Protocol](https://modelcontextprotocol.io)
server that speaks **MCP over stdio** and drives a live Window Maker / EWMH
desktop (input synthesis, window control, structured observation, screen
capture). Any MCP-capable client or agent can register it.

This page is the per-client reference. The copy-paste config fragments live in
[../integrations/](../integrations/) and are also shipped to
`/usr/share/wmaker-ai/integrations/` by the `wmaker-ai` package.

## The invocation is the only universal part

There is **no single config-file format** every client reads. What is universal
is how the server is launched:

| Lane | Command | Notes |
|------|---------|-------|
| native | `ai-mcp` | drives your real `DISPLAY`; needs `wmaker-ai` installed on PATH |
| sandbox | `docker run -i --rm wmaker-ai-sandbox` | a disposable Xvfb + Window Maker desktop in a container |

Sanity-check the native lane against your running X session first:

```bash
ai-mcp --check      # prints display / size / depth / shm and exits
```

## Generate the config (recommended — never drifts)

The binary emits its own config, so you don't hand-copy anything:

```bash
ai-mcp print-config                    # mcpServers shape, native   → .mcp.json / Cursor / Claude Desktop
ai-mcp print-config --sandbox          # mcpServers shape, sandbox (Docker)
ai-mcp print-config --client vscode    # VS Code servers shape       → .vscode/mcp.json
ai-mcp print-config --name my-desktop  # custom server name
ai-mcp print-config --absolute         # emit the resolved ai-mcp path instead of "ai-mcp"
ai-mcp print-agents-md                 # the AGENTS.md guidance block
```

## Per-client setup

### Claude Code

```bash
# CLI (simplest):
claude mcp add wmaker-desktop -- ai-mcp
# or the sandbox:
claude mcp add wmaker-sandbox -- docker run -i --rm wmaker-ai-sandbox

# or drop a project-scoped config so the repo carries it:
ai-mcp print-config > .mcp.json
```

### Claude Desktop

Merge [`integrations/mcp/mcp.json`](../integrations/mcp/mcp.json) into the
`mcpServers` object of your `claude_desktop_config.json`
(macOS: `~/Library/Application Support/Claude/`,
Linux: `~/.config/Claude/`), then restart the app.

### Cursor

```bash
mkdir -p .cursor && ai-mcp print-config > .cursor/mcp.json   # project scope
# user scope: merge into ~/.cursor/mcp.json
```

### VS Code

VS Code uses `servers` (not `mcpServers`) and a `type` field:

```bash
mkdir -p .vscode && ai-mcp print-config --client vscode > .vscode/mcp.json
```

### Windsurf / Cline / Zed / other clients

These read the `mcpServers` shape — use
[`integrations/mcp/mcp.json`](../integrations/mcp/mcp.json) verbatim or merge it
into the client's MCP settings.

## Teach the agent how to drive well (AGENTS.md)

The server-registration config tells a client *how to connect*; the AGENTS.md
block tells the agent *how to use the tools well* — climb the observation ladder
before requesting pixels, act through the highest-level tool, and respect the
command router's confirmation contract.

```bash
# Robust: idempotent marker install into a repo's AGENTS.md (create / append / refresh)
integrations/agents/install-agents-md.sh ./AGENTS.md

# Literal patch: cleanly creates AGENTS.md where none exists
git apply integrations/agents/AGENTS.wmaker-ai.md.patch

# Or just append the block
cat integrations/agents/AGENTS.wmaker-ai.md >> AGENTS.md
```

For a repo that already has an `AGENTS.md`, prefer the installer — it splices the
block between `<!-- wmaker-ai:begin -->` / `<!-- wmaker-ai:end -->` markers and
refreshes in place on re-run. The literal patch is a new-file diff, so it only
applies where `AGENTS.md` does not yet exist.

## Tool surface

Once connected, the model gets the full observe-and-act toolset:

| Group | Tools |
|-------|-------|
| Observe (no pixels) | `observe`, `desktop_scene`, `list_windows`, `accessibility_tree`, `pointer` |
| Capture (pixels) | `screenshot`, `changed_regions`, `changed_regions_fast`, `wait_for_idle` |
| Window control (EWMH) | `focus`, `move_resize`, `tile`, `minimize`, `maximize`, `close_window` |
| Input synthesis (XTEST) | `move_mouse`, `click`, `scroll`, `drag`, `type`, `key`, `key_combo` |
| Clipboard | `set_clipboard`, `get_clipboard` |
| Command router / skills | `route_command`, `list_app_skills`, `app_skill_acquisition_plan` |

See [ai-mcp-observation.md](ai-mcp-observation.md) for the observation model and
[../integrations/README.md](../integrations/README.md) for the config artifacts.
