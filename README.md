```
                              wmaker-ng
                     Window Maker — Next Generation

                       <https://tacitsoft.dev>
                  a TacitSoft modernization initiative
              honoring upstream: <https://windowmaker.org>

                                  by

                            The wmaker-ng Team

         "Speed is a feature. We intend to add the next thirty
          years of the Linux desktop to Window Maker without
          spending a single millisecond of its famous lightness."
                              -- the design rule
```

Description
===========

**wmaker-ng** brings the GNU Window Maker into the modern Linux desktop —
automatic media handling, power and session awareness, native notifications,
and a first-class AI control plane — **without surrendering the speed, the
small footprint, or the NeXTSTEP soul** that made Window Maker worth keeping.

It does this by refusing to touch what already works. The Window Maker core
stays a lean C window manager, tracked pristine against its upstream so it
remains trivially rebasable. Everything new lives *beside* it as small,
single-purpose binaries that speak the standard desktop protocols — D-Bus,
EWMH, XDamage, XTEST — and the Model Context Protocol (MCP). Nothing modern is
welded into the hot path.

The result is a ladder, not a fork you can never escape:

    wmaker      the core window manager (C, upstream parity)
    wmaker-ng   core + modern Linux integration  (no AI, no bloat)
    wmaker-ai   wmaker-ng + the MCP AI control plane


Philosophy
==========

Window Maker has shipped modular companions — dockapps — since day one. We are
not changing that methodology; we are extending it.

  * **The core stays sacred.** All upstream changes remain pull-and-rebase
    clean. New behavior arrives as new binaries, never as core edits, except
    for a small, documented, upstream-bound patch series.

  * **Amenities are processes, not features.** Auto-mounting, power, and
    notifications are event-driven daemons that talk D-Bus to the system
    (udisks2 / logind / upower) and EWMH to the window manager. The system
    already does the privileged work; we only react to it.

  * **AI is a client, not a kernel.** The MCP layer lets *any* model drive the
    desktop the way a human would — move the pointer, click menus, read the
    screen — over a documented protocol. The window manager never learns it is
    being driven.

  * **Lightweight is the product.** If a feature cannot be added without taxing
    the idle desktop, it ships disabled, out-of-process, or not at all.


Architecture
============

    +---------------------------------------------------------------+
    |  Layer 3 — wmaker-ai   (Rust)                                  |
    |  MCP server · screen capture+diff protocol · input synthesis  |
    |  model-agnostic — any agent connects over MCP                  |
    +---------------------------------------------------------------+
    |  Layer 2 — wmaker-ng   (Rust)                                  |
    |  automount(udisks2) · power(logind/upower) · notify · dockapps |
    |  D-Bus to the system, EWMH to the WM — zero core changes       |
    +---------------------------------------------------------------+
    |  Layer 1 — Window Maker core   (C, upstream, kept pristine)    |
    |  rebased on repo.or.cz · only tiny hooked seams + tiling       |
    +---------------------------------------------------------------+

The layers are decoupled at **runtime** (D-Bus, EWMH, MCP) — they do not
compile-link against each other. That is what keeps the C core pristine and the
Rust companions independently shippable.

Languages, decided forensically for **performance and safety**:

  * **C** — the core. It *is* Window Maker; native, rebasable, upstreamable.
  * **Rust** — all new systems code (ng + ai). C-class speed with no garbage
    collector, plus compile-time memory and thread safety on a daemon that is
    network-facing and synthesizes input. Native `x11rb` (XDamage/XTEST/XShm),
    `zbus` (D-Bus), `rmcp`/`tokio`/`serde` (MCP).
  * **Python** — quarantined to `ml/` for Phase-4 model work only (uv + ruff).

See [PLAN.md](PLAN.md) for the full mapping, repository topology, packaging,
and the one-month roadmap.

Daily-driver acceptance gates and measurement loops live in
[docs/daily-driving-readiness.md](docs/daily-driving-readiness.md).


Status
======

**v0.1.0 released.** Signed deb / rpm / apk packages and static tarballs for
amd64 + arm64, built and GPG-signed by an OIDC-gated release pipeline. The
`ng` daemons (automount / power / notify) and the `ai-mcp` control plane are
functional; object-first observation and the packaging matrix are the active
workstreams.

Releases: <https://github.com/tacitness/wmaker-ng/releases>


Repository topology
===================

    repo.or.cz/wmaker-crm  --upstream-->  tacitness/wmaker-crm      (C core, rebasable)
                                                 |  runtime deps only
                                                 v  (D-Bus / EWMH / MCP)
                                       tacitness/wmaker-ng       (Rust: ng + ai)
                                                 |  packaged via nfpm
                                                 v
              packages:  wmaker · wmaker-ng · wmaker-ai  -->  subscribable repo

The pristine C fork lives in its own repository so that `git rebase
upstream/master` only ever churns upstream history. This repository holds the
Rust companion workspace and the Python ML tooling.


Quickstart — drive a desktop with your model
============================================

`ai-mcp` speaks standard EWMH / XTEST / XDamage, so it drives **any**
EWMH-compliant window manager — stock Window Maker straight from your distro
included. The `tacitness/wmaker-crm` fork is *not* required at runtime; it
exists to carry upstream-bound core patches (tiling seams, maximize behavior).

### 1. Native — your real X session

Grab the package for your distro from the
[latest release](https://github.com/tacitness/wmaker-ng/releases/latest):

```bash
# Debian/Ubuntu               # EL9+/Fedora                # Alpine
sudo apt install ./wmaker-ng_*_amd64.deb ./wmaker-ai_*_amd64.deb
sudo dnf install ./wmaker-{ng,ai}-*.x86_64.rpm
sudo apk add --allow-untrusted ./wmaker-{ng,ai}_*_x86_64.apk

# sanity check against your running X session
ai-mcp --check
```

Then register it with any MCP client. Claude Code, for example:

```bash
claude mcp add wmaker-desktop -- ai-mcp
```

For other clients (Claude Desktop, Cursor, VS Code, Windsurf, Zed …), `ai-mcp`
emits its own config — no hand-copying:

```bash
ai-mcp print-config                 # mcpServers shape → .mcp.json / Cursor / Claude Desktop
ai-mcp print-config --client vscode > .vscode/mcp.json
```

Drop-in config fragments live in [integrations/](integrations/), and the full
per-client walkthrough + tool surface is in
[docs/mcp-clients.md](docs/mcp-clients.md). There is also an appendable
`AGENTS.md` block (`ai-mcp print-agents-md`) that teaches an agent how to drive
the desktop well.

The model gets `list_windows` / `focus` / `move_resize` / `tile` /
`move_mouse` / `click` / `type` / `key` / `screenshot` / `desktop_scene` —
full observe-and-act on the live desktop. The window manager never learns it
is being driven.

Command fixtures for the voice/app-skill lane can be tested without audio or
X:

```bash
ai-mcp route-command --dry-run --text "open browser to example dot com"
ai-mcp route-command --dry-run --text "make a cylinder in Blender and render it"
```

### 2. Sandboxed — a disposable desktop in Docker

The sandbox image bundles Xvfb + Window Maker + `ai-mcp` into one
MCP-over-stdio container — a scriptable desktop your agent can own:

```bash
make sandbox-image        # needs the wmaker-crm:headless base (see sandbox/)
docker run -i --rm wmaker-ai-sandbox            # MCP on stdin/stdout

# as an MCP server in Claude Code:
claude mcp add wmaker-sandbox -- docker run -i --rm wmaker-ai-sandbox
```

Public registry images (`public.ecr.aws/y6d2s4r6/wmaker-ai-sandbox`) are
landing shortly — after that the quickstart is the `docker run` line alone,
no local build.

See [sandbox/README.md](sandbox/README.md) for the browser-enabled variant.


Building from source
====================

```bash
make ci-local      # fmt + clippy + tests + audit — full CI parity
make build         # release binaries into dist/
make packages      # deb/rpm/apk via nfpm (needs staged cross-builds)
```


License
=======

Provisional: **GPL-2.0-or-later**, honoring Window Maker's lineage. Final
license selection is tracked in [PLAN.md](PLAN.md) and may differ per layer for
the out-of-process companions.


Acknowledgements
================

Window Maker is the GNU window manager for the X Window System, created by
Alfredo K. Kojima and Dan Pascu, maintained today as the Crossover Maintenance
Release at <git://repo.or.cz/wmaker-crm.git>. wmaker-ng stands on their work and
intends to give back — every core improvement is meant to flow upstream.
