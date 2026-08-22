#!/usr/bin/env bash
# Assert the guest image actually filters egress.
#
# Runs the real image and the real entrypoint, because the property under test
# lives in the entrypoint's iptables rules, not in any Rust code. Uses a
# user-defined Docker network on purpose: Docker only runs its embedded resolver
# at 127.0.0.11 there, and that resolver is what let an earlier "deny-all" guest
# still resolve names (nat OUTPUT DNATs it off port 53 before filter OUTPUT
# sees it, so a --dport 53 match never fires).
#
# Usage: images/linux-xfce/test-egress.sh [image-tag]
set -euo pipefail

IMAGE="${1:-berthos-linux-xfce:dev}"
NET="berth-egress-test-$$"
CONTAINERS=()
FAILURES=0

cleanup() {
  for c in "${CONTAINERS[@]:-}"; do [ -n "$c" ] && docker rm -f "$c" >/dev/null 2>&1 || true; done
  docker network rm "$NET" >/dev/null 2>&1 || true
}
trap cleanup EXIT

pass() { echo "  ok   $1"; }
fail() { echo "  FAIL $1"; FAILURES=$((FAILURES + 1)); }

# Never pipe into grep -q here. It exits on the first match, the producer takes
# SIGPIPE, and `set -o pipefail` turns that into a failed pipeline -- a matched
# assertion reported as a failure, intermittently, depending on how fast the
# producer flushes. Capture first, match in the shell.
logs_of() { docker logs "$1" 2>&1 || true; }

# Reaching a real host over the public internet is the one assertion here that
# can fail for reasons that have nothing to do with the egress filter. A single
# TCP timeout from a CI runner is not evidence the allowlist is broken, so give
# it a few attempts before calling it a failure. The negative assertions are
# never retried: a host that stays blocked is the whole point, and retrying
# would only make a leak harder to see.
reachable() { # $1 container, $2 url
  local i
  for i in 1 2 3; do
    if as_agent "$1" "curl -sS --max-time 20 -o /dev/null $2"; then
      return 0
    fi
    sleep 2
  done
  return 1
}
has() { case "$2" in *"$1"*) return 0 ;; *) return 1 ;; esac; }

# Root inside the guest; PATH is trimmed for the berth user so name it in full.
rules() { docker exec -u root "$1" /usr/sbin/iptables -S OUTPUT 2>/dev/null; }
# The unprivileged user an agent actually runs as.
as_agent() { docker exec -u berth "$1" sh -c "$2" >/dev/null 2>&1; }

start_guest() { # $1 name, $2 BERTH_ALLOWLIST value
  local name="$1" allow="$2"
  CONTAINERS+=("$name")
  docker run -d --name "$name" --network "$NET" \
    --cap-add NET_ADMIN --cap-add SETUID --cap-add SETGID --cap-add SETPCAP \
    -e BERTH_ALLOWLIST="$allow" "$IMAGE" >/dev/null
  # Wait on the egress log line, not on the DROP policy: apply_egress sets the
  # policy first and logs last, so keying on the policy races the rest of the
  # ruleset into existence and reads a half-built chain.
  for _ in $(seq 1 30); do
    running="$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null || true)"
    if [ "$running" != "true" ]; then
      echo "  container $name exited; logs:"; docker logs "$name" 2>&1 | sed 's/^/    /' | tail -20
      return 1
    fi
    log_text="$(logs_of "$name")"
    if has "egress deny-all" "$log_text" || has "egress allowlist:" "$log_text"; then
      return 0
    fi
    sleep 1
  done
  echo "  egress rules never appeared in $name; logs:"; docker logs "$name" 2>&1 | sed 's/^/    /' | tail -20
  return 1
}

echo "image: $IMAGE"
label=$(docker image inspect -f '{{index .Config.Labels "berth.egress.version"}}' "$IMAGE" 2>/dev/null || echo "")
[ "$label" = "2" ] && pass "berth.egress.version=2" || fail "berth.egress.version: expected 2, got '${label:-<missing>}'"

docker network create "$NET" >/dev/null

echo
echo "deny-all (BERTH_ALLOWLIST=\"\") -- nothing may leave, DNS included"
if start_guest "berth-egress-deny-$$" ""; then
  g="berth-egress-deny-$$"
  deny_rules="$(rules "$g")"
  has '-P OUTPUT DROP' "$deny_rules"          && pass "default policy DROP"       || fail "default policy is not DROP"
  has '-d 127.0.0.11/32 -j DROP' "$deny_rules" && pass "embedded resolver dropped" || fail "embedded resolver not dropped"
  has "egress deny-all" "$(logs_of "$g")"     && pass "logged deny-all"           || fail "did not log deny-all"
  as_agent "$g" 'getent hosts github.com'           && fail "DNS resolved under deny-all"    || pass "DNS blocked"
  as_agent "$g" 'curl -sS --max-time 8 https://github.com' && fail "TCP reached github.com"  || pass "TCP blocked"
  dnsmasq_count="$(docker exec -u root "$g" sh -c 'ps ax | grep -c "[d]nsmasq"' 2>/dev/null || true)"
  dnsmasq_count="$(printf '%s' "$dnsmasq_count" | tr -dc '0-9')"
  [ "${dnsmasq_count:-0}" -eq 0 ] \
    && pass "no name filter needed" || fail "name filter running in deny-all"
  # Ordering is the whole bug: the resolver DROP has to precede the loopback
  # ACCEPT, or the ACCEPT lets every DNAT-ed lookup straight back out.
  # pipefail would abort the run when grep finds nothing -- that is a FAIL to
  # report, not a reason to stop testing.
  drop_at=$(printf '%s\n' "$deny_rules" | grep -n -- '-d 127.0.0.11/32 -j DROP' | cut -d: -f1 | sed -n 1p)
  lo_at=$(printf '%s\n' "$deny_rules" | grep -n -- '-o lo -j ACCEPT' | cut -d: -f1 | sed -n 1p)
  if [ -n "$drop_at" ] && [ -n "$lo_at" ] && [ "$drop_at" -lt "$lo_at" ]; then
    pass "resolver DROP precedes loopback ACCEPT"
  else
    fail "resolver DROP at '${drop_at:-none}' does not precede loopback ACCEPT at '${lo_at:-none}'"
  fi
else
  fail "deny-all guest did not start"
fi

echo
echo "allowlist (BERTH_ALLOWLIST=github.com) -- only that host may leave"
if start_guest "berth-egress-allow-$$" "github.com"; then
  g="berth-egress-allow-$$"
  has "egress allowlist: github.com" "$(logs_of "$g")" && pass "logged allowlist" || fail "did not log allowlist"
  as_agent "$g" 'getent hosts github.com'                        && pass "DNS resolves"      || fail "DNS blocked, but allowlisted hosts need it"
  reachable "$g" https://github.com \
    && pass "allowed host reachable" \
    || fail "allowlisted host unreachable after 3 attempts"
  as_agent "$g" 'curl -sS --max-time 8 -o /dev/null https://example.com' && fail "non-listed host reachable" || pass "non-listed host blocked"

  # Address rules alone left DNS wide open: a guest could resolve anything and
  # exfiltrate through a domain the attacker runs the nameserver for. Only
  # allowlisted names may be forwarded now.
  # Both names below resolve publicly, so a guest that answers them is really
  # forwarding -- a name that does not exist would NXDOMAIN either way and
  # would prove nothing.
  as_agent "$g" 'getent hosts example.com' && fail "non-listed NAME resolved" || pass "non-listed name refused"
  as_agent "$g" 'getent hosts www.example.com' && fail "non-listed subdomain resolved" || pass "exfil-shaped subdomain refused"
  # And the guest must not be able to skip the filter by asking Docker directly.
  # Assert on the send, not on a reply: a DROPped destination fails sendto with
  # EPERM immediately, whereas waiting for a reply passes against an open
  # resolver too as long as the query is malformed.
  as_agent "$g" "python3 -c \"import socket; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(b'x', ('127.0.0.11', 53))\"" \
    && fail "guest reached the embedded resolver directly" || pass "embedded resolver unreachable by the agent"
else
  fail "allowlist guest did not start"
fi

echo
if [ "$FAILURES" -eq 0 ]; then echo "egress: all assertions passed"; else echo "egress: $FAILURES assertion(s) failed"; fi
exit "$FAILURES"
