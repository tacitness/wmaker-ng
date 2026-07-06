# Releasing

How wmaker-ng turns a git tag into signed, multi-arch packages on
`repos.tacitsoft.dev`. House style mirrors tsctl: **version lives only in git
tags**, third-party CI Actions are pinned by commit SHA, and packages publish to
a subscribable repo.

## Cut a release

```bash
make bump-patch   # or bump-minor / bump-major — tags vX.Y.Z and pushes
```

The pushed `v*` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds → packages → signs → publishes and cuts a GitHub Release. No file
edits, no version numbers in source.

## Target matrix

One **glibc** build pinned to the **EL9 floor (glibc 2.34)** is forward
compatible across every supported glibc distro; only **Alpine** needs the
separate **musl-static** build. Two libc × two arches = four binary sets, fanned
into packages by [`nfpm`](https://nfpm.goreleaser.com):

| Build (cargo-zigbuild)              | Packages   | Runs on                                                              |
|-------------------------------------|------------|----------------------------------------------------------------------|
| `*-linux-gnu.2.34` (amd64, arm64)   | deb, rpm   | EL9, EL10, Fedora (latest-2), Debian 12/13, Ubuntu 22.04/24.04 LTS    |
| `*-linux-musl` static (amd64, arm64)| apk        | Alpine (current stable; musl is distro-agnostic)                      |

Plus portable `.tar.gz` + `.sha256` per arch/libc and a first-class static
`.tar.zst` channel from the musl builds. Static artifacts attach to the GitHub
Release and publish under `repos.tacitsoft.dev/releases/wmaker-ng/`.

Architectures: **x86_64** and **aarch64**. Non-EOL versions as of this writing —
revisit when distros roll.

## Pipeline stages

1. **build** (matrix ×4) — `scripts/build.sh` cross-compiles with `cargo-zigbuild`
   (glibc floor pin + musl static) and `scripts/tarball.sh` packs each set.
2. **packages** — `make packages` → `nfpm` renders deb/rpm from the glibc stage
   and apk from the musl stage, both arches.
3. **release** — build the static channel, hand off, cut the GitHub Release:
   - **handoff** — `scripts/publish.sh` stages packages into the `_incoming/`
     contract shape and uploads to `_incoming/wmaker-ng/` (gated on
     `REPOS_BUCKET`). The infra-owned **repo-indexer** (dagobah-infra#306,
     SDD-305 §5) — the single writer — pools them, `rpm --addsign`s, rebuilds
     and GPG/RSA-signs all apt/rpm/apk metadata, and publishes the lineage
     roots. This producer signs nothing and holds no repo keys. (Consequence:
     the rpm files attached to the GitHub Release are unsigned copies; the
     repo-served rpms are signed. sha256s cover both.)
   - **static** — `.tar.zst`, `manifest.json`, `latest`, and `install.sh`
     synced directly (additive) to `/releases/wmaker-ng/` — product-scoped, no
     shared metadata, so no indexer round-trip.
   - Indexer sweeps hourly; for immediate publish:
     `gh workflow run repo-indexer.yml -R tacitness/dagobah-infra`.
   - **AUR** — optionally render `wmaker-ng-bin` / `wmaker-ai-bin` from the
     static `.tar.zst` sha256sums and push to the AUR git remotes.
   - **Gentoo** — overlay ebuilds under `packaging/gentoo/`, fed by the same
     static `.tar.zst` release artifacts.

## Secrets — OIDC only; no repo signing keys in this repo's lane

The `release` job assumes an AWS role via **OIDC**. Since the `_incoming`
cutover (dagobah-infra#306) this producer holds **no repo signing keys** — the
GPG/apk keys live with the infra repo-indexer, which signs everything it
publishes. The only producer-side secret is the AUR deploy key, pulled from
Secrets Manager at release time.

**Repo variables** (GitHub → Settings → Variables): `AWS_ROLE_ARN` = the OIDC
role to assume (`us-west-2`); `REPOS_BUCKET` = the repos S3 bucket (activates
the handoff). Optional: `SM_AUR_KEY` override; set `AUR_PUBLISH=true` only
after the AUR package remotes and deploy key are provisioned.

**Secrets Manager entries** (`us-west-2`):

| Secret id (default)                          | Contents                                            |
|----------------------------------------------|-----------------------------------------------------|
| `/tacitsoft/wmaker-ng/aur-deploy-ssh-key`    | SSH private key for `aur@aur.archlinux.org` package remotes |
| `/tacitsoft/wmaker-ng/{gpg,apk}-signing-key` | **Indexer-only** now — producer IAM no longer grants them |

The OIDC role (`wmaker_ng_publish_mode = "incoming"` in dagobah-infra prod) is
scoped to `_incoming/wmaker-ng/*` + `releases/wmaker-ng/*` — this producer
cannot touch shared metadata, other tenants' trees, or `/keys/`.

## Secret scanning

`gitleaks` runs in the pre-commit hook (`.githooks/secret-scan.sh`, staged
changes) and in CI (`validate.yml`, full history), configured by
[`.gitleaks.toml`](.gitleaks.toml). `make install-dev-tools` installs it; the
hook falls back to a built-in regex scan if gitleaks is absent.

## Local dry run (no signing, no publish)

```bash
make release-local   # cross-build + packages + tarballs into dist/
AUR_DRY_RUN=1 scripts/publish-aur.sh dist/static/"$PKG_VERSION" "$PKG_VERSION"  # render AUR files, no push
```

Repo assembly is no longer local — the dagobah-infra repo-indexer owns it
(`scripts/repo-indexer/indexer.sh` there, runnable against a scratch bucket).
Cross-builds need `cargo-zigbuild` + `zig`; `make install-dev-tools` covers
`cargo-audit`/`cargo-deny`, install the cross toolchain separately.

## Consumer install (once published + signed)

Repos are LINEAGE-FIRST (SDD-305): one enrollment per lineage serves every
TacitSoft tool — select the tool by package name.

```bash
# Debian / Ubuntu
curl -fsSL https://repos.tacitsoft.dev/keys/tacitsoft.gpg \
  | sudo tee /usr/share/keyrings/tacitsoft.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/tacitsoft.gpg] https://repos.tacitsoft.dev/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/tacitsoft.list
sudo apt update && sudo apt install wmaker-ng   # or wmaker-ai

# EL8 / EL9 (RHEL / Rocky / Alma) / Fedora
sudo tee /etc/yum.repos.d/tacitsoft.repo <<'EOF'
[tacitsoft]
name=TacitSoft
baseurl=https://repos.tacitsoft.dev/rpm/el/$releasever/$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://repos.tacitsoft.dev/keys/tacitsoft.gpg
EOF
sudo dnf install wmaker-ng

# Alpine  (apk appends /<arch>/APKINDEX.tar.gz itself)
sudo wget -O /etc/apk/keys/tacitsoft-apk.rsa.pub \
  https://repos.tacitsoft.dev/keys/tacitsoft-apk.rsa.pub
echo "https://repos.tacitsoft.dev/apk/v3.20/main" \
  | sudo tee -a /etc/apk/repositories
sudo apk update && sudo apk add wmaker-ng

# Arch Linux / AUR
paru -S wmaker-ng-bin   # optional: wmaker-ai-bin

# Static tar.zst channel
curl -fsSL https://repos.tacitsoft.dev/releases/wmaker-ng/install.sh | sh

# Gentoo overlay
sudo install -d /var/db/repos/wmaker-ng
sudo rsync -a packaging/gentoo/ /var/db/repos/wmaker-ng/
sudo emerge x11-wm/wmaker-ng   # optional: x11-wm/wmaker-ai
```
