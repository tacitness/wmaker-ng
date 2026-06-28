# wmaker-ai sandbox (#18)

A portable, scriptable Window Maker desktop any MCP agent can drive: `Xvfb` +
Window Maker + `ai-mcp` in one OCI image.

## Build

```bash
make sandbox-image     # builds ai-mcp (release), copies it in, docker build
```

Requires the base image `wmaker-crm:headless` in the local Docker store (built
from `tacitness/wmaker-crm` via `make -f infra.mk image`). Public/registry
distribution is tracked in dagobah-infra#126.

## Run

```bash
# Drive it as an MCP client would (stdio):
docker run -i --rm wmaker-ai-sandbox          # then speak MCP on stdin/stdout

# Demo with a window to click (xclock):
docker run -i --rm wmaker-ai-sandbox sh -c 'xclock & sleep 1; exec ai-mcp'
```

The base entrypoint brings up Xvfb (`DISPLAY=:99`) + wmaker, then execs the
command into the live desktop. `ai-mcp` exposes `move_mouse`/`click`/`type`/
`key`/`list_windows`/`focus`/`move_resize`/`tile`/`screenshot` — the same tools
proven by hand in #16.

For an automated host-side runtime smoke, use:

```bash
DISPLAY=:9 make mcp-smoke
```

The smoke client speaks MCP over stdio, launches a disposable X client, moves it
through EWMH, and verifies both full screenshot and XDamage delta observations.
