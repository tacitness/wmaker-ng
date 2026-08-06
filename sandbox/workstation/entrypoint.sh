#!/bin/sh
# Recover safely from a container restart inside the same Kubernetes Pod.
# Kubernetes emptyDir volumes survive container restarts, so Xvfb's lock and
# socket can outlive the process and otherwise cause a permanent restart loop.
set -eu

: "${DISPLAY:=:99}"
: "${WMAKER_X_RUNTIME_DIR:=/tmp}"

display_number=${DISPLAY#:}
display_number=${display_number%%.*}
lock_file="$WMAKER_X_RUNTIME_DIR/.X${display_number}-lock"
socket_file="$WMAKER_X_RUNTIME_DIR/.X11-unix/X${display_number}"

if [ -f "$lock_file" ]; then
	lock_pid=$(tr -cd '0-9' <"$lock_file")
	if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then
		echo "[wmaker-ai-workstation] X display $DISPLAY is owned by live pid $lock_pid" >&2
		exit 70
	fi
	echo "[wmaker-ai-workstation] removing stale X lock for $DISPLAY" >&2
	rm -f "$lock_file" "$socket_file"
elif [ -S "$socket_file" ] || [ -e "$socket_file" ]; then
	echo "[wmaker-ai-workstation] removing orphaned X socket for $DISPLAY" >&2
	rm -f "$socket_file"
fi

exec /usr/local/bin/wmaker-headless "$@"
