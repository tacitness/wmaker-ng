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

Plus portable `.tar.gz` + `.sha256` per arch/libc, attached to the GitHub
Release (the "compiled binaries" artifact; also seeds the AUR/static channels).

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
   - **publish** — `rsync` to `repos.tacitsoft.dev` under `/srv/repos/wmaker-ng/`.

## Secrets — OIDC + AWS Secrets Manager (house pattern)

No signing keys live as GitHub Actions secrets. The `release` job assumes an AWS
role via **OIDC** and pulls keys from **Secrets Manager** at release time — one
rotatable source of truth, consistent with dagobah-infra (ESO → Secrets
Manager). Signing + publish **gate on the `AWS_ROLE_ARN` repo variable**: until
it is set, the pipeline still builds, packages, assembles *unsigned* repos, and
cuts the GitHub Release; it hardens automatically once infra wires the role.

**Repo variable** (GitHub → Settings → Variables): `AWS_ROLE_ARN` = the OIDC
role to assume (`us-west-2`). Optional overrides: `SM_GPG_KEY`, `SM_APK_KEY`,
`SM_DEPLOY_KEY` if the Secrets Manager paths differ from the defaults below.

**Secrets Manager entries** (`us-west-2`, ops to provision):

| Secret id (default)                          | Contents                                            |
|----------------------------------------------|-----------------------------------------------------|
| `/tacitsoft/wmaker-ng/gpg-signing-key`       | Armored GPG **private** key — signs apt + rpm (key id derived on import) |
| `/tacitsoft/wmaker-ng/apk-signing-key`       | abuild **RSA** private key — signs the apk `APKINDEX` |
| `/tacitsoft/wmaker-ng/repos-deploy-ssh-key`  | SSH private key for `deploy@repos.tacitsoft.dev`    |

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
```
