# berth

**A berth is a parked computer an agent can lease.**

v0.1 is a **private Linux outpost**: Claude Code (or Codex / Grok Build) on a
laptop drives an isolated Linux desktop in Docker on a machine you own. The
guest is Linux (linux/arm64 on Apple Silicon). **The host desktop is never
driven.**

Windows, macOS guests, and `class=mesh` are not in v0.1.

```
laptop (CLI / Claude Code / MCP)
        │  HTTPS (cloudflared) or loopback
        ▼
parked box (macOS + Docker Desktop/OrbStack, or Linux + Docker)
        │  berth-node on 127.0.0.1:7432
        ▼
Linux guest (Xvfb + openbox + Chromium)   ← not Finder, not your cursor
```

## Quick start

Develop on macOS with Docker Desktop or OrbStack. You do not need a Linux
workstation.

```sh
# once
docker build -t berthos-linux-xfce:dev images/linux-xfce
cargo install --path crates/berth-cli
berth doctor
```

`berth doctor` must be green (Docker daemon, guest image, `~/.berth` writable).
cloudflared is a warning unless you want `--tunnel cloudflare`. Unpaired is a
warning until `berth pair`.

### Two machines (headline)

The node still binds **loopback**. `cloudflared` is the public edge. Pairing is
`POST /v1/pair` with `{code}` — the token is never placed on the tunnel URL.

```sh
# parked Mac or Linux box
berth node up --tunnel cloudflare
# pairing code: ABCD-EFGH
# quick tunnel; pair with https://….trycloudflare.com
# named (TUNNEL_TOKEN set): named tunnel; pair with your hostname

# laptop (or this Mac via the public URL — a phone hotspot is a valid test)
berth pair --url https://<name>.trycloudflare.com --code ABCD-EFGH
berth up --os linux
berth view          # prints the noVNC URL; does not open a browser
claude mcp add --transport stdio berth -- berth mcp
berth end
```

Install `cloudflared` first (`brew install cloudflared` on macOS; on Linux,
`echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main' | sudo tee /etc/apt/sources.list.d/cloudflared.list && sudo apt-get update && sudo apt-get install cloudflared`).

noVNC (`berth view`) is node-local on the mapped guest port; the tunnel does
not publish it. Agents use the tunneled session WS / `berth mcp`.

### One machine (loopback)

Same Mac, no tunnel. Enough to get Claude Code clicking.

```sh
berth node up
# prints a pairing code; listens on 127.0.0.1:7432

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

## Security

- **Host desktop is never driven.** Isolation is the product. No
  `--network=host`, no `/tmp/.X11-unix`, no host `DISPLAY`.
- Node HTTP binds **127.0.0.1**. Remote access is Cloudflare Tunnel + pairing
  token, not a bind-all listener.
- Guest egress is **default-deny**. Default allowlist: `github.com`,
  `pypi.org`, `registry.npmjs.org`. **Empty allowlist = no outbound** (not
  "allow all"). Set in `~/.berth/config.toml`:

  ```toml
  allowlist = "github.com,pypi.org,registry.npmjs.org"
  # allowlist = ""   # deny all outbound
  ```

  Or `BERTH_ALLOWLIST` on the node process (unset = default, empty = deny-all).
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
