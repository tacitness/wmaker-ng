# wmaker-ai-workstation (#90): supervised content/research desktop profile.
#
# Layering:  wmaker-crm:headless → wmaker-ai-sandbox → wmaker-ai-browser
#                                               → wmaker-ai-workstation
#
# The workstation remains opt-in so the base sandbox/browser images keep their
# lighter footprint. This layer adds guaranteed desktop apps, launchers, and
# session plumbing needed for supervised content workflows.
FROM wmaker-ai-browser

LABEL org.opencontainers.image.title="wmaker-ai-workstation"
LABEL org.opencontainers.image.description="Headless wmaker desktop + browsers, terminal, content tools, accessibility plumbing, and ai-mcp."
LABEL org.opencontainers.image.source="https://github.com/tacitness/wmaker-ng"

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends curl ca-certificates gnupg; \
    install -d -m 0755 /etc/apt/keyrings; \
    curl -fsSLo /etc/apt/keyrings/packages.mozilla.org.asc \
        https://packages.mozilla.org/apt/repo-signing-key.gpg; \
    test "$(gpg --show-keys --with-colons /etc/apt/keyrings/packages.mozilla.org.asc \
        | awk -F: '$1 == "fpr" { print $10; exit }')" = \
        "35BAA0B33E9EB396F59CA838C0BA5CE6DC6315A3"; \
    echo "deb [signed-by=/etc/apt/keyrings/packages.mozilla.org.asc] https://packages.mozilla.org/apt mozilla main" \
        > /etc/apt/sources.list.d/mozilla.list; \
    printf '%s\n' \
        'Package: *' \
        'Pin: origin packages.mozilla.org' \
        'Pin-Priority: 1000' \
        > /etc/apt/preferences.d/mozilla; \
    curl -fsSLo /tmp/xtradeb.asc \
        https://keyserver.ubuntu.com/pks/lookup?op=get\&search=0x5301fa4fd93244fbc6f6149982bb6851c64f6880; \
    gpg --batch --dearmor --output /usr/share/keyrings/xtradeb.gpg /tmp/xtradeb.asc; \
    rm -f /tmp/xtradeb.asc; \
    echo "deb [signed-by=/usr/share/keyrings/xtradeb.gpg] https://ppa.launchpadcontent.net/xtradeb/apps/ubuntu noble main" \
        > /etc/apt/sources.list.d/xtradeb.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        chromium \
        firefox \
        xfce4-terminal \
        xterm \
        blender \
        libreoffice \
        libreoffice-gtk3 \
        gimp \
        inkscape \
        ffmpeg \
        imagemagick \
        xdg-utils \
        desktop-file-utils \
        shared-mime-info \
        dbus-x11 \
        at-spi2-core \
        libatk-adaptor \
        fonts-dejavu-core \
        fonts-liberation \
        fonts-noto-core \
        fonts-noto-color-emoji \
        pulseaudio-utils \
        file; \
    update-alternatives --install /usr/bin/x-www-browser x-www-browser /usr/bin/brave-browser 200; \
    update-alternatives --install /usr/bin/x-www-browser x-www-browser /usr/bin/chromium 190; \
    rm -rf /var/lib/apt/lists/*

COPY sandbox/workstation/launch.sh /usr/local/bin/wmaker-ai-workstation
COPY sandbox/workstation/entrypoint.sh /usr/local/bin/wmaker-ai-workstation-entrypoint
COPY sandbox/workstation/bin/ /usr/local/bin/
COPY sandbox/workstation/desktop/ /usr/local/share/applications/
COPY scripts/blender-cylinder.py /usr/share/wmaker-ng/blender-cylinder.py

RUN set -eux; \
    chmod 0755 /usr/local/bin/wmaker-ai-workstation /usr/local/bin/wmaker-ai-workstation-entrypoint /usr/local/bin/wmaker-open-* /usr/local/bin/wmaker-terminal-shell; \
    chmod 0644 /usr/local/share/applications/*.desktop; \
    update-desktop-database /usr/local/share/applications || true

ENV BROWSER=brave-browser \
    START_URL=about:blank \
    USER_DATA_DIR=/profile \
    WORKSPACE_DIR=/workspace \
    WORKSPACE_HOME=/workspace/home \
    WMAKER_TERMINAL_TITLE="wmaker terminal"

ENTRYPOINT ["/usr/local/bin/wmaker-ai-workstation-entrypoint"]
CMD ["wmaker-ai-workstation"]
