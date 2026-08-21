# Review (2026-08-21)

Verdict: **keep the idea, narrow it.** The gap is real. The first draft tried to
be Devin Outposts, Cua Cloud, Windows 365, and Helium in one README. That is
how it dies. What survives is a socket with three inventory sources, shipped
in order, with a meter that is honest about OS and density.

This file is the adversarial pass. Edits landed in the other docs the same day.

## What is actually true

1. Models will click. They will not host Windows or Mac. That is still the
   hole. Claude GA, OpenAI `computer`, Gemini desktop, Grok Bot (Linux only),
   Devin (macOS via Outposts only).
2. Cua Driver + Lume already are the hands and the Apple-silicon hypervisor.
   Rewriting them is a waste.
3. Apple §3 (24h exclusive Apple hardware, developer services, notice) and
   Microsoft's agent Cloud PC SKU are the legal rails. A consumer laptop mesh
   is not.
4. Occupancy is the right unit, not clicks. Shared silicon is the cheap SKU.
   Mac cannot be that SKU.

## What was wrong in the draft

### The wedge claimed too much

`berth up --os linux` on localhost is Anthropic's Docker demo plus MCP. It is
not "more than Grok Bot." Grok Bot is a persistent *cloud* PC. The unique
object is:

> Your other machine (NUC / Mac mini / parked box), reached by a tunnel, is
> Claude Code / Codex / Grok Build's computer — and later a Windows 365
> Cloud PC is, through the same command.

That is Devin Outposts, opened. Local Linux is bootstrap, not the headline.

### Windows 365 is not `density=shared`

Check-out / check-in is **pooling**: one tenant at a time on a warm guest,
then revert. Shared is **concurrent overcommit**: many sessions on one host
at once. Different price, different noise, different license. The draft
collapsed them. Split: `density` slices silicon; `pooled` is scheduling.

### `os_mult` 1 / 1.5 / 2 is a mesh bid default, not physics

Hetzner Linux ~$0.05/hr vs W365 Agents $0.40/hr is ~8×, not 1.5×. AWS Mac
~$1.20/hr × 24h floor is a *day*, not 2× a Linux minute. Applying 1.5× on
top of W365's already-Windows price would double-charge. Multipliers apply
to **mesh host bids**. Cloud providers quote absolute USD.

### Mac 2× without the floor is a lie to hosts

A public Mac is `exclusive` × `os_mult=2` × **min 86400 seconds**. A
15-minute iOS test still buys a day. Consumer MacBooks do not join that
market. Public Mac supply is a company-operated §3 fleet (we notify Apple)
or AWS/MacStadium resale.

### Residential "park your laptop" will get the company killed

Lid close, CGNAT, sleep, Wi-Fi, and a browser equal a fraud appliance.
Cloudflare and Stripe will notice. Public mesh hosts are wired, no-sleep
mini PCs, default-deny egress. Laptops are private outposts.

### Cua is a complement, not a scoreboard win

If Cua ships `cua mcp lease` tomorrow, a "one API for sandboxes" pitch is
theirs. Our remaining surface is: agent-agnostic **outpost** (BYO hardware
for Claude/Codex/Grok, not only Devin) + licensed broker + Linux mesh.
Stay on that. Do not race their cloud.

## Decisions locked

1. **One protocol, three inventory classes:** private outpost, licensed
   cloud, mesh. Mesh is supply, not the company.
2. **Ship order:** local Linux → remote private outpost (the real wedge) →
   W365 Windows → AWS/MacStadium Mac → Linux mesh (shared + isolated) →
   company-run Mac §3 if Apple answers.
3. **`density`:** `shared` | `isolated` | `exclusive`. **`pooled`:** warm
   check-out, orthogonal. W365 = isolated + pooled. Cheap Linux = shared.
   Public Mac = exclusive, not pooled below 24h.
4. **Meter:** seconds × (vCPU + RAM + disk) × mesh `os_mult` × `density_mult`.
   Cloud SKUs ignore `os_mult` and pass through list price + our cut.
5. **Public Mac:** we (or a notified lessor) own the minis. Not your
   MacBook.
6. **Credits** USD-pegged. Spend or cash out. Token only as a 1:1 wrapper
   after a second of credit actually settles.
7. **Public mesh egress defaults deny.** Unrestricted internet is a
   paid, KYC'd buyer flag. Custom ISOs are not a day-one mesh feature.
8. **Wrap Cua Driver.** Do not fork it.
9. **Numbers from invoices, not vibes.** See [MATH.md](MATH.md). Shared
   density_mult **0.30**. Windows consumer mesh **dropped** (multi-session
   is AVD-only). Mac is a **day price**. Billing is per-second after a
   minimum, with monthly/annual caps — same shape as Hetzner + E2B + Apple.
10. **Golden image + workspace volume + optional S3 mount.** See
    [IMAGE.md](IMAGE.md), [TENANCY.md](TENANCY.md). Account to activate,
    wallet optional for cash-out.

## Remaining risks (accepted)

- Apple ignores §3 notice from a small lessor. Then public Mac is AWS
  resale only. Private Lume still works.
- Microsoft W365-for-Agents API stays Copilot-Studio-shaped and is painful
  to wrap. Then AVD per-user is the Windows path, slower.
- Abuse on Linux shared still happens behind an allowlist (compromised
  GitHub Pages, etc.). Recording + holdback + slash are mitigation, not
  proof.
- Cua or Devin open the socket themselves. Then berth is a mesh +
  outpost implementation of their API. That is still a company if the
  mesh has inventory.

## What would make us walk away

- Host-desktop sharing "just to ship."
- A ticker, a whitepaper, and no `berth node` on a real box.
- Five-minute P2P Mac.
- Windows Home miner.
- Competing with Cua on `pip install cua`.
