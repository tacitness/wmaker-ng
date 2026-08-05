# wmaker-ai-workstation (#90)

`wmaker-ai-workstation` is an explicit opt-in layer above
`wmaker-ai-browser` for supervised content/research desktops:

```text
wmaker-crm:headless
  -> wmaker-ai-sandbox
     -> wmaker-ai-browser
        -> wmaker-ai-workstation
```

It keeps the lightweight sandbox/browser images unchanged at rest while adding:

- terminal: `xfce4-terminal` with `xterm` fallback
- browsers: existing Brave plus guaranteed Chromium and Mozilla Firefox
- content tools: Blender, LibreOffice, GIMP, Inkscape, FFmpeg, ImageMagick
- session plumbing: `xdg-utils`, `dbus-x11`, AT-SPI runtime, fonts, MIME data
- deterministic launchers: `wmaker-open-terminal`, `wmaker-open-browser`,
  `wmaker-open-chrome`, `wmaker-open-firefox`, `wmaker-open-blender`,
  `wmaker-open-libreoffice`, `wmaker-open-gimp`,
  `wmaker-open-inkscape`

Firefox comes from Mozilla's signed APT repository, with the repository key
fingerprint verified during the build. This avoids Ubuntu Noble's Snap
transition package while retaining a deterministic non-Snap install.

## Build

```bash
make sandbox-workstation-image
```

## Mount contract

- `/profile`: browser-only identity/profile state for Brave, Chromium, and
  Firefox (`/profile/firefox`).
- `/workspace`: writable home/artifact mount for application settings and user
  outputs.
- `/workspace/home`: exported as `HOME`, with `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME`, `XDG_DATA_HOME`, and `XDG_STATE_HOME` rooted beneath it.

The image stays immutable. Do not bake cookies, credentials, MFA state, or user
files into layers. Single-writer rules still apply to `/profile`; if a prior
browser instance left stale `Singleton*` locks behind, use `CLEAR_SINGLETON=1`
only when the original browser is known to be stopped.

In Kubernetes, keep `/profile` and `/workspace` on separate claims. `/profile`
is single-writer browser identity state for Brave/Chromium/Firefox; `/workspace`
is the writable home and artifact lane for terminal, office, image, and render
tools.

## Runtime envelope

- security: non-root pod, `RuntimeDefault` seccomp, all capabilities dropped,
  no service-account token mount
- workstation resources: request `2` vCPU / `4Gi` RAM / `8Gi` ephemeral, limit
  `6` vCPU / `12Gi` RAM / `16Gi` ephemeral
- browser sandboxing: Chromium-family launchers keep the existing
  `--no-sandbox` container contract visible; this issue does not weaken it
  further

## Run

```bash
docker run -i --rm \
  -v "$PWD/profile:/profile" \
  -v "$PWD/workspace:/workspace" \
  wmaker-ai-workstation
```

The launcher establishes a session D-Bus, enables AT-SPI for GTK clients, keeps
browser state on `/profile`, and moves app/home state to `/workspace/home`
before handing off to `wmaker-ai-browser`.

## Smoke

```bash
python3 scripts/smoke-workstation.py
```

The smoke proves:

- `open terminal` resolves through the deterministic launcher path
- terminal startup writes a nonce from the launched shell session
- Brave, Chromium, Firefox, Blender, LibreOffice, GIMP, and Inkscape open visible windows
- Blender background render produces bounded artifacts
- FFmpeg/ImageMagick transform a bounded fixture
- AT-SPI returns a non-empty accessibility tree
- `/profile` and `/workspace` survive container replacement
