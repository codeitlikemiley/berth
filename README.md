# berth

**A berth is a parked computer an agent can lease.**

Most agents can think. Almost none of them have a Windows box, a Mac, or even a
Linux desktop they are allowed to touch. Grok Bot ships a cloud Linux PC.
Devin has Linux plus Windows, and macOS only if you bring a machine. Claude's
computer-use API is generally available — and still expects *you* to provide
the computer. That gap is the product.

berth is an open **computer-session layer**: a protocol, a node you run on
hardware you own, and later a mesh that matches an agent to a berth. It is
not another agent. It is not another sandbox SDK. It is the place an agent
sits down.

```
agent (Claude, Grok, Codex, custom)
        │  MCP / Anthropic computer_toolset / OpenAI computer / Gemini computer_use
        ▼
     berth control plane   (lease, match, record, settle credits)
        │
   ┌────┼─────────────────────────┐
   ▼    ▼                         ▼
private     licensed cloud          mesh (supply)
outpost     W365 for Agents         Linux shared / isolated
your NUC    AWS / MacStadium Mac    wired mini PCs, not laptops
Mac mini    Hetzner / Cua / E2B     public Mac = our §3 minis only
```

## Why this exists

Three things became true at once in 2026:

1. **The models can drive a GUI.** Claude `computer_toolset_20260801` (GA),
   OpenAI Responses `computer` tool, Gemini Computer Use on desktop. All of
   them speak screenshot + click + type. None of them include the OS.
2. **The drivers exist.** [Cua Driver](https://cua.ai/cua-driver) is MIT,
   background-input on macOS / Windows / Linux, MCP and CLI. Lume virtualizes
   macOS on Apple silicon. You should not rewrite that.
3. **The computers are missing.** Grok Bot's machine is Linux. Devin Cloud has
   no first-party macOS VM. Cua's cloud macOS is a partnership waitlist. There
   is no open, agent-agnostic way to say `berth up --os windows` and hand that
   session to Claude Code.

The naive idea — "Airbnb your laptop to random agents" — dies on licensing
and security. The idea that survives is three inventory classes on one socket:

- **Private outpost (the wedge).** Park a NUC or Mac mini. Your agents —
  Claude Code, Codex, Grok Build — sit there through a tunnel. Devin
  Outposts, opened. Laptops stay here; they do not join the public mesh.
- **Licensed cloud.** Windows via [Windows 365 for Agents](https://www.microsoft.com/en-us/windows-365/agents)
  (~$0.40/VM/hr, check-out / check-in). macOS via AWS / MacStadium
  (24-hour minimum). Linux via Hetzner / Cua / E2B.
- **Mesh (Linux-first supply).** Wired, no-sleep mini PCs parking isolated
  guests. **Shared** (many sessions, one host) is the cheap SKU. Public
  Mac is a company-run 24-hour exclusive §3 fleet, not your MacBook.

Read the argument: [docs/THESIS.md](docs/THESIS.md).  
Read the market: [research/LANDSCAPE.md](research/LANDSCAPE.md).  
Read the legal constraints: [docs/LEGAL.md](docs/LEGAL.md).  
Read the meter: [docs/ECONOMICS.md](docs/ECONOMICS.md).  
Read the numbers: [docs/MATH.md](docs/MATH.md).  
Read tenancy: [docs/TENANCY.md](docs/TENANCY.md).  
Read the image: [docs/IMAGE.md](docs/IMAGE.md).  
Read the protocol: [spec/computer-session.md](spec/computer-session.md).  
Read the review: [docs/REVIEW.md](docs/REVIEW.md).

## What we will not build

- A speculative ticker before a second of gas settles. Gas is a meter
  (vCPU + RAM + disk × OS × density). Spend it on rent or cash it out.
  A token, if any, wraps those credits 1:1. See [docs/ECONOMICS.md](docs/ECONOMICS.md).
- A new computer-use driver. We wrap [Cua Driver](https://github.com/trycua/cua)
  (and AT-SPI / UIA where Cua is the wrong tool).
- Host-desktop sharing. An untrusted agent on your logged-in session is RCE
  with a screenshot loop. Isolation is the product.
- Five-minute P2P macOS rentals. Apple's SLA forbids time-sharing and
  requires 24-hour exclusive leases for developer services. We will not
  pretend otherwise.
- Windows Home laptops rented to strangers. That license is not a service
  license. Windows supply is W365 / AVD / SPLA / evaluation-for-dev, not OEM.
- Laptops on the public mesh. Lid close is not an SLA. Mini PC on Ethernet,
  or stay a private outpost.

## Status

Private Linux outpost on one machine (node + CLI). No tunnel, no MCP yet.
The guest is Linux in Docker (linux/arm64 on Apple Silicon). The host
desktop is never driven.

```sh
# this Mac, with Docker Desktop or OrbStack
berth node up
# prints a pairing code; listens on 127.0.0.1:7432

# another terminal, same machine — copy the code from node up
berth pair --code ABCD-EFGH
berth up --os linux
berth view          # prints the noVNC URL; does not open a browser
berth end
```

`berth pair --url` defaults to `http://127.0.0.1:7432`. Token and URL land
in `~/.berth/config.toml`. MCP (`berth mcp`) is the next slice.

## Name

Working title. A berth is where a ship is parked and made fast. Alternatives
that capture the same shape: *yard*, *slip*, *chassis*, *outpost* (taken by
Devin). Rename is cheap this early.

## License

MIT. See [LICENSE](LICENSE).
