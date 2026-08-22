# berth

**A berth is a parked computer an agent can lease.**

v0.1 is a **private Linux outpost**: Claude Code (or Codex / Grok Build) on a
laptop drives an isolated Linux desktop in Docker on a machine you own. The
guest is Linux (linux/arm64 on Apple Silicon). **The host desktop is never
driven.**

Windows, macOS guests, and `class=mesh` are not in v0.1.

**Humans use the node console. Agents stay on CLI/MCP.** Occupancy USD is
quoted, not billed. Force disconnect forfeits host credit (`forfeited`); it is
not a cash fine. No wallets, earnings, or cash-out in v0.1.

```
laptop (CLI / Claude Code / MCP)     browser (operator console)
        │  HTTPS (cloudflared) or loopback
        ▼
parked box (macOS + Docker Desktop/OrbStack, or Linux + Docker)
        │  berth-node on 127.0.0.1:7432  — GET / is the console
        ▼
Linux guest (Xvfb + openbox + Chromium)   ← not Finder, not your cursor
```

## Quick start

Develop on macOS with Docker Desktop or OrbStack. You do not need a Linux
workstation.

```sh
# once — Node 22 on PATH (or a pre-built apps/console/dist) for the real dashboard
docker build -t berthos-linux-xfce:dev images/linux-xfce
cargo install --path crates/berth-cli
berth doctor
```

`berth doctor` must be green (Docker daemon, guest image, `~/.berth` writable).
It does **not** probe the embedded SPA. cloudflared is a warning unless you
want `--tunnel cloudflare`. Unpaired is a warning until `berth pair`.

The guest image check is not "does this image exist" — an image built before
the egress filter existed inspects fine and filters nothing. berth requires the
`berth.egress.version` label the Dockerfile stamps, so
`image ... predates the egress filter` means exactly one thing: rebuild it with
the `docker build` line above. `images/linux-xfce/test-egress.sh` asserts what
the image does to traffic and runs in CI.

### Workspaces

`/workspace` lives on a named Docker volume. Reuse one by naming it, or omit it
and get a fresh disk:

```sh
berth up --workspace ws_myproject   # same /workspace as last time
berth workspace ls                  # what exists, and which are reusable
berth workspace rm ws_old           # refused while a lease is live
```

Agents can pass the same id as `workspace` to the `berth_lease` MCP tool.

A workspace nobody has leased for **7 days** has its disk reclaimed
automatically, because otherwise every `berth up` leaks one forever. Set
`BERTH_WORKSPACE_TTL_DAYS` to change the window, or `0` to turn it off. The
sweep only touches workspaces this node recorded, never one with a live lease,
and Docker refuses to remove a volume that is still attached.

### Node 22 at compile time

The dashboard is compiled into berth. **Node 22 is required at compile time**
for the real UI (`npm` on PATH, or a pre-built `apps/console/dist`).
`cargo install --path crates/berth-cli` without Node embeds the placeholder
page, not the dashboard. If `http://127.0.0.1:7432/` shows that placeholder,
install Node 22 and re-run `cargo install --path crates/berth-cli`.

### Human path (console)

Same Mac, no tunnel. The node starts **parked** (it already accepts leases).

```sh
berth node up
# pairing code: ABCD-EFGH
# listening on 127.0.0.1:7432
# console: http://127.0.0.1:7432/
```

Open [http://127.0.0.1:7432/](http://127.0.0.1:7432/) on this machine.
**Pair this browser.** On loopback the code is shown (`GET /v1/pairing` only).
A tunneled browser types the stderr code — the code is never placed on the
URL. Default pair does **not** revoke other bearers, so you can pair the CLI
**and** this browser. `berth pair --revoke-others` or
[http://127.0.0.1:7432/doctor](http://127.0.0.1:7432/doctor) → **Revoke other
clients** rotates everyone else (this browser stays in; CLI must re-pair).

Then:

1. Home shows **Park / Unpark**. Parked = inventory on (new leases allowed).
   Unpark while a live session exists is **409** (`cannot unpark while a lease
   is live`) — End or Force disconnect first. Creating a lease while unparked
   is **409** (`node is unparked`) **before** Docker starts a guest.
2. **New lease** — wizard **What** (linux; windows/macos disabled) → **Where**
   (this node, must be parked) → **Review** (quoted USD, **not charged**).
   Confirm.
3. **End** is graceful (`DELETE` / `berth end` / MCP `berth_end`) — occupancy
   stays income-eligible. **Force disconnect** (`POST /v1/leases/{id}/force` or
   `berth end --force`) forfeits host income for that row (`forfeited`; badge
   “No income — forced disconnect”). Not a cash fine. MCP cannot force.
   Quotes printed or shown are **not charged**.

`berth view` is node-local noVNC on the mapped guest port. The tunnel does not
publish it. The session pane iframes noVNC only when this browser is on the
parked box; otherwise last screenshot or “use MCP / open viewer on the parked
box.”

Cloudflare origin parameter `httpHostHeader: 127.0.0.1` is **unsupported**.
`Cf-Ray` is still present, so the pairing code stays 404 and the console does
not treat the request as loopback.

Operator HTTP (list, park, force, wizard) is [docs/CONSOLE.md](docs/CONSOLE.md).
That is not the agent protocol.

### Agent path (CLI / MCP)

Agents do not use the console. Pair once (default does not log out the
browser), then `berth up` / `berth mcp`. `berth_end` is graceful.

#### One machine (loopback)

```sh
berth node up
# prints a pairing code; listens on 127.0.0.1:7432
# console: http://127.0.0.1:7432/

# another terminal, same machine
berth pair --code ABCD-EFGH
berth up --os linux
berth view
claude mcp add --transport stdio berth -- berth mcp
berth end
```

`berth pair --url` defaults to `http://127.0.0.1:7432`. Token and URL land in
`~/.berth/config.toml` (mode 0600). `berth mcp` is stdio JSON-RPC (tools talk
to the guest, not the host desktop).

#### Two machines (headline)

The node still binds **loopback**. `cloudflared` is the public edge. Pairing is
`POST /v1/pair` with `{code}` — the token is never placed on the tunnel URL.

```sh
# parked Mac or Linux box
berth node up --tunnel cloudflare
# pairing code: ABCD-EFGH
# console: http://127.0.0.1:7432/   (open this on the parked box)
# quick tunnel; pair with https://….trycloudflare.com
# named (TUNNEL_TOKEN set): named tunnel; pair with your hostname

# laptop (or this Mac via the public URL — a phone hotspot is a valid test)
berth pair --url https://<name>.trycloudflare.com --code ABCD-EFGH
berth up --os linux
# berth view is only useful on the parked node (127.0.0.1); the tunnel does not
# publish noVNC. Agents use berth mcp / the tunneled session WS.
claude mcp add --transport stdio berth -- berth mcp
berth end
```

A tunneled browser can load the console and type the stderr pairing code. That
pair still does not revoke the CLI unless you pass `--revoke-others` or open
`/doctor` → **Revoke other clients**.

Install `cloudflared` first (`brew install cloudflared` on macOS; on Linux,
`echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main' | sudo tee /etc/apt/sources.list.d/cloudflared.list && sudo apt-get update && sudo apt-get install cloudflared`).

## Security

- **Host desktop is never driven.** Isolation is the product. No
  `--network=host`, no `/tmp/.X11-unix`, no host `DISPLAY`.
- Node HTTP binds **127.0.0.1**. Remote access is Cloudflare Tunnel + pairing
  token, not a bind-all listener.
- Pairing code on `GET /v1/pairing` is loopback-operator only. Ignore
  `X-Forwarded-*`. `httpHostHeader: 127.0.0.1` is unsupported (see above).
- Guest egress is **default-deny**. Default allowlist: `github.com`,
  `pypi.org`, `registry.npmjs.org`. **Empty allowlist = no outbound** (not
  "allow all"). Set in `~/.berth/config.toml`:

  ```toml
  allowlist = "github.com,pypi.org,registry.npmjs.org"
  # allowlist = ""   # deny all outbound
  ```

  If the key is omitted, the node uses `BERTH_ALLOWLIST` (unset = default,
  empty = deny-all). A present key (including `""`) is sent on the lease and
  wins over the node env.
- `vcpu` / `mem_gib` of `0` is rejected (not unlimited).

`os=windows`, `os=macos`, and `class=mesh` return a clear error. They are not
implemented in v0.1.

## Why this exists

Most agents can think. Almost none of them have a Windows box, a Mac, or even a
Linux desktop they are allowed to touch. Grok Bot ships a cloud Linux PC.
Devin has Linux plus Windows, and macOS only if you bring a machine. Claude's
computer-use API is generally available — and still expects *you* to provide
the computer. That gap is the product.

berth is an open **computer-session layer**: a protocol, a node you run on
hardware you own, and later a mesh that matches an agent to a berth. It is
not another agent. It is not another sandbox SDK. It is the place an agent
sits down.

Read the argument: [docs/THESIS.md](docs/THESIS.md).
Read the human console: [docs/CONSOLE.md](docs/CONSOLE.md).
Read the market: [research/LANDSCAPE.md](research/LANDSCAPE.md).
Read the legal constraints: [docs/LEGAL.md](docs/LEGAL.md).
Read the meter: [docs/ECONOMICS.md](docs/ECONOMICS.md).
Read the numbers: [docs/MATH.md](docs/MATH.md).
Read tenancy: [docs/TENANCY.md](docs/TENANCY.md).
Read the image: [docs/IMAGE.md](docs/IMAGE.md).
Read the protocol: [spec/computer-session.md](spec/computer-session.md).
Read the review: [docs/REVIEW.md](docs/REVIEW.md).
Read the MVP plan: [docs/MVP.md](docs/MVP.md).

## What we will not build (yet)

- Windows or macOS guests, W365, public Mac, Cua Driver
- `class=mesh`, earning, cash-out, wallets
- Host-desktop sharing (an untrusted agent on your logged-in session is RCE
  with a screenshot loop)
- Five-minute P2P macOS rentals (Apple's SLA forbids time-sharing)
- A speculative ticker before a second of gas settles

## v0.1.0 tag checklist

- [ ] `berth doctor` green on macOS + Docker Desktop (and Linux+Docker if present)
- [ ] `berth node up` + open `http://127.0.0.1:7432/` + pair this browser
- [ ] Wizard What → Where → Review shows quoted USD (not charged)
- [ ] `berth pair` (CLI) still works after pairing the console (no `--revoke-others`)
- [ ] Park / unpark; unpark while live is blocked (409)
- [ ] Force disconnect = no income; `berth end` / `berth_end` graceful
- [ ] `berth node up` + `berth up --os linux` on one machine
- [ ] Same path across two machines via `--tunnel cloudflare`
- [ ] Claude Code screenshot + click (e2e screenshot)
- [ ] `/workspace` persists across `berth end` + a second `berth up`
- [ ] Host desktop / cursor untouched
- [ ] Quote printed (USD), not collected
- [ ] `os=windows|macos` and `class=mesh` return a clear error
- [ ] Empty egress allowlist denies outbound
- [ ] README can be followed without reading MATH.md

## Name

Working title. A berth is where a ship is parked and made fast. Alternatives
that capture the same shape: *yard*, *slip*, *chassis*, *outpost* (taken by
Devin). Rename is cheap this early.

## License

MIT. See [LICENSE](LICENSE).
