# berth MVP — executable plan

**Goal.** Claude Code (or Codex / Grok Build) on a laptop drives an isolated Linux desktop on another machine you own, through one CLI + MCP. That is Devin Outposts, opened. It is the whole MVP.

**Done when** a stranger can:

```
# on the parked box (Mac with Docker, or any Linux with Docker)
berth node up --name home-nuc

# on the laptop
berth pair --node home-nuc          # prints a join code / tailscale/cloudflare
berth up --node home-nuc --os linux
claude mcp add --transport stdio berth -- berth mcp
```

…then ask Claude: *open Chromium, go to example.com, screenshot*. They see the click happen in `berth view`. `/workspace` still has the files after `berth end` + a second `berth up`.

The **human console is node-local** (`http://127.0.0.1:7432/` from the same process). A hosted control plane remains post-MVP item 6.

No mesh. No token. No Windows. No public Mac. No hosted SaaS control plane.

## Where you develop: macOS is fine

You do **not** need a Linux workstation. The *guest* is Linux (a Docker
container with Xvfb). The *node* and *CLI* are native binaries. On a Mac:

| Piece | Runs on |
| --- | --- |
| `berth` CLI, MCP, Claude Code | macOS (this machine) |
| `berth node` daemon | macOS, talking to **Docker Desktop** |
| Agent desktop (XFCE, Chromium, xdotool) | **Linux inside Docker** |
| Your host cursor / Finder | never touched |

Same trick as local Kubernetes: Darwin on the outside, Linux VM in
Docker Desktop, containers inside that.

**You need:** Docker Desktop (or OrbStack — often faster on Apple
Silicon), a recent Rust (`rustup`), and ~10 GB disk for images.

**Apple Silicon:** build `linux/arm64` (Debian + Chromium both have
arm64). Do not force `linux/amd64` under qemu unless you like 5× slower
desktops.

**One-machine loop (weeks 1–2):** node + CLI + container all on this
Mac. That is enough to get Claude Code clicking.

**Two-machine loop (PR7):** still no Linux PC required. Options:

1. This Mac (CLI) + another Mac with Docker (node), or
2. This Mac + a cheap cloud Linux VPS (Hetzner ~$5/mo) running the node, or
3. Phone-hotspot test: node on the Mac, CLI on the same Mac via
   cloudflared so traffic leaves the LAN.

A dedicated Linux NUC is nicer for a *parked* outpost later. It is not
a build dependency.

**Later (post-MVP) macOS *guests* (Lume)** actually want this Mac. Docker
cannot legally or practically be the macOS desktop. Private Lume is a
reason to stay on Apple silicon, not a reason to switch to Linux now.

---

## 1. What MVP is / is not

| In | Out (explicit) |
| --- | --- |
| Linux only | Windows, macOS, W365, AWS Mac |
| `class=private` | `class=mesh`, earning, cash-out, wallets |
| One node, one or few sessions | Shared overcommit marketplace |
| Container session (Kasm-shaped) | Full VM / QEMU |
| Local ledger (sqlite, quotes printed, not billed) | Stripe, credits settlement |
| MCP + Anthropic computer_toolset adapter | Gemini adapter (easy later) |
| `/workspace` volume | S3 mount (stub the lease field, do not ship rclone yet) |
| Default-deny egress allowlist | Unrestricted internet |
| Cloudflare Tunnel **or** Tailscale (pick one, ship both later) | Custom relay network |
| xdotool + ImageMagick inside the guest | Cua Driver (add when the loop works) |
| Human viewer (noVNC or a PNG stream) | Session recording as a product |
| Node-local console (`GET /`) | Hosted SaaS dashboard / accounts |

Post-MVP (do not start until the Done-when above is true):

1. Cua Driver in the image  
2. OpenAI `computer` adapter  
3. rclone `/mnt/s3`  
4. Lume private macOS  
5. **W365 for Agents provider** (`crates/berth-provider-w365`) — wrap
   [microsoft/windows-365-for-agents](https://github.com/microsoft/windows-365-for-agents)
   MCP (acquire → Ready → tools → release). Not in v0.1: needs an Agent 365
   tenant. See [WINDOWS365.md](WINDOWS365.md).  
6. Hosted control plane + credits (the node-local console is v0.1; this is the hosted plane)  
7. Linux mesh (wired mini PCs)  
8. Mac §3 fleet  

---

## 2. Architecture (MVP)

No central cloud. The **node is the control plane** for private leases.

```
Laptop (macOS)                 Tunnel                    Parked box (macOS or Linux + Docker)
┌─────────────────────┐        (cloudflared)        ┌──────────────────────────┐
│ berth CLI           │  HTTPS/WSS ───────────────► │ berth-node (daemon)      │
│ berth mcp (stdio)   │                             │  GET  /  console         │
│ Claude Code / Codex │                             │  POST /v1/leases         │
│ operator browser    │                             │  WS  /v1/sessions/:id    │
└─────────────────────┘                             │  docker run berthos      │
                                                    │    Xvfb + XFCE + Chromium│
                                                    │    xdotool + import      │
                                                    │    /workspace volume     │
                                                    │  noVNC :6080 (viewer)    │
                                                    └──────────────────────────┘
```

- **Protocol:** JSON over WebSocket as in `spec/computer-session.md` (actions + frames). HTTP for lease CRUD. Operator list/park/force is the node console, not the agent socket.
- **Auth:** node issues a pairing token (`berth pair`). Token in `Authorization: Bearer`. Default pair does not revoke other bearers.
- **Isolation:** one Docker container per session. Not the host desktop. Ever.
- **Driver v0:** `xdotool` + `import -window root png:-` on `DISPLAY=:99`. Coordinates = screenshot pixels. Swap to Cua Driver later without changing the protocol.

### Why not Cua Driver on day one

Cua Driver is the right long-term Linux/macOS/Windows driver. For a headless Xvfb container, xdotool is 50 lines and matches OpenAI's own reference. Blocking MVP on a third-party daemon is how this slips a month. Image keeps a `/usr/local/bin/driver` shim so we can switch.

---

## 3. Tech stack (locked)

| Piece | Choice | Why |
| --- | --- | --- |
| Language | **Rust** (edition 2024 / workspace) | Matches this OSS org; one binary for CLI+node |
| Runtime | tokio | WS + HTTP + docker |
| HTTP | axum | lease API |
| WS | tokio-tungstenite / axum ws | action channel |
| Docker | bollard | start/stop/exec in the guest |
| MCP | `rmcp` or a thin JSON-RPC stdio (keep it small) | Claude Code / Codex |
| Image | Debian bookworm + XFCE + Xvfb + x11vnc + noVNC | E2B/Anthropic-shaped |
| Tunnel | **cloudflared** named tunnel (token on the node) | NAT without inbound ports |
| DB | sqlite (node-local) via sqlx | leases, pairing, workspace paths |
| Tests | cargo test + one `tests/e2e_docker.rs` ignored unless `BERTH_E2E=1` | |

Repo layout to create:

```
berth/
  Cargo.toml                  (workspace)
  crates/
    berth-protocol/           types, action enum, serde, coord scaling
    berth-node/               axum daemon, docker, sqlite
    berth-cli/                berth, berth node, berth mcp, berth view
    berth-mcp/                stdio MCP server (used by cli)
    berth-adapter-anthropic/  toolset_20260801 ⇄ protocol
  images/linux-xfce/          Dockerfile + entrypoint
  docs/                       (already exists)
  spec/computer-session.md    (already exists)
  tests/e2e/
```

Single user-facing binary: `berth` (cli crate, features `node` always on for MVP).

---

## 4. Build sequence (PRs you can merge)

Each PR is independently reviewable. Do not start N+1 until N's acceptance box is green.

### PR0 — repo skeleton (½ day)

**Files:** `Cargo.toml`, `crates/berth-cli` hello, `crates/berth-protocol` empty types, `.github/workflows/ci.yml` (`cargo fmt`, `clippy -D warnings`, `test`), `rust-toolchain.toml`.

**Accept:** `cargo test` and `berth --help` print `up | node | mcp | pair | end | view`.

### PR1 — protocol crate (1 day)

**Files:** `crates/berth-protocol`

Implement, with tests:

- `Action` / `ActionBatch` / `Ack` / `Frame` (png bytes, width, height)
- `LeaseRequest` / `Lease` / `Quote` (subset of the spec: `os=linux`, `class=private`, `density=isolated|shared`, `resources`, `term=on_demand`, `min_seconds`)
- Coordinate helper: if frame was scaled, map click xy back to guest pixels
- Reject `os != linux` and `class=mesh` in a `validate_mvp()` that the node calls

**Accept:** table-driven serde tests from the JSON in `spec/computer-session.md`. Golden fixtures in `crates/berth-protocol/tests/fixtures/`.

### PR2 — berthOS Linux image (1–2 days)

**Files:** `images/linux-xfce/Dockerfile`, `entrypoint.sh`, `scripts/action.sh`

Image contents (MVP, not the full IMAGE.md catalog):

- debian:bookworm-slim
- xvfb, xfce4 (or openbox + tint2 if xfce is too fat — prefer **openbox + tint2** for size), xdotool, imagemagick, x11vnc, novnc, websockify
- chromium
- git, python3, curl, jq
- user `berth`, `WORKDIR /workspace`
- `entrypoint.sh` starts Xvfb `:99` 1280x800x24, window manager, x11vnc `:5900`, noVNC `:6080`
- `scripts/action.sh` implements `screenshot|click|type|key|scroll|move|wait` on `DISPLAY=:99`

Build: `docker build -t berthos-linux-xfce:dev images/linux-xfce`

**Accept:**

```
docker run --rm -p 6080:6080 berthos-linux-xfce:dev
# open http://localhost:6080 — desktop visible
docker exec <id> /usr/local/bin/action.sh screenshot > /tmp/s.png
file /tmp/s.png   # PNG, ~1280x800
```

Keep image under ~1.5 GB uncompressed if possible.

### PR3 — executor (docker + actions) (2 days)

**Files:** `crates/berth-node/src/docker.rs`, `executor.rs`

- `Session::start(lease)` → `docker create/start` with:
  - `--memory`, `--cpus` from `resources`
  - volume `berth-ws-<id>:/workspace`
  - env `DISPLAY=:99`
  - cap drop; no `--network=host`
  - network: internal bridge + egress proxy later; **MVP: default docker network but iptables allowlist via a tiny egress sidecar OR `--network=none` plus a allowlist proxy**. Simplest MVP that is still safe: **`--dns` + nftable in entrypoint** reading `BERTH_ALLOWLIST` (comma domains). If that slips, ship `--network=none` and only local viewer; add allowlist in PR3b.
- `Session::exec(batch)` runs `action.sh` in order, stops on first failure, returns `Ack`
- `Session::screenshot()` → `Frame`
- `Session::stop()` → container remove, **keep volume**

**Accept:** rust integration test (ignored without docker): start session, type into a terminal, screenshot non-empty PNG, stop, start again, `/workspace` file still there.

### PR4 — node HTTP/WS + sqlite (2 days)

**Files:** `crates/berth-node`

- `berth node up --bind 127.0.0.1:7432 --pair-file ~/.berth/node.token`
- sqlite `~/.berth/node.db`: `leases`, `sessions`, `workspaces`, `pair_tokens`
- Routes:
  - `POST /v1/pair` (first-run prints code; laptop `berth pair`)
  - `POST /v1/leases` → start container, return `lease_id`, `session_id`, `ws_url`, `viewer_url`, `quote`
  - `DELETE /v1/leases/:id` → stop, billable_seconds = max(min, elapsed)
  - `GET /v1/leases/:id`
  - `WS /v1/sessions/:id` → ActionBatch in, Frame/Ack out
- Quote: use MATH.md seed `p_cpu/p_mem/p_disk`, `term=on_demand`, `min_seconds=60` for container. Print USD. **Do not charge.** Store `quote` JSON on the lease for later settlement.
- Health: `GET /healthz`

**Accept:** `curl` lease → ws click → ack + frame. `DELETE` lease. sqlite row remains with `seconds`.

### PR5 — CLI (1 day)

**Files:** `crates/berth-cli`

```
berth node up [--name] [--tunnel cloudflare]
berth pair --url https://… --code XXXX
berth up --node <name> [--os linux]
berth mcp                      # stdio, uses last session or BERTH_SESSION
berth view                     # open viewer URL
berth end
berth status
```

Config: `~/.berth/config.toml` (`nodes.home-nuc.url`, `token`).

**Accept:** documented happy path on one machine (node + cli, no tunnel yet).

### PR6 — MCP + Anthropic adapter (2 days)

**Files:** `crates/berth-mcp`, `crates/berth-adapter-anthropic`

MCP tools (minimum):

- `berth_lease` `{os, seconds?}`
- `berth_screenshot`
- `berth_click` `{x,y,button?}`
- `berth_type` `{text}`
- `berth_key` `{keys}`
- `berth_scroll` `{x,y,dy}`
- `berth_end`

Map 1:1 to protocol actions. `berth_lease` calls the node.

Anthropic adapter: function that takes `tool_use` blocks with `toolset_name=computer` and returns `tool_result` list (image for screenshot, `OK` otherwise). Used later by a thin `berth loop` if we run without MCP. For MVP, MCP is the product surface; keep the adapter as a library with unit tests so a future `berth loop --anthropic` is a day.

**Accept:**

```
berth node up
berth up --os linux
claude mcp add --transport stdio berth -- berth mcp
# in Claude Code: "take a screenshot of the desktop"
# screenshot comes back
```

Record a 30-second screencast. That is the launch artifact.

### PR7 — remote outpost tunnel (1–2 days)

**Files:** node `--tunnel cloudflare`, CLI `berth pair` over HTTPS.

- Node: if `cloudflared` present and `TUNNEL_TOKEN` set, run it as a child; else print install instructions.
- Laptop talks to `https://<name>.trycloudflare.com` (quick tunnel) or named tunnel.
- Pairing still required. Token never on the URL.

**Accept:** two machines. Laptop not on the same LAN (phone hotspot is a valid test). Same Claude Code path as PR6.

### PR8 — harden + docs (1 day)

- Default egress allowlist: `github.com`, `pypi.org`, `registry.npmjs.org`. **Empty allowlist = no outbound** (not "allow all"). Config `allowlist = ""` or `BERTH_ALLOWLIST=`.
- Resource caps enforced (`vcpu`/`mem_gib` of `0` is rejected, not unlimited).
- `berth doctor` (Docker daemon, guest image, `BERTH_HOME` writable, paired token warn, optional cloudflared warn). Exit 0 only if required checks pass.
- README rewritten around the two-machine / macOS+Docker path (keep research links). Security note: host desktop is never driven.
- `os=windows|macos` and `class=mesh` return clear errors. Do not implement those guests.
- Tag-ready v0.1.0 checklist in README.

**Accept:** `berth doctor` green on **macOS + Docker Desktop** (and
Linux+Docker if present).

---

## 5. Tests (required, not optional)

| Level | What |
| --- | --- |
| Unit | protocol serde, coord scale, lease validate, anthropic mapping |
| Docker e2e | `BERTH_E2E=1 cargo test -p berth-node -- --ignored` start/click/persist/stop |
| Manual gate | PR6 Claude Code path; PR7 two-machine path |

No mock MCP. If Claude Code is unavailable in CI, script a raw MCP stdio handshake (initialize → tools/list → tools/call screenshot).

---

## 6. Risks and how the plan absorbs them

| Risk | Mitigation |
| --- | --- |
| XFCE image too fat / slow | Fall back to openbox+tint2 (Anthropic demo). Decision in PR2, not later. |
| Wayland on host | Guest is Xvfb. Host compositor irrelevant. |
| Docker on macOS | Node is Darwin; guest is Linux in Docker Desktop/OrbStack. First-class. Build `linux/arm64` on Apple Silicon. |
| cloudflared ToS / abuse | Private class, pairing token, allowlist. Named tunnel not anonymous quick tunnel for anything public. |
| Cua Driver mismatch later | `action.sh` is the seam. PR post-MVP replaces the script with cua-driver MCP inside the guest. |
| Scope creep (mesh, token, Windows) | This file. If a PR does not serve the Done-when, it waits. |

---

## 7. Calendar (one engineer)

| Week | PRs | Exit |
| --- | --- | --- |
| 1 | 0, 1, 2 | Image boots, protocol types compile |
| 2 | 3, 4, 5 | One-machine `berth up` + viewer |
| 3 | 6, 7 | Claude Code remote outpost |
| 4 | 8 + buffer | Doctor, allowlist, README, ship tag `v0.1.0` |

Two engineers: split PR2 (image) // PR1+3 (protocol+executor), then join at PR4.

---

## 8. v0.1.0 tag checklist

- [ ] `berth doctor` green on macOS + Docker Desktop (and Linux+Docker if present)
- [ ] `berth node up` + `berth up --os linux` on one machine
- [ ] Same path across two machines via tunnel
- [ ] Claude Code screenshot + click (e2e screenshot)
- [ ] `/workspace` persists
- [ ] Host X session / cursor untouched
- [ ] Quote printed (USD), not collected
- [ ] `os=windows|macos` and `class=mesh` return a clear error
- [ ] Empty egress allowlist denies outbound
- [ ] README can be followed without reading MATH.md

---

## 9. After v0.1.0 (do not schedule until tagged)

Ordered, still:

1. Cua Driver in image (macOS/Windows private become possible)  
2. OpenAI adapter  
3. `/mnt/s3` rclone  
4. Lume private macOS  
5. W365 provider (`isolated` + `pooled`, list $0.40/hr)  
6. Hosted control plane + credits (MATH.md) — not the node-local console  
7. Linux mesh  
8. Mac §3  

---

## Key decisions

1. **MVP = private Linux outpost + MCP.** Mesh/token/Windows are later products.
2. **Node is the control plane.** No hosted API in v0.1. The human console is node-local (`GET /`).
3. **Rust workspace, one `berth` binary.**
4. **xdotool in Xvfb for v0;** Cua Driver is a driver swap, not a rewrite.
5. **Container per session, volume per workspace.** Never the host desktop.
6. **cloudflared for NAT.** Pairing token for auth.
7. **Print quotes, do not bill.** Ledger schema exists so settlement can attach.
8. **Eight PRs, four weeks, one e2e gate: Claude Code clicks a remote desktop.**
