#!/bin/bash
# Start the guest X stack only. Never talks to a host display.
set -euo pipefail

export DISPLAY="${DISPLAY:-:99}"
export WIDTH="${WIDTH:-1280}"
export HEIGHT="${HEIGHT:-800}"
export HOME="${HOME:-/home/berth}"
export USER="${USER:-berth}"
export LANG="${LANG:-C.UTF-8}"
export LC_ALL="${LC_ALL:-C.UTF-8}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/runtime-${USER}}"
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-openbox}"

mkdir -p "$XDG_RUNTIME_DIR" "$HOME" /workspace /tmp/.X11-unix
chmod 700 "$XDG_RUNTIME_DIR"
chmod 1777 /tmp/.X11-unix || true
cd /workspace

log() { echo "[berth] $*" >&2; }

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-launch >/dev/null; then
  eval "$(dbus-launch --sh-syntax)"
  export DBUS_SESSION_BUS_ADDRESS DBUS_SESSION_BUS_PID
fi

wait_for_x() {
  local n=0
  while [ "$n" -lt 50 ]; do
    if xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
      return 0
    fi
    n=$((n + 1))
    sleep 0.1
  done
  return 1
}

wait_port() {
  local port="$1" n=0
  while [ "$n" -lt 50 ]; do
    if bash -c "echo >/dev/tcp/127.0.0.1/${port}" 2>/dev/null; then
      return 0
    fi
    n=$((n + 1))
    sleep 0.1
  done
  return 1
}

xvfb_pid=""
openbox_pid=""
tint2_pid=""
x11vnc_pid=""
novnc_pid=""
xterm_pid=""

cleanup() {
  trap - EXIT INT TERM
  rm -f /tmp/berth-ready
  for pid in "${novnc_pid:-}" "${x11vnc_pid:-}" "${xterm_pid:-}" "${tint2_pid:-}" "${openbox_pid:-}" "${xvfb_pid:-}"; do
    if [ -n "$pid" ]; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  if [ -n "${DBUS_SESSION_BUS_PID:-}" ]; then
    kill "$DBUS_SESSION_BUS_PID" 2>/dev/null || true
  fi
}

trap cleanup EXIT
trap 'cleanup; exit 0' INT TERM

log "starting Xvfb ${DISPLAY} ${WIDTH}x${HEIGHT}x24"
Xvfb "$DISPLAY" -screen 0 "${WIDTH}x${HEIGHT}x24" \
  -ac -noreset -nolisten tcp -dpi 96 \
  +extension RANDR +extension GLX +extension RENDER +extension XTEST \
  >/tmp/xvfb.log 2>&1 &
xvfb_pid=$!

if ! wait_for_x; then
  log "Xvfb failed to start"
  cat /tmp/xvfb.log >&2 || true
  exit 1
fi

if command -v xsetroot >/dev/null; then
  xsetroot -solid "#2e3440" || true
fi

log "starting openbox"
openbox >/tmp/openbox.log 2>&1 &
openbox_pid=$!

log "starting tint2"
tint2 >/tmp/tint2.log 2>&1 &
tint2_pid=$!

xterm -ls -geometry 100x28+24+24 >/tmp/xterm.log 2>&1 &
xterm_pid=$!

log "starting x11vnc :5900"
x11vnc -display "$DISPLAY" -forever -shared -nopw \
  -rfbport 5900 -wait 10 -noxdamage -repeat \
  >/tmp/x11vnc.log 2>&1 &
x11vnc_pid=$!

if ! wait_port 5900; then
  log "x11vnc failed to listen on :5900"
  cat /tmp/x11vnc.log >&2 || true
  exit 1
fi

log "starting noVNC :6080"
websockify --web=/usr/share/novnc 0.0.0.0:6080 localhost:5900 \
  >/tmp/novnc.log 2>&1 &
novnc_pid=$!

if ! wait_port 6080; then
  log "noVNC failed to listen on :6080"
  cat /tmp/novnc.log >&2 || true
  exit 1
fi

# let openbox/tint2/xterm map before the first screenshot
sleep 0.4
touch /tmp/berth-ready
log "desktop ready DISPLAY=${DISPLAY} VNC=:5900 noVNC=:6080"

if [ "$#" -gt 0 ]; then
  "$@"
  exit $?
fi

wait "$xvfb_pid"
