FROM wmaker-ai-workstation

LABEL org.opencontainers.image.title="wmaker-ai-workstation-vnc"
LABEL org.opencontainers.image.description="Supervised VNC overlay for wmaker-ai-workstation."
LABEL org.opencontainers.image.source="https://github.com/tacitness/wmaker-ng"

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3-venv x11vnc \
    && python3 -m venv /opt/mcp-proxy \
    && /opt/mcp-proxy/bin/pip install --no-cache-dir \
        mcp-proxy==0.12.0 mcp==1.27.1 \
    && rm -rf /var/lib/apt/lists/*

COPY sandbox/workstation-vnc/launch.sh /usr/local/bin/wmaker-ai-workstation-vnc
RUN chmod 0755 /usr/local/bin/wmaker-ai-workstation-vnc

CMD ["wmaker-ai-workstation-vnc"]
