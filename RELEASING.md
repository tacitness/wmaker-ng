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
Release and publish under `repos.tacitsoft.dev/wmaker-ng/static/`.

Architectures: **x86_64** and **aarch64**. Non-EOL versions as of this writing —
revisit when distros roll.

## Pipeline stages

1. **build** (matrix ×4) — `scripts/build.sh` cross-compiles with `cargo-zigbuild`
   (glibc floor pin + musl static) and `scripts/tarball.sh` packs each set.
2. **packages** — `make packages` → `nfpm` renders deb/rpm from the glibc stage
   and apk from the musl stage, both arches.
3. **release** — assemble + sign repos, publish, cut the GitHub Release:
   - **apt** — `reprepro`, signed `InRelease` + `Release.gpg` (GPG).
   - **rpm** — `rpm --addsign` packages + `createrepo_c` + signed `repomd.xml` (GPG).
   - **apk** — `APKINDEX` signed with `abuild-sign` (RSA) in an Alpine container.
   - **publish** — `aws s3 sync` into the SDD-126 `repos.tacitsoft.dev`
     S3+CloudFront bucket under `/wmaker-ng/` (gated on `REPOS_BUCKET`).
   - **static** — `.tar.zst`, `manifest.json`, `latest`, and `install.sh` under
     `/static`.
   - **AUR** — optionally render `wmaker-ng-bin` / `wmaker-ai-bin` from the
     static `.tar.zst` sha256sums and push to the AUR git remotes.
   - **Gentoo** — overlay ebuilds under `packaging/gentoo/`, fed by the same
     static `.tar.zst` release artifacts.

## Secrets — OIDC + AWS Secrets Manager (house pattern)

No signing keys live as GitHub Actions secrets. The `release` job assumes an AWS
role via **OIDC** and pulls keys from **Secrets Manager** at release time — one
rotatable source of truth, consistent with dagobah-infra (ESO → Secrets
Manager). Signing **gates on the `AWS_ROLE_ARN` repo variable**; repo
publishing additionally gates on **`REPOS_BUCKET`** (the S3 bucket behind
`repos.tacitsoft.dev`, provisioned by dagobah-infra `public-dist`). Until they
are set, the pipeline still builds, packages, assembles *unsigned* repos, and
cuts the GitHub Release; each lane hardens automatically once infra wires its
variable. Public key halves (GPG + apk RSA) are exported into the repo tree
under `/keys/` at release time.

**Repo variables** (GitHub → Settings → Variables): `AWS_ROLE_ARN` = the OIDC
role to assume (`us-west-2`); `REPOS_BUCKET` = the repos S3 bucket (activates
publishing). Optional overrides: `SM_GPG_KEY`, `SM_APK_KEY`, `SM_AUR_KEY` if
the Secrets Manager paths differ from the defaults below. Set
`AUR_PUBLISH=true` only after the AUR package remotes and deploy key are
provisioned.

**Secrets Manager entries** (`us-west-2`, ops to provision):

| Secret id (default)                          | Contents                                            |
|----------------------------------------------|-----------------------------------------------------|
| `/tacitsoft/wmaker-ng/gpg-signing-key`       | Armored GPG **private** key — signs apt + rpm (key id derived on import) |
| `/tacitsoft/wmaker-ng/apk-signing-key`       | abuild **RSA** private key — signs the apk `APKINDEX` |
| `/tacitsoft/wmaker-ng/aur-deploy-ssh-key`    | SSH private key for `aur@aur.archlinux.org` package remotes |

Publishing needs no deploy key: the same OIDC role gets `s3:PutObject`/
`s3:DeleteObject`/`s3:ListBucket` on the repos bucket (dagobah-infra#252).

> apt/rpm use **GPG**; apk uses a **separate RSA** key. The OIDC role's trust
> policy must include `repo:tacitness/wmaker-ng:*` and its IAM policy must grant
> `secretsmanager:GetSecretValue` on `/tacitsoft/wmaker-ng/*`. Never add secrets
> from this repo; infra provisions them in Secrets Manager.

## Secret scanning

`gitleaks` runs in the pre-commit hook (`.githooks/secret-scan.sh`, staged
changes) and in CI (`validate.yml`, full history), configured by
[`.gitleaks.toml`](.gitleaks.toml). `make install-dev-tools` installs it; the
hook falls back to a built-in regex scan if gitleaks is absent.

## Local dry run (no signing, no publish)

```bash
make release-local   # cross-build + packages + tarballs into dist/
make repo-apt repo-rpm   # assemble unsigned apt/rpm repos locally
AUR_DRY_RUN=1 scripts/publish-aur.sh dist/static/releases/"$PKG_VERSION" "$PKG_VERSION"  # render AUR files, no push
```

`make repo-apk` needs an Alpine host (`apk` + `abuild-sign`). Cross-builds need
`cargo-zigbuild` + `zig`; `make install-dev-tools` covers `cargo-audit`/`cargo-deny`,
install the cross toolchain separately.

## Consumer install (once published + signed)

```bash
# Debian / Ubuntu
curl -fsSL https://repos.tacitsoft.dev/wmaker-ng/apt/wmaker-ng-archive-keyring.asc \
  | sudo tee /etc/apt/keyrings/wmaker-ng.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/wmaker-ng.asc] https://repos.tacitsoft.dev/wmaker-ng/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/wmaker-ng.list
sudo apt update && sudo apt install wmaker-ng   # or wmaker-ai

# EL9/EL10 / Fedora
sudo tee /etc/yum.repos.d/wmaker-ng.repo <<'EOF'
[wmaker-ng]
name=wmaker-ng
baseurl=https://repos.tacitsoft.dev/wmaker-ng/rpm
enabled=1
gpgcheck=1
gpgkey=https://repos.tacitsoft.dev/wmaker-ng/rpm/RPM-GPG-KEY-wmaker-ng
EOF
sudo dnf install wmaker-ng

# Alpine
echo "https://repos.tacitsoft.dev/wmaker-ng/apk/$(apk --print-arch)" \
  | sudo tee -a /etc/apk/repositories
sudo wget -P /etc/apk/keys https://repos.tacitsoft.dev/wmaker-ng/apk/wmaker-ng.rsa.pub
sudo apk update && sudo apk add wmaker-ng

# Arch Linux / AUR
paru -S wmaker-ng-bin   # optional: wmaker-ai-bin

# Static tar.zst channel
curl -fsSL https://repos.tacitsoft.dev/wmaker-ng/static/install.sh | sh

# Gentoo overlay
sudo install -d /var/db/repos/wmaker-ng
sudo rsync -a packaging/gentoo/ /var/db/repos/wmaker-ng/
sudo emerge x11-wm/wmaker-ng   # optional: x11-wm/wmaker-ai
```
