# Release Package Matrix

`wmaker-ng` release channels are intentionally explicit about ABI and distro
floor. Generic package names are avoided when a binary floor matters.

Current first-party release lanes:

| Family | Targets | Gate |
| --- | --- | --- |
| RPM EL8 | Rocky/RHEL 8 compatible glibc 2.28 RPMs | `*.el8.*.rpm`, install smoke |
| RPM EL9+ | Rocky/RHEL 9/10, Fedora-compatible glibc 2.34 RPMs | `*.el9.*.rpm`, install smoke |
| DEB | Debian/Ubuntu glibc 2.34 lane | install smoke plus release notes |
| APK | Alpine musl static lane | apk index/signing smoke |
| Static | GNU + musl tarballs | checksums and static channel manifest |
| AUR | `wmaker-ng-bin`, `wmaker-ai-bin` | `.SRCINFO`/PKGBUILD freshness |
| Helm | `charts/wmaker-ng` | lint/render for sandbox/browser/gpu profiles |
| OCI | `wmaker-ai-sandbox`, `wmaker-ai-browser` | anonymous `docker run` smoke |

Deferred downstream lanes:

- COPR and OBS need project ownership/credentials and downstream adoption
  decisions before publishing is automated.
- Nix requires a local Nix validation lane before the flake is release-gated.
- GPU Helm runtime stays dry-run until the cluster display/GPU contract is
  validated outside this repo.

Source builds remain the universal fallback:

```bash
make build
sudo install -m 0755 target/release/ai-mcp /usr/local/bin/ai-mcp
```
