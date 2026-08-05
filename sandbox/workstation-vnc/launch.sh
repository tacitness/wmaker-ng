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

# The Kubernetes workload uses a network MCP bridge, so no stdio client is
# attached to this container. Keep the inherited MCP process and desktop alive.
tail -f /dev/null | /usr/local/bin/wmaker-ai-workstation
