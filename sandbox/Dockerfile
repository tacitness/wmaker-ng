# wmaker-ai sandbox (#18): Xvfb + Window Maker + ai-mcp in one image.
# The base entrypoint (wmaker-headless) brings up Xvfb + wmaker, then execs the
# CMD into the live desktop — so ai-mcp serves MCP against a real running WM.
FROM wmaker-crm:headless

LABEL org.opencontainers.image.title="wmaker-ai-sandbox"
LABEL org.opencontainers.image.description="Xvfb + Window Maker + ai-mcp — a scriptable, MCP-drivable desktop."
LABEL org.opencontainers.image.source="https://github.com/tacitness/wmaker-ng"

# A couple of trivial X clients (xclock/xeyes) so the desktop has something to
# see and click in demos/tests. ~small; drop if you want a bare image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends x11-apps \
    && rm -rf /var/lib/apt/lists/*

# Prebuilt on the host and copied in (see `make sandbox-image`).
COPY ai-mcp /usr/local/bin/ai-mcp

# ai-mcp serves MCP over stdio — drive with `docker run -i`.
CMD ["ai-mcp"]
