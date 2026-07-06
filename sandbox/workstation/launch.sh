#!/bin/sh
set -eu

: "${WORKSPACE_DIR:=/workspace}"
: "${WORKSPACE_HOME:=$WORKSPACE_DIR/home}"
: "${USER_DATA_DIR:=/profile}"

export HOME="$WORKSPACE_HOME"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_STATE_HOME="$HOME/.local/state"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$WORKSPACE_DIR/runtime}"
export NO_AT_BRIDGE=0
export GTK_MODULES="${GTK_MODULES:+$GTK_MODULES:}gail:atk-bridge"
export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1

mkdir -p \
    "$USER_DATA_DIR" \
    "$HOME" \
    "$XDG_CONFIG_HOME" \
    "$XDG_CACHE_HOME" \
    "$XDG_DATA_HOME" \
    "$XDG_STATE_HOME" \
    "$XDG_RUNTIME_DIR"
chmod 0700 "$XDG_RUNTIME_DIR"

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-launch >/dev/null 2>&1; then
    eval "$(dbus-launch --sh-syntax)"
    export DBUS_SESSION_BUS_ADDRESS DBUS_SESSION_BUS_PID
fi

if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment \
        DISPLAY \
        HOME \
        USER \
        XDG_RUNTIME_DIR \
        XDG_CONFIG_HOME \
        XDG_CACHE_HOME \
        XDG_DATA_HOME \
        XDG_STATE_HOME \
        GTK_MODULES \
        NO_AT_BRIDGE \
        QT_LINUX_ACCESSIBILITY_ALWAYS_ON >/dev/null 2>&1 || true
fi

if command -v at-spi-bus-launcher >/dev/null 2>&1; then
    at-spi-bus-launcher --launch-immediately >/tmp/at-spi.log 2>&1 &
fi

exec /usr/local/bin/wmaker-ai-browser
