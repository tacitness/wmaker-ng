# Packaging

`nfpm` recipes that turn the release binaries into deb/rpm/apk packages for
publication to `repos.tacitsoft.dev` (PLAN §7). Two packages map onto the
facets:

- **`wmaker-ng`** — the `ng-*` daemons (auto-mount, power, notify).
- **`wmaker-ai`** — the `ai-mcp` server; `depends:` on `wmaker-ng`.

The `wmaker` C core ships from its own repository (`tacitness/wmaker-crm`).

One recipe per package serves every target — the Makefile drives the matrix by
exporting `PKG_VERSION` / `PKG_ARCH` / `WMNG_STAGE` and rendering with
`envsubst` before calling `nfpm` (see `scripts/package.sh`).

## Build

```bash
make cross-build     # glibc(EL9 floor) + musl static, amd64 + arm64
make packages        # → dist/pkg/*.{deb,rpm,apk}  (deb/rpm from glibc, apk from musl)
make release-local   # cross-build + packages + tarballs, no signing/publish
```

Versioning comes from the git tag via the Makefile (`PKG_VERSION`); it is never
written into the recipes. The full tag → signed multi-arch repos flow, the
target matrix, required CI secrets, and consumer install instructions live in
[../RELEASING.md](../RELEASING.md). This is still a **skeleton** — the recipes
package binaries that gain behavior as the daemons are implemented (PLAN §8).

Requires [`nfpm`](https://nfpm.goreleaser.com) and `cargo-zigbuild` + `zig`:
see `make install-cross-tools`.
