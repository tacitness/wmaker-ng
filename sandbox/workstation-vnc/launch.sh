#!/bin/sh
set -eu

: "${DISPLAY:=:99}"
: "${VNC_PASSWORD_FILE:=/run/secrets/wmaker-vnc-password}"

if [ ! -r "$VNC_PASSWORD_FILE" ]; then
    echo "VNC password file is not readable: $VNC_PASSWORD_FILE" >&2
    exit 66
fi

password="$(cat "$VNC_PASSWORD_FILE")"
x11vnc -storepasswd "$password" /tmp/x11vnc.pass >/dev/null
unset password

x11vnc \
    -display "$DISPLAY" \
    -rfbauth /tmp/x11vnc.pass \
    -rfbport 5900 \
    -forever \
    -shared \
    -noxdamage \
    -o /tmp/x11vnc.log &

# Keep the network bridge and its stdio child in the workstation container.
# App launch commands must share the workstation filesystem; a lightweight
# sidecar can observe X11, but cannot execute Blender or browser launchers.
exec /opt/mcp-proxy/bin/mcp-proxy \
    --host 0.0.0.0 \
    --port "${MCP_PORT:-8090}" \
    --pass-environment \
    -- \
    /usr/local/bin/wmaker-ai-workstation
