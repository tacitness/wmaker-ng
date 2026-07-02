# wmaker-ai-browser (#20)

A real browser **inside** the headless Window Maker desktop, drivable by any MCP
agent. Layered on top of the `wmaker-ai-sandbox` image:

```
wmaker-crm:headless     Xvfb + wmaker, built from the wmaker-crm C source
  └─ wmaker-ai-sandbox   + ai-mcp (drives the desktop over MCP/stdio)
       └─ wmaker-ai-browser   + Brave (default) + Chromium (best-effort fallback)
```

On boot: the base entrypoint brings up `Xvfb` + `wmaker`, then `launch.sh`
starts the browser as a background X client on `DISPLAY=:99` and execs `ai-mcp`.
The container's main process is the MCP transport (stdio) — drive it with
`docker run -i`. The WM never learns it is being driven.

## Build

```bash
make sandbox-browser-image     # builds wmaker-ai-sandbox first, then this layer
```

## Run / drive

The repo ships a dependency-free driver that boots the image, speaks MCP, waits
for the browser window, then proves both observe (`screenshot` /
`changed_regions`) and drive (`click` / `type`):

```bash
python3 scripts/drive-browser.py --url https://example.com --out-dir /tmp/drive
# writes 01-page.png (rendered page) and 02-after-input.png (after typing)
```

Or speak MCP yourself:

```bash
docker run -i --rm -e START_URL=https://example.com wmaker-ai-browser
```

## Profile / auth

| Env / flag        | Default          | Meaning                                       |
|-------------------|------------------|-----------------------------------------------|
| `BROWSER`         | `brave-browser`  | browser binary (`brave-browser` \| `chromium`) |
| `START_URL`       | `about:blank`    | page opened on boot                           |
| `USER_DATA_DIR`   | `/profile`       | `--user-data-dir`                             |
| `CLEAR_SINGLETON` | `0`              | if `1`, clear stale `Singleton*` locks in the profile |

To carry over your real logins/bookmarks, bind-mount the host profile:

```bash
python3 scripts/drive-browser.py \
  --profile ~/.config/BraveSoftware/Brave-Browser \
  --clear-singleton --url https://news.ycombinator.com --out-dir /tmp/drive
```

> **Caveat — the host browser must be closed.** Brave holds a `Singleton` lock on
> a profile while it runs. Driving your *live* profile while host Brave is open
> will refuse to start or race the running instance. For repeatable runs, prefer
> a scoped/throwaway profile seeded from secrets — tracked in #21
> (secrets-managed auth), the intended long-term path.

## Notes

- **Chromium is best-effort.** Ubuntu's archive `chromium` is a snap stub, so the
  image pulls a real `.deb` from a community PPA and does not fail the build if
  that source is unavailable. Brave is the guaranteed default.
- `--no-sandbox` is required (no user namespaces in the container); Brave shows
  an "unsupported flag" infobar — cosmetic.
