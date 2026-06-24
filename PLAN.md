# wmaker-ng — Master Plan & Architecture Mapping

> The charter is in [README.md](README.md). This document is the full
> engineering map: decisions, topology, layer specs, the AI protocol, packaging,
> and the one-month roadmap. It is the source of truth until superseded by code
> + `ARCHITECTURE.md` in the scaffolded tree.

---

## 1. North star

Modernize GNU Window Maker for the contemporary Linux desktop — and make it the
first window manager an AI can drive natively — **without costing the idle
desktop a single avoidable millisecond.** Lightweight is not a constraint on the
product; it *is* the product.

Three shippable tiers (a ladder, not a one-way fork):

| Package     | Contents                         | Audience                       |
|-------------|----------------------------------|--------------------------------|
| `wmaker`    | core fork (C), rebasable         | purists / upstream parity      |
| `wmaker-ng` | core + infra shims               | "modern but lightweight", no AI|
| `wmaker-ai` | ng + MCP / AI control plane      | full AI-native desktop         |

Dependency direction: **ai → ng → core.** You can run ng with zero AI footprint.

---

## 2. Repository topology

```
repo.or.cz/wmaker-crm  --upstream-->  tacitness/wmaker-crm      (C core, rebasable)
                                             |  runtime deps only (D-Bus/EWMH/MCP)
                                             v
                                   tacitness/wmaker-ng       (Rust workspace + ml/)
                                             |  nfpm
                                             v
            packages: wmaker · wmaker-ng · wmaker-ai  -->  repos.tacitsoft.dev
```

**Two repositories, deliberately:**

- **`tacitness/wmaker-crm`** — the C fork. The *only* thing that rebases on
  `repo.or.cz`. Isolated so its history is "upstream + a small patch series" and
  nothing else; rebases never churn Rust history.
- **`tacitness/wmaker-ng`** (this repo) — one Cargo workspace holding **both** the
  `ng` and `ai` facets (they share `x11`/`dbus`/`ewmh` crates), plus a `ml/`
  Python subdir for Phase 4. Two facets, three+ packages, one toolchain.

Remotes on the C fork:

```
upstream  git://repo.or.cz/wmaker-crm.git   (fetch only; push disabled)
origin    git@github.com:tacitness/wmaker-crm.git
# later: gitea remote added, origin re-pointed — a one-line migration checklist item
```

---

## 3. Language decision (ADR-0001)

Optimized for **performance and safety**; difficulty explicitly excluded.

| Layer        | Workload                                   | Language          |
|--------------|--------------------------------------------|-------------------|
| Core         | X11 event loop, window management          | **C**             |
| wmaker-ng    | event-driven D-Bus daemons, dockapps       | **Rust**          |
| wmaker-ai    | MCP server, screen capture/diff, input synth| **Rust**         |
| ML tooling   | data wrangling, model distillation         | **Python** (uv+ruff) |

**Rationale (short form):**
- **C** for the core because it *is* Window Maker — native, rebasable,
  upstreamable. Its weakness (manual memory; cf. upstream's recent
  `_NET_WM_ICON` integer overflow and `wmsetbg` leaks) is exactly why new,
  untrusted-input-facing code does **not** go in C.
- **Rust** for all new systems code: C-class speed with **no GC** (deterministic
  capture-loop latency) *and* compile-time memory + thread safety on a
  network-facing daemon that synthesizes input. Native ecosystem fit:
  `x11rb` (XDamage/XTEST/XShm/XFixes), `zbus`, `rmcp`, `tokio`, `serde`.
- **Python** only where it uniquely wins — the ML/distillation stack —
  quarantined to `ml/`.

Conscious divergence from the tsctl-Go house precedent: tsctl is a CLI
orchestrator (Go's sweet spot); wmaker-ai is a realtime + security workload where
Go's GC jitter and runtime-only race detection cost us on the two chosen axes.

---

## 4. Planned workspace layout

```
wmaker-ng/                         # Cargo workspace
├─ Cargo.toml                      # [workspace] members
├─ crates/
│  ├─ wmng-x11/                    # shared: XDamage, XTEST, XShm, XFixes (x11rb)
│  ├─ wmng-dbus/                   # shared: udisks2 / logind / upower (zbus)
│  ├─ wmng-ewmh/                   # shared: _NET_* window control client
│  ├─ ng-automount/               # bin: udisks2 reactor + dockapp   -> wmaker-ng
│  ├─ ng-power/                   # bin: logind/upower session daemon -> wmaker-ng
│  ├─ ng-notify/                  # bin: org.freedesktop.Notifications-> wmaker-ng
│  ├─ ai-mcp/                     # bin: MCP server (the heart)       -> wmaker-ai
│  └─ ai-proto/                   # lib: screen-diff protocol + codec -> wmaker-ai
├─ core-patches/                   # MIRROR of the few C seams (canonical copy in tacitness/wmaker-crm)
├─ ml/                             # Python (uv): Phase-4 model/distillation tooling
├─ packaging/                      # nfpm recipes -> deb/rpm/apk (+ brew) -> repos.tacitsoft.dev
├─ .githooks/                      # pre-commit, pre-push (house style: tsctl)
├─ .github/workflows/              # validate / release (pinned-SHA actions)
├─ Makefile                        # version from git tags only (house style)
├─ AGENTS.md CLAUDE.md             # agent operating docs (house style)
└─ ARCHITECTURE.md ROADMAP.md
```

---

## 5. Layer specifications

### Layer 1 — Core (C, `tacitness/wmaker-crm`)
- Tracked pristine against `repo.or.cz/wmaker-crm`.
- Permitted edits: a **small, documented, upstream-bound patch series** only —
  ideally single hook-point seams that dispatch into our files.
- First core feature: **tiling mode** + maximize/organization enhancements,
  behind a config flag. Goal: upstream them via `git send-email` and *delete*
  them from our series once accepted.
- Coordinate with upstream's active `_NET_WM` / directional-focus work to avoid
  collisions.

### Layer 2 — wmaker-ng amenities (Rust)
Event-driven, idle-until-poked daemons. **Never** add synchronous D-Bus into the
WM event loop — D-Bus lives only in these companions; they talk to the WM
asynchronously via EWMH.

| Daemon         | System interface (D-Bus)            | Surfaces as            |
|----------------|-------------------------------------|------------------------|
| `ng-automount` | `org.freedesktop.UDisks2`           | dockapp + auto-mount   |
| `ng-power`     | `org.freedesktop.login1` / UPower   | suspend/idle/lid/battery|
| `ng-notify`    | `org.freedesktop.Notifications`     | native notifications   |

UDisks2/logind/upower already do the privileged work; these are thin reactors.

### Layer 3 — wmaker-ai control plane (Rust)
An MCP server exposing computer-use-style tools over existing X11 extensions:

- **Input synthesis** (`move_mouse`, `click`, `type`, `key`) → **XTEST**.
- **Window control** (`list_windows`, `focus`, `move`, `resize`, `tile`) → **EWMH**.
- **Screen capture** (`screenshot`) → **XShm**.
- **Change feed** (`get_changed_regions`) → **XDamage** (see §6).

Model-agnostic: the model lives elsewhere (API, or local llama.cpp/ollama). The
server is a **broker + capture engine**, not an ML runtime.

---

## 6. The screen-diff protocol (ai-proto)

The genuinely novel piece. Grounded in primitives, not reinvented:

- **XDamage** gives exact dirty rectangles per frame — change detection is free.
- **XShm** gives fast shared-memory pixel access.
- The invention is the **encoding** that makes deltas cheap for a model.

Frame model (video-codec analogy):
- **Keyframe (I-frame):** full screen capture/description — the baseline.
- **Delta (P-frame):** `{rect, content}` list from XDamage since last frame.
- **Re-baseline cadence:** every *N* seconds, or when cumulative damage area
  exceeds a threshold — the "rebaseline every XYZ" instinct, formalized.
- **Semantic tier (later):** instead of raw pixel crops, emit structured deltas
  ("button 'OK' appeared at x,y") via EWMH + AT-SPI accessibility, pixel crop as
  fallback. This is what makes context tiny *and* model-legible.

Phasing: ship the **pixel-rect tier first** (works with today's vision models),
add the semantic tier after.

### Containerized GUI sandbox
`Xvfb` + Window Maker + `ai-mcp` in one OCI image = a portable, scriptable
desktop any agent can drive. Flagship near-term artifact; needs no new ML.

### Distilled model (Phase 4, gated)
A small (~1B) vision model that natively speaks the protocol is a real ML
project — **explicitly last**, and **fed by traces** captured while driving the
sandbox with off-the-shelf models. You cannot distill a protocol you have not
validated. Lives in `ml/`.

---

## 7. Engineering standards (house style, from tsctl)

- **Latest, non-deprecated dependencies only.** No deprecated or
  near-deprecated libraries/versions, anywhere.
- **Rust:** `rustfmt` + `clippy` (deny warnings in CI), `cargo-audit` +
  `cargo-deny` (supply-chain / license / advisory gating).
- **Python (`ml/` only):** `uv` for packaging, `ruff` for lint+format, `mypy`.
- **C (core repo):** match upstream Window Maker style exactly.
- **Git hooks** (`.githooks/`): `pre-commit` (fmt + lint + secret scan),
  `pre-push` (build + test).
- **CI** (`.github/workflows/`): build · test · fmt-check · clippy · audit ·
  security scan; **all third-party Actions pinned by commit SHA** with version
  comments.
- **Versioning:** lives **only in git tags**; injected at build via the Makefile.
- **Packaging:** `nfpm` → deb/rpm/apk; publish to a subscribable repo
  (`repos.tacitsoft.dev`; later, Gitea's native package registry).
- **Branch:** `main`.

---

## 8. One-month roadmap

**Week 1 — Foundation**
1. Create `tacitness/wmaker-crm` C fork; wire `upstream`/`origin`; CI proving the
   core still builds. *(wmaker-ng repo + this plan: done.)*
2. Scaffold the Cargo workspace, `.githooks`, CI/CD, lint/format/audit gating,
   `AGENTS.md`/`CLAUDE.md`, Makefile.
3. Establish the rebase ritual (monthly fetch+rebase of upstream).

**Week 2 — Prove the patterns (parallel spikes)**
4. `ng-automount` — udisks2 reactor + dockapp. Validates Layer 2 end-to-end.
5. `ai-mcp` skeleton — XTEST + XShm + EWMH exposed as MCP tools; drive by hand.

**Week 3 — Sandbox + protocol v0**
6. Containerize Xvfb + wmaker + `ai-mcp`; demo an external agent clicking a button.
7. `ai-proto` v0: XDamage → dirty-rect deltas + keyframe re-baseline; measure
   context size vs. full-frame.

**Week 4 — Package + first core mod**
8. `nfpm` recipes → deb/rpm/apk; draft subscribable repo.
9. Tiling-mode spike in `core-patches/`; study upstream's `git send-email`
   contribution process.

**Parked (Phase 4+):** semantic delta tagging (AT-SPI); distilled ~1B model;
Gitea migration (mechanical when ready).

---

## 9. Open decisions

- Final license per layer (provisional: GPL-2.0-or-later for lineage).
- Dockapp vs. tray surface for `ng-automount`.
- MCP transport: stdio vs. socket for the sandbox.
- Whether `core-patches/` is the canonical copy or a mirror of `tacitness/wmaker-crm`
  (current plan: canonical in the C repo, mirrored here for reference).
