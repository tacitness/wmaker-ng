# CLAUDE.md — wmaker-ng

The Rust companion workspace for the wmaker-ng modernization. Read
[AGENTS.md](AGENTS.md) for the full operating rules, [PLAN.md](PLAN.md) for the
architecture map, and [ARCHITECTURE.md](ARCHITECTURE.md) for layer detail.

## Key rules

- **The C core is sacred and lives elsewhere** (`tacitness/wmaker`). Nothing here
  compile-links against it; layers are decoupled at runtime (D-Bus / EWMH / MCP).
- **Lightweight is the product.** Out-of-process, event-driven, idle-until-poked.
  Never put synchronous D-Bus or capture work in a hot loop.
- **Latest, non-deprecated dependencies only.** Verify current stable versions;
  declare them once in `[workspace.dependencies]`.
- **Version lives only in git tags** — injected by the Makefile, never written
  into `Cargo.toml`.
- Keep changes minimal and validated before pushing. Never add or rotate secrets.

## Quality gate

```bash
make pre-commit   # fmt-check + clippy (deny warnings) + secret-scan (before every commit)
make pre-push     # build + test
make ci-local     # full parity with CI (before every PR)
make hooks        # install repo-managed git hooks
```

## Workspace

Eight crates under `crates/`: shared `wmng-{x11,dbus,ewmh}`; the `ng-*` daemons
(`automount`, `power`, `notify`) → `wmaker-ng`; `ai-mcp` + `ai-proto` →
`wmaker-ai`. Dependency direction: **ai → ng → shared**.
