# Packaging

`nfpm` recipes that turn the release binaries into deb/rpm/apk packages for
publication to `repos.tacitsoft.dev` (PLAN §7). Two packages map onto the
facets:

- **`wmaker-ng`** — the `ng-*` daemons (auto-mount, power, notify).
- **`wmaker-ai`** — the `ai-mcp` server; `depends:` on `wmaker-ng`.

The `wmaker` C core ships from its own repository (`tacitness/wmaker`).

## Build

```bash
make release        # build optimized binaries into target/release/
make package-deb    # → dist/*.deb   (also: package-rpm, package-apk)
make packages       # all three formats
```

Versioning comes from the git tag via the Makefile (`PKG_VERSION`); it is never
written into the nfpm recipes. This directory is a **skeleton** — the recipes
reference binaries that land as the daemons are implemented (PLAN §8).

Requires [`nfpm`](https://nfpm.goreleaser.com): `go install
github.com/goreleaser/nfpm/v2/cmd/nfpm@latest`.
