# AGENTS.md — wmaker-ng

This repository is the **Rust companion workspace** for the wmaker-ng
modernization (charter in [README.md](README.md), full map in [PLAN.md](PLAN.md),
layer detail in [ARCHITECTURE.md](ARCHITECTURE.md)). It holds both facets —
`ng-*` (modern Linux integration, no AI) and `ai-*` (the MCP control plane) —
plus the `ml/` Python tooling for Phase 4.

The C window-manager core lives in a **separate** repository
(`tacitness/wmaker`, rebased on `repo.or.cz/wmaker-crm`). Do not vendor or edit
core C here; the layers are decoupled at runtime (D-Bus / EWMH / MCP).

## Rules

- **Keep the core sacred.** Nothing in this repo compile-links against the C
  core. New behavior is new out-of-process binaries that speak D-Bus, EWMH,
  XDamage/XTEST/XShm, and MCP. Core edits — if ever — happen in the C repo as a
  small, documented, upstream-bound patch series (mirrored under
  `core-patches/`).
- **Lightweight is the product.** No feature may tax the idle desktop. If it
  cannot be added out-of-process, behind a flag, or disabled by default, it does
  not land. Never add synchronous D-Bus into a hot loop.
- **Latest, non-deprecated dependencies only.** Verify the current stable
  version before adding or bumping a crate. Declare shared versions once in
  `[workspace.dependencies]`; crates opt in with `dep.workspace = true`.
- **Versioning lives only in git tags**, injected by the Makefile. Never write a
  version number into `Cargo.toml` (the `0.0.0` placeholder stays).
- **Run the gates before committing and before marking any PR ready.**
- **Never add or rotate secrets from this repo.** The pre-commit secret scan is
  a backstop, not a license to be careless.
- Keep diffs minimal and task-focused. Output should state: summary, files
  touched, validation, risks, next step.

## Quality gate

```bash
make pre-commit   # fmt-check + clippy (deny warnings) + secret-scan
make pre-push     # build + test
make ci-local     # full parity with .github/workflows/validate.yml
make hooks        # install the repo-managed git hooks (.githooks/)
```

CI (`.github/workflows/validate.yml`) runs: fmt-check · clippy (deny warnings) ·
build · test · cargo-audit · cargo-deny. All third-party Actions are pinned by
commit SHA with a version comment — bump the SHA and the comment together.

## Workspace map

| Crate          | Kind | Facet      | Purpose                                              |
|----------------|------|------------|------------------------------------------------------|
| `wmng-x11`     | lib  | shared     | XDamage / XTEST / XShm / XFixes (x11rb)              |
| `wmng-dbus`    | lib  | shared     | udisks2 / logind / upower async client (zbus)        |
| `wmng-ewmh`    | lib  | shared     | `_NET_*` window-control client                        |
| `ng-automount` | bin  | wmaker-ng  | UDisks2 reactor + dockapp                             |
| `ng-power`     | bin  | wmaker-ng  | logind/upower session daemon                          |
| `ng-notify`    | bin  | wmaker-ng  | `org.freedesktop.Notifications` server                |
| `ai-mcp`       | bin  | wmaker-ai  | MCP server (input synth / capture / window control)   |
| `ai-proto`     | lib  | wmaker-ai  | screen-diff protocol + codec                          |

Dependency direction is strictly **ai → ng → shared → (runtime) core**.

## Packaging

`nfpm` recipes in `packaging/nfpm/` produce deb/rpm/apk for the `wmaker-ng` and
`wmaker-ai` packages across {glibc(EL9 floor), musl} × {amd64, arm64}, published
signed to `repos.tacitsoft.dev`. The tag-driven pipeline, target matrix, and
required CI secrets are documented in [RELEASING.md](RELEASING.md). See
`make packages` / `make release-local` for local builds.
