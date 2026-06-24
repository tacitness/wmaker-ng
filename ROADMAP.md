# Roadmap

Mirror of [PLAN.md](PLAN.md) §8, kept beside the code as a checklist. PLAN.md
remains the source of truth until superseded by code + ARCHITECTURE.md.

## Week 1 — Foundation
- [ ] Create `tacitness/wmaker` C fork; wire `upstream`/`origin`; CI proving the
      core still builds. *(separate C repo — see PLAN §2)*
- [x] Scaffold this Cargo workspace, `.githooks`, CI/CD, lint/format/audit
      gating, `AGENTS.md`/`CLAUDE.md`, Makefile.
- [ ] Establish the rebase ritual (monthly fetch+rebase of upstream).

## Week 2 — Prove the patterns (parallel spikes)
- [ ] `ng-automount` — udisks2 reactor + dockapp (validates Layer 2 end-to-end).
- [ ] `ai-mcp` skeleton — XTEST + XShm + EWMH exposed as MCP tools; drive by hand.

## Week 3 — Sandbox + protocol v0
- [ ] Containerize Xvfb + wmaker + `ai-mcp`; demo an external agent clicking a button.
- [ ] `ai-proto` v0: XDamage → dirty-rect deltas + keyframe re-baseline; measure
      context size vs. full-frame.

## Week 4 — Package + first core mod
- [ ] `nfpm` recipes → deb/rpm/apk; draft subscribable repo.
- [ ] Tiling-mode spike in `core-patches/`; study upstream's `git send-email` flow.

## Parked (Phase 4+)
- [ ] Semantic delta tagging (AT-SPI).
- [ ] Distilled ~1B vision model that natively speaks the protocol (`ml/`).
- [ ] Gitea migration (mechanical when ready).
