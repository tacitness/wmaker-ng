```
                              wmaker-ng
                     Window Maker — Next Generation

                       <https://windowmaker.org>
                  a TacitSoft modernization initiative

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


Status
======

Bootstrapping. This commit establishes the charter and the plan. Scaffolding —
the Cargo workspace, CI/CD, git hooks, security and supply-chain gating, and
the first proof-of-concept companion — follows.


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


Building
========

Not yet. Build instructions land with the scaffolding commit.


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
