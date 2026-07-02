# Packaging

`nfpm` recipes that turn the release binaries into deb/rpm/apk packages for
publication to `repos.tacitsoft.dev` (PLAN §7), plus AUR `-bin` package files
fed by the release tarballs. Two packages map onto the facets:

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

## Runtime Defaults

The packages install binaries only. They intentionally do not install or enable
systemd/systemd-user units yet:

- `wmaker-ng` ships `ng-automount`, `ng-power`, and `ng-notify` as opt-in
  developer tools until their D-Bus reactors are real.
- `wmaker-ai` ships `ai-mcp` as a stdio MCP server that the driving agent starts
  on the target `DISPLAY`.

This keeps a daily-driver install reversible and idle-clean: no background
daemon starts just because the package was installed.

Versioning comes from the git tag via the Makefile (`PKG_VERSION`); it is never
written into the recipes. The full tag → signed multi-arch repos flow, the
target matrix, required CI secrets, and consumer install instructions live in
[../RELEASING.md](../RELEASING.md). This is still a **skeleton** — the recipes
package binaries that gain behavior as the daemons are implemented (PLAN §8).

Requires [`nfpm`](https://nfpm.goreleaser.com) and `cargo-zigbuild` + `zig`:
see `make install-cross-tools`.

## AUR

`packaging/aur/wmaker-ng-bin/` and `packaging/aur/wmaker-ai-bin/` hold the AUR
package files. They intentionally consume the GitHub Release `.tar.gz`
artifacts instead of rebuilding from source:

- `wmaker-ng-bin` installs `ng-automount`, `ng-power`, and `ng-notify`.
- `wmaker-ai-bin` installs `ai-mcp` and depends on `wmaker-ng-bin`.

The committed package files use the early release baseline version and `SKIP`
checksums. During a tag release, `scripts/static-channel.sh` creates musl
`.tar.zst` artifacts, then `scripts/publish-aur.sh` renders the actual tag
version and sha256sums before pushing to the AUR git remotes.

## Static Tarballs

`scripts/static-channel.sh` promotes the musl tarballs into a distro-agnostic
static channel:

- `releases/<version>/wmaker-ng-<version>-amd64-musl.tar.zst`
- `releases/<version>/wmaker-ng-<version>-arm64-musl.tar.zst`
- per-artifact `.sha256` files
- `manifest.json`
- `VERSION`
- `latest -> releases/<version>`
- `install.sh`

The installer detects `x86_64`/`aarch64`, fetches the matching static archive,
verifies the sha256, and installs binaries into `${PREFIX:-/usr/local}/bin`.
