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
    if ! docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null | grep -q true; then
      echo "  container $name exited; logs:"; docker logs "$name" 2>&1 | sed 's/^/    /' | tail -20
      return 1
    fi
    docker logs "$name" 2>&1 | grep -qE 'egress (deny-all|allowlist:)' && return 0
    sleep 1
  done
  echo "  egress rules never appeared in $name; logs:"; docker logs "$name" 2>&1 | sed 's/^/    /' | tail -20
  return 1
}

echo "image: $IMAGE"
label=$(docker image inspect -f '{{index .Config.Labels "berth.egress.version"}}' "$IMAGE" 2>/dev/null || echo "")
[ "$label" = "1" ] && pass "berth.egress.version=1" || fail "berth.egress.version: expected 1, got '${label:-<missing>}'"

docker network create "$NET" >/dev/null

echo
echo "deny-all (BERTH_ALLOWLIST=\"\") -- nothing may leave, DNS included"
if start_guest "berth-egress-deny-$$" ""; then
  g="berth-egress-deny-$$"
  rules "$g" | grep -q -- '-P OUTPUT DROP'          && pass "default policy DROP"            || fail "default policy is not DROP"
  rules "$g" | grep -q -- '-d 127.0.0.11/32 -j DROP' && pass "embedded resolver dropped"      || fail "embedded resolver not dropped"
  docker logs "$g" 2>&1 | grep -q "egress deny-all" && pass "logged deny-all"                || fail "did not log deny-all"
  as_agent "$g" 'getent hosts github.com'           && fail "DNS resolved under deny-all"    || pass "DNS blocked"
  as_agent "$g" 'curl -sS --max-time 8 https://github.com' && fail "TCP reached github.com"  || pass "TCP blocked"
  # Ordering is the whole bug: the resolver DROP has to precede the loopback
  # ACCEPT, or the ACCEPT lets every DNAT-ed lookup straight back out.
  # pipefail would abort the run when grep finds nothing -- that is a FAIL to
  # report, not a reason to stop testing.
  drop_at=$(rules "$g" | grep -n -- '-d 127.0.0.11/32 -j DROP' | head -1 | cut -d: -f1 || true)
  lo_at=$(rules "$g" | grep -n -- '-o lo -j ACCEPT' | head -1 | cut -d: -f1 || true)
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
  docker logs "$g" 2>&1 | grep -q "egress allowlist: github.com" && pass "logged allowlist" || fail "did not log allowlist"
  as_agent "$g" 'getent hosts github.com'                        && pass "DNS resolves"      || fail "DNS blocked, but allowlisted hosts need it"
  as_agent "$g" 'curl -sS --max-time 15 -o /dev/null https://github.com' && pass "allowed host reachable" || fail "allowlisted host unreachable"
  as_agent "$g" 'curl -sS --max-time 8 -o /dev/null https://example.com' && fail "non-listed host reachable" || pass "non-listed host blocked"
else
  fail "allowlist guest did not start"
fi

echo
if [ "$FAILURES" -eq 0 ]; then echo "egress: all assertions passed"; else echo "egress: $FAILURES assertion(s) failed"; fi
exit "$FAILURES"
