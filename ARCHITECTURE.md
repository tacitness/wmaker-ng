# Architecture

> Expands [PLAN.md](PLAN.md) §5 with the concrete shape of this Rust workspace.
> The charter is in [README.md](README.md); the full engineering map (topology,
> packaging, roadmap, open decisions) remains in PLAN.md. This document is the
> source of truth for *how the crates fit together*.

## 1. The three-layer ladder

```
+---------------------------------------------------------------+
|  Layer 3 — wmaker-ai   (Rust, this repo: ai-*)                |
|  MCP server · screen capture+diff protocol · input synthesis  |
|  model-agnostic — any agent connects over MCP                  |
+---------------------------------------------------------------+
|  Layer 2 — wmaker-ng   (Rust, this repo: ng-*)                |
|  automount(udisks2) · power(logind/upower) · notify · dockapps |
|  D-Bus to the system, EWMH to the WM — zero core changes       |
+---------------------------------------------------------------+
|  Layer 1 — Window Maker core   (C, tacitness/wmaker, pristine)|
|  rebased on repo.or.cz · only tiny hooked seams + tiling       |
+---------------------------------------------------------------+
```

The layers are decoupled at **runtime** (D-Bus, EWMH, MCP). They do **not**
compile-link against each other — that is what keeps the C core rebasable and
the Rust companions independently shippable. Dependency direction is strictly
**ai → ng → shared → (runtime) core**.

## 2. Crate topology

```
crates/
├─ wmng-x11    (lib)   shared  XDamage · XTEST · XShm · XFixes        [x11rb]
├─ wmng-dbus   (lib)   shared  udisks2 · logind · upower async client [zbus, tokio]
├─ wmng-ewmh   (lib)   shared  _NET_* window-control client           [x11rb → wmng-x11]
├─ ng-automount(bin)   ng      udisks2 reactor + dockapp              [wmng-dbus, wmng-ewmh]
├─ ng-power    (bin)   ng      logind/upower session daemon           [wmng-dbus, wmng-ewmh]
├─ ng-notify   (bin)   ng      org.freedesktop.Notifications server   [wmng-dbus, wmng-ewmh]
├─ ai-mcp      (bin)   ai      MCP server (the heart)                 [rmcp, wmng-x11, wmng-ewmh, ai-proto]
└─ ai-proto    (lib)   ai      screen-diff protocol + codec           [serde, x11rb → wmng-x11]
```

Three shared libraries form the seam; five binaries/libraries build on top.
`wmng-*` carry no `main`; the daemons are the only entry points.

## 3. Layer 1 — Core (C, `tacitness/wmaker`)

Not in this repository. Tracked pristine against `repo.or.cz/wmaker-crm` with a
small, documented, upstream-bound patch series. First core feature: **tiling
mode** + maximize/organization, behind a config flag, intended for upstream via
`git send-email` and deletion from our series once accepted. The few C seams are
mirrored here under `core-patches/` for reference only (canonical copy lives in
the C repo — PLAN §9).

## 4. Layer 2 — wmaker-ng amenities (Rust)

Event-driven, idle-until-poked daemons. **D-Bus lives only here** — never in the
WM event loop. Each daemon subscribes to a system service via `wmng-dbus` and
surfaces state to the window manager via `wmng-ewmh`; the WM never learns it is
being driven.

| Daemon         | System interface (D-Bus)          | Surfaces as              |
|----------------|-----------------------------------|--------------------------|
| `ng-automount` | `org.freedesktop.UDisks2`         | dockapp + auto-mount     |
| `ng-power`     | `org.freedesktop.login1` / UPower | suspend/idle/lid/battery |
| `ng-notify`    | `org.freedesktop.Notifications`   | native notifications     |

UDisks2/logind/upower already do the privileged work; these are thin reactors.
The dockapp-vs-tray surface for `ng-automount` is an open decision (PLAN §9).

## 5. Layer 3 — wmaker-ai control plane (Rust)

`ai-mcp` is an MCP server exposing computer-use-style tools over existing X11
extensions. It is a **broker + capture engine, not an ML runtime** — the model
lives elsewhere (API, or local llama.cpp/ollama) and connects over MCP.

| Tool group       | Tools                                           | X11 mechanism      |
|------------------|-------------------------------------------------|--------------------|
| input synthesis  | `move_mouse`, `click`, `type`, `key`            | XTEST              |
| window control   | `list_windows`, `focus`, `move`, `resize`, `tile`| EWMH (`wmng-ewmh`)|
| screen capture   | `screenshot`                                    | XShm               |
| change feed      | `get_changed_regions`                           | XDamage (`ai-proto`)|

MCP transport (stdio vs. socket) is an open decision (PLAN §9); the scaffold
declares `rmcp` with the `transport-io` (stdio) feature as the near-term default.

## 6. The screen-diff protocol (`ai-proto`)

The genuinely novel piece, grounded in X primitives rather than reinvented.
**XDamage** yields exact dirty rectangles per frame (change detection is free);
**XShm** gives fast shared-memory pixel access. The invention is the *encoding*
that makes deltas cheap for a model — a video-codec analogy:

- **Keyframe (I-frame):** full screen capture/description — the baseline.
- **Delta (P-frame):** `{rect, content}` list from XDamage since the last frame.
- **Re-baseline cadence:** every *N* seconds, or when cumulative damage area
  exceeds a threshold.
- **Semantic tier (later):** structured deltas ("button 'OK' appeared at x,y")
  via EWMH + AT-SPI accessibility, with a pixel crop as fallback — tiny context,
  model-legible.

Phasing: ship the **pixel-rect tier first** (works with today's vision models),
add the semantic tier after. A containerized `Xvfb` + Window Maker + `ai-mcp`
image is the flagship near-term artifact and needs no new ML.

## 7. Engineering invariants (enforced)

- **No core linkage.** `ng-*`/`ai-*` never depend on Layer 1 at compile time.
- **No D-Bus in a hot loop.** All system I/O is async and out-of-process.
- **Latest, non-deprecated deps**, declared once in `[workspace.dependencies]`.
- **Version only in git tags**, injected by the Makefile.
- **Gates:** fmt-check · clippy (deny warnings) · build · test · cargo-audit ·
  cargo-deny — locally via `make ci-local`, in CI via `validate.yml` (Actions
  pinned by commit SHA).

See PLAN §9 for open decisions (license per layer, dockapp vs. tray, MCP
transport, `core-patches/` canonicality).
