#!/bin/bash
# Start the guest X stack only. Never talks to a host display.
set -euo pipefail

# Always :99. Ignore inbound DISPLAY so an XQuartz/TCP forward cannot hijack.
export DISPLAY=:99
export WIDTH="${WIDTH:-1280}"
export HEIGHT="${HEIGHT:-800}"
export HOME="${HOME:-/home/berth}"
export USER="${USER:-berth}"
export LANG="${LANG:-C.UTF-8}"
export LC_ALL="${LC_ALL:-C.UTF-8}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/runtime-${USER}}"
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-openbox}"

log() { echo "[berth] $*" >&2; }

# Unset → default hosts. Empty string → deny-all outbound (including DNS).
if [ -z "${BERTH_ALLOWLIST+x}" ]; then
  BERTH_ALLOWLIST="github.com,pypi.org,registry.npmjs.org"
  export BERTH_ALLOWLIST
fi

resolve_hosts() {
  local host="$1"
  if command -v getent >/dev/null 2>&1; then
    getent ahostsv4 "$host" 2>/dev/null | awk '{print $1}' | sort -u || true
    return
  fi
  python3 -c 'import socket,sys
h=sys.argv[1]
s=set()
try:
  for a in socket.getaddrinfo(h, None, socket.AF_INET):
    s.add(a[4][0])
except OSError:
  pass
print("\n".join(sorted(s)))
' "$host" 2>/dev/null || true
}

apply_egress() {
  command -v iptables >/dev/null 2>&1 || {
    log "iptables is not installed; refusing to start without an egress filter"
    return 1
  }

  # Read the real resolvers now: in allowlist mode resolv.conf is later pointed
  # at our own filter, and dnsmasq still needs somewhere to forward to.
  local upstreams
  upstreams="$(awk '/^nameserver[ \t]+/ { print $2 }' /etc/resolv.conf 2>/dev/null || true)"

  iptables -F OUTPUT
  iptables -P OUTPUT DROP
  iptables -A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT \
    || iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

  if [ -z "${BERTH_ALLOWLIST}" ]; then
    # Docker DNATs 127.0.0.11:53 to an ephemeral port in nat OUTPUT, and nat
    # OUTPUT runs before filter OUTPUT. A --dport 53 match therefore never sees
    # the embedded resolver, and the loopback ACCEPT below would let every
    # lookup through -- deny-all would still resolve names. Drop the resolver
    # by address, ahead of that ACCEPT.
    local dns_ns
    while read -r dns_ns; do
      [ -n "$dns_ns" ] || continue
      case "$dns_ns" in
        *[!0-9.]*) continue ;;
      esac
      iptables -A OUTPUT -d "$dns_ns" -j DROP
    done <<EOF
$upstreams
EOF
    # Fixed address of Docker's embedded resolver, even if resolv.conf was
    # edited. -C first so the usual case (it is already the nameserver above)
    # does not add a duplicate rule.
    iptables -C OUTPUT -d 127.0.0.11 -j DROP 2>/dev/null \
      || iptables -A OUTPUT -d 127.0.0.11 -j DROP
    iptables -A OUTPUT -p udp --dport 53 -j DROP
    iptables -A OUTPUT -p tcp --dport 53 -j DROP
  else
    # Address filtering alone still let a guest resolve any name, which is a
    # working exfiltration channel even when no TCP destination is reachable.
    # Only the resolver user may reach upstream now; everything else asks
    # dnsmasq on 127.0.0.1, which forwards allowlisted names and answers
    # NXDOMAIN for the rest. root is allowed because apply_egress has to
    # resolve the allowlist itself, and no guest process can become root: the
    # entrypoint drops to berth with no capabilities and there is no sudo.
    local dns_up
    while read -r dns_up; do
      [ -n "$dns_up" ] || continue
      case "$dns_up" in
        *[!0-9.]*) continue ;;
      esac
      iptables -A OUTPUT -d "$dns_up" -m owner --uid-owner 0 -j ACCEPT
      iptables -A OUTPUT -d "$dns_up" -m owner --uid-owner berthdns -j ACCEPT
      iptables -A OUTPUT -d "$dns_up" -j DROP
    done <<EOF
$upstreams
EOF
    iptables -C OUTPUT -d 127.0.0.11 -j DROP 2>/dev/null || {
      iptables -A OUTPUT -d 127.0.0.11 -m owner --uid-owner 0 -j ACCEPT
      iptables -A OUTPUT -d 127.0.0.11 -m owner --uid-owner berthdns -j ACCEPT
      iptables -A OUTPUT -d 127.0.0.11 -j DROP
    }
  fi
  iptables -A OUTPUT -o lo -j ACCEPT

  if [ -n "${BERTH_ALLOWLIST}" ]; then
    local d ip
    local IFS=,
    for d in ${BERTH_ALLOWLIST}; do
      d="${d#"${d%%[![:space:]]*}"}"
      d="${d%"${d##*[![:space:]]}"}"
      d="$(printf '%s' "$d" | tr '[:upper:]' '[:lower:]')"
      [ -n "$d" ] || continue
      case "$d" in
        *[!a-z0-9.-]*) log "skipping invalid allowlist host"; continue ;;
      esac
      if printf '%s' "$d" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$'; then
        iptables -A OUTPUT -p tcp -d "$d" --dport 443 -j ACCEPT
        iptables -A OUTPUT -p tcp -d "$d" --dport 80 -j ACCEPT
        continue
      fi
      while read -r ip; do
        [ -n "$ip" ] || continue
        case "$ip" in
          *[!0-9.]*) continue ;;
        esac
        iptables -A OUTPUT -p tcp -d "$ip" --dport 443 -j ACCEPT
        iptables -A OUTPUT -p tcp -d "$ip" --dport 80 -j ACCEPT
      done <<EOF
$(resolve_hosts "$d" || true)
EOF
    done
  fi

  if [ -d /proc/sys/net/ipv6 ]; then
    command -v ip6tables >/dev/null 2>&1 || {
      log "ipv6 is present but ip6tables is missing; refusing to start"
      return 1
    }
    ip6tables -F OUTPUT
    ip6tables -P OUTPUT DROP
    ip6tables -A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT 2>/dev/null \
      || ip6tables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
    if [ -z "${BERTH_ALLOWLIST}" ]; then
      ip6tables -A OUTPUT -p udp --dport 53 -j DROP
      ip6tables -A OUTPUT -p tcp --dport 53 -j DROP
    fi
    ip6tables -A OUTPUT -o lo -j ACCEPT
  fi

  # Last, because it repoints resolv.conf: resolving the allowlist above still
  # needs the real resolver.
  if [ -n "${BERTH_ALLOWLIST}" ]; then
    start_name_filter "$upstreams" || return 1
  fi

  if [ -z "${BERTH_ALLOWLIST}" ]; then
    log "egress deny-all (empty allowlist)"
  else
    log "egress allowlist: ${BERTH_ALLOWLIST}"
  fi
}

# Forward only allowlisted names and answer NXDOMAIN for the rest, so a query
# for a name nobody allowlisted never leaves the guest. Address rules alone did
# not stop this: an agent could encode data in a subdomain of a domain the
# attacker controls and read it off their authoritative nameserver.
start_name_filter() {
  local upstreams="$1"
  command -v dnsmasq >/dev/null 2>&1 || {
    log "dnsmasq is not installed; refusing to start without a name filter"
    return 1
  }

  local up
  up="$(printf '%s\n' "$upstreams" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' | head -1 || true)"
  [ -n "$up" ] || up="127.0.0.11"

  local conf=/run/berth-dnsmasq.conf
  {
    printf '%s\n' "no-resolv" "listen-address=127.0.0.1" "bind-interfaces"
    printf '%s\n' "user=berthdns" "group=berthdns" "cache-size=256"
    # `#` is dnsmasq's catch-all and `local=` means answer here, never forward.
    printf '%s\n' "local=/#/"
  } > "$conf"

  local IFS=, d
  for d in ${BERTH_ALLOWLIST}; do
    d="${d#"${d%%[![:space:]]*}"}"
    d="${d%"${d##*[![:space:]]}"}"
    d="$(printf '%s' "$d" | tr '[:upper:]' '[:lower:]')"
    [ -n "$d" ] || continue
    case "$d" in
      *[!a-z0-9.-]*) continue ;;
    esac
    # A bare IP in the allowlist has no name to forward.
    if printf '%s' "$d" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$'; then
      continue
    fi
    printf 'server=/%s/%s\n' "$d" "$up" >> "$conf"
  done

  dnsmasq --conf-file="$conf" --pid-file=/run/berth-dnsmasq.pid || {
    log "dnsmasq failed to start; refusing to start without a name filter"
    return 1
  }

  printf 'nameserver 127.0.0.1\noptions ndots:0\n' > /etc/resolv.conf
  log "dns filter: forwarding only allowlisted names to ${up}"
}

output_policy_is_drop() {
  iptables -S OUTPUT 2>/dev/null | grep -q -- '-P OUTPUT DROP'
}

if [ "$(id -u)" -eq 0 ]; then
  apply_egress
  command -v setpriv >/dev/null 2>&1 || {
    log "setpriv is missing; cannot drop to berth"
    exit 1
  }
  export BERTH_EGRESS_OK=1
  exec setpriv --reuid=berth --regid=berth --init-groups \
    --inh-caps=-all --bounding-set=-all --ambient-caps=-all -- "$0" "$@"
fi

if [ "${BERTH_EGRESS_OK:-}" != 1 ] && ! output_policy_is_drop; then
  log "not root and OUTPUT policy is not DROP; refusing to start without an egress filter"
  exit 1
fi

mkdir -p "$XDG_RUNTIME_DIR" "$HOME" /workspace /tmp/.X11-unix
chmod 700 "$XDG_RUNTIME_DIR"
chmod 1777 /tmp/.X11-unix || true
cd /workspace

alive() { [ -n "${1:-}" ] && kill -0 "$1" 2>/dev/null; }

wait_for_x() {
  local n=0
  while [ "$n" -lt 50 ]; do
    if xdpyinfo -display :99 >/dev/null 2>&1; then
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

start_openbox() {
  openbox >/tmp/openbox.log 2>&1 &
  openbox_pid=$!
}

start_tint2() {
  tint2 >/tmp/tint2.log 2>&1 &
  tint2_pid=$!
}

start_xterm() {
  xterm -ls -fa 'Liberation Mono' -fs 11 -geometry 100x28+24+24 \
    >/tmp/xterm.log 2>&1 &
  xterm_pid=$!
}

start_x11vnc() {
  # localhost only: noVNC/websockify is the published viewer.
  x11vnc -display :99 -forever -shared -nopw -localhost \
    -rfbport 5900 -wait 10 -noxdamage -repeat \
    >/tmp/x11vnc.log 2>&1 &
  x11vnc_pid=$!
}

start_novnc() {
  websockify --web=/usr/share/novnc 0.0.0.0:6080 127.0.0.1:5900 \
    >/tmp/novnc.log 2>&1 &
  novnc_pid=$!
}

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

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-launch >/dev/null; then
  eval "$(dbus-launch --sh-syntax)"
  export DBUS_SESSION_BUS_ADDRESS DBUS_SESSION_BUS_PID
fi
if [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  # so docker exec /usr/bin/chromium shares the session bus
  printf 'export DBUS_SESSION_BUS_ADDRESS=%q\n' "$DBUS_SESSION_BUS_ADDRESS" > /tmp/berth-dbus.env
fi

log "starting Xvfb :99 ${WIDTH}x${HEIGHT}x24"
Xvfb :99 -screen 0 "${WIDTH}x${HEIGHT}x24" \
  -ac -noreset -nolisten tcp -dpi 96 \
  +extension RANDR +extension GLX +extension RENDER +extension XTEST \
  >/tmp/xvfb.log 2>&1 &
xvfb_pid=$!

if ! wait_for_x; then
  log "Xvfb failed to start"
  cat /tmp/xvfb.log >&2 || true
  exit 1
fi

xsetroot -solid "#2e3440" || true

log "starting openbox"
start_openbox
log "starting tint2"
start_tint2
start_xterm

log "starting x11vnc :5900 (localhost)"
start_x11vnc
if ! wait_port 5900; then
  log "x11vnc failed to listen on :5900"
  cat /tmp/x11vnc.log >&2 || true
  exit 1
fi

log "starting noVNC :6080"
start_novnc
if ! wait_port 6080; then
  log "noVNC failed to listen on :6080"
  cat /tmp/novnc.log >&2 || true
  exit 1
fi

# let openbox/tint2/xterm map before the first screenshot
sleep 0.4
touch /tmp/berth-ready
log "desktop ready DISPLAY=:99 noVNC=:6080"

if [ "$#" -gt 0 ]; then
  "$@"
  exit $?
fi

while alive "$xvfb_pid"; do
  alive "$openbox_pid" || { log "openbox died, restarting"; start_openbox; }
  alive "$tint2_pid" || { log "tint2 died, restarting"; start_tint2; }
  alive "$x11vnc_pid" || { log "x11vnc died, restarting"; start_x11vnc; }
  alive "$novnc_pid" || { log "novnc died, restarting"; start_novnc; }
  sleep 2
done

log "Xvfb exited"
exit 1
