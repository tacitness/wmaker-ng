FROM wmaker-ai-workstation

LABEL org.opencontainers.image.title="wmaker-ai-workstation-vnc"
LABEL org.opencontainers.image.description="Supervised VNC overlay for wmaker-ai-workstation."
LABEL org.opencontainers.image.source="https://github.com/tacitness/wmaker-ng"

RUN apt-get update \
    && apt-get install -y --no-install-recommends x11vnc \
    && rm -rf /var/lib/apt/lists/*

COPY sandbox/workstation-vnc/launch.sh /usr/local/bin/wmaker-ai-workstation-vnc
RUN chmod 0755 /usr/local/bin/wmaker-ai-workstation-vnc

CMD ["wmaker-ai-workstation-vnc"]
