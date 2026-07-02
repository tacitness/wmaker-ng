# wmaker-ai-browser (#20): a real browser INSIDE the headless wmaker desktop.
#
# Layering:  wmaker-crm:headless  →  wmaker-ai-sandbox  →  wmaker-ai-browser
#            (Xvfb + wmaker)          (+ ai-mcp)            (+ Brave/Chromium)
#
# The base `wmaker-headless` entrypoint brings up Xvfb + wmaker, then execs our
# CMD. `wmaker-ai-browser` (launch.sh) starts the browser as a background X
# client on the live desktop, then execs `ai-mcp` — so an MCP agent drives a
# real browsing session. The WM never learns it is being driven.
FROM wmaker-ai-sandbox

LABEL org.opencontainers.image.title="wmaker-ai-browser"
LABEL org.opencontainers.image.description="Headless wmaker desktop + Brave/Chromium + ai-mcp — an agent-drivable browser."
LABEL org.opencontainers.image.source="https://github.com/tacitness/wmaker-ng"

# Brave (default, matches the operator's host browser/profile) via its apt repo.
# Chromium is a best-effort fallback X client; on Ubuntu the archive `chromium`
# is a snap stub, so we pull a real .deb from the community xtradeb PPA and do
# NOT fail the build if that source is unavailable.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends curl ca-certificates gnupg fonts-liberation; \
    curl -fsSLo /usr/share/keyrings/brave-browser-archive-keyring.gpg \
        https://brave-browser-apt-release.s3.brave.com/brave-browser-archive-keyring.gpg; \
    echo "deb [signed-by=/usr/share/keyrings/brave-browser-archive-keyring.gpg] https://brave-browser-apt-release.s3.brave.com/ stable main" \
        > /etc/apt/sources.list.d/brave-browser-release.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends brave-browser; \
    ( curl -fsSLo /usr/share/keyrings/xtradeb.gpg \
        https://keyserver.ubuntu.com/pks/lookup?op=get\&search=0x4d4d3da3f8aa6e15ddd2c8a05068f0f4a4d4d3da 2>/dev/null \
        && echo "deb [signed-by=/usr/share/keyrings/xtradeb.gpg] https://ppa.launchpadcontent.net/xtradeb/apps/ubuntu noble main" \
            > /etc/apt/sources.list.d/xtradeb.list \
        && apt-get update \
        && apt-get install -y --no-install-recommends chromium \
        && echo "chromium: installed" ) \
      || echo "chromium: skipped (fallback source unavailable) — Brave remains the default"; \
    rm -rf /var/lib/apt/lists/*

COPY launch.sh /usr/local/bin/wmaker-ai-browser
RUN chmod +x /usr/local/bin/wmaker-ai-browser

# Browser launch defaults; override at `docker run` time.
#   BROWSER        which browser binary to launch (brave-browser | chromium)
#   START_URL      page to open on boot
#   USER_DATA_DIR  --user-data-dir (bind-mount the host profile here)
ENV BROWSER=brave-browser \
    START_URL=about:blank \
    USER_DATA_DIR=/profile

# Xvfb + wmaker (base entrypoint) → browser (background) → ai-mcp (stdio).
CMD ["wmaker-ai-browser"]
