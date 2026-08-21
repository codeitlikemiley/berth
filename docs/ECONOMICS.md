# Gas: how a parked computer gets paid

An agent occupying a berth burns **gas**. A host that parks a legal, isolated
guest earns that gas. Gas can be **spent** on another berth or **cashed out**.
That is the whole market.

This is not a meme coin. It is a meter, like Ethereum gas or an AWS invoice.
**Derived prices, minima, and why 1.5×/2.0× were wrong: [MATH.md](MATH.md).**
Who can share a box: [TENANCY.md](TENANCY.md). What boots: [IMAGE.md](IMAGE.md).

## What is being sold

Not "a computer." Occupancy of an attested guest:

| Dimension | Why it is in the meter |
| --- | --- |
| **vCPU** | Click loops are cheap; builds, browsers, simulators are not |
| **memory** | Chrome + Electron + Xcode eat RAM; oversubscribe it and the session hitchs |
| **storage** | Golden images, derived data, Windows pagefile |
| **OS class** | License + scarcity. Linux is cattle. Windows is a Microsoft meter. Mac is Apple hardware with a 24h exclusive floor |
| **density** | Shared pool vs dedicated VM vs whole machine |

Time is the outer loop: the host cannot sell those vCPUs to someone else for
the seconds you hold them. Actions (screenshot, click) are *not* the unit —
a thinking agent idles, and the machine is still blocked.

```
gas = seconds
    × ( w_cpu × vcpu  +  w_mem × gib_ram  +  w_disk × gib_disk )
    × os_mult
    × density_mult
    × (optional: gpu, region, ax-tree, sla)
```

Protocol cut: **10–15%** (Vast.ai keeps ~15%, RunPod ~7%). Host gets the rest.

## OS multipliers (protocol defaults)

These are *relative to Linux on the same resources* and apply to **mesh
host bids only**. Cloud providers already baked the OS into the list
price — do not multiply W365's $0.40/hr by 1.5 again.

Hosts can still bid; `os_mult` is the matcher floor so a Mac is not bought
as if it were a Hetzner VPS.

| `os` | `os_mult` (mesh) | Why |
| --- | --- | --- |
| `linux` | **1.0** (mesh) | Host-cost + margin. Ceiling is E2B ~$0.17/hr, not Hetzner $0.01. |
| `windows` | **list USD** | W365 $0.40/hr. No 1.5× on top. Consumer mesh Windows is not a SKU (Win11 multi-session is AVD-only). |
| `macos` | **$/day** | Host ~$0.59/day cost → sell **$3–8/day**. Not 2× Linux minutes. 24h legal min. |

Cloud pass-through (no `os_mult`):

| SKU | What you actually pay |
| --- | --- |
| Shared Linux mesh | ~$0.015–0.04/hr session (MATH.md) |
| Hetzner / Cua Linux | their list + our cut |
| W365 for Agents | **$0.40/VM/hr** US (+ $5/VM/month if always-on) + our cut |
| AWS Mac | ~$1.08–$1.23/hr, **24h minimum** (~$26–$30) + our cut |

A 15-minute Windows job on W365 is ~$0.10 + cut. A 15-minute *public* Mac
job is still a day. Do not tell Mac hosts they will earn "2× Linux" for
ten minutes. They earn 2× Linux *for 24 hours of exclusive hardware*.

## Density and pooling (not the same thing)

`density` slices silicon. `pooled` is scheduling: check out a warm guest,
use it, revert, check in. Windows 365 for Agents is **isolated + pooled**,
not shared. Shared is the cheap concurrent SKU. Pooling is how minutes of
Windows get cheap without noisy neighbors.

| `density` | `density_mult` | What the tenant gets | Who can sell it |
| --- | --- | --- | --- |
| **`shared`** | **0.30** | Own session on an **overcommitted** host. CPU/RAM not guaranteed. Cheapest. | Linux containers. Windows multi-session **in AVD only**. **macOS: never** |
| **`isolated`** | **1.0** | Dedicated guest VM, one tenant for the lease. Default. | Linux, private Windows, licensed Windows cloud |
| **`exclusive`** | **1.8** | The whole box. No noisy neighbor. | Required for public macOS (§3). Optional high-SLA Linux/Windows |

`pooled: true` does not change `density_mult`. It changes start time and
utilization. A pooled isolated Windows Cloud PC still bills isolated
Windows rates for the seconds checked out — that is why W365 can be $0.40
for ten minutes instead of a month.

**Shared does not mean two agents, one cursor.** One graphical session, one
controller. Apple SLA §2I already forbids multi-control of a Mac desktop;
even on Linux, two models clicking the same XFCE is a toy, not a product.

Overcommit is advertised (`cpu_overcommit`, default cap **3.0** on the
public mesh). Above that the matcher hides the node from `isolated` buyers
and labels it `shared` only. Quality is a reputation input (screenshot
RTT, steal time, premature disconnects), not a vibe.

Shared means **multiplex the host**:

```
physical box (16 vCPU, 64 GiB)
   ├── session A  (agent, 2 vCPU share, 4 GiB cap)   ─┐
   ├── session B  (agent, 2 vCPU share, 4 GiB cap)    ├─ density=shared
   ├── session C  (human watch-only viewer)          ─┘
   └── isolated VM D  (4 vCPU guaranteed)              density=isolated
```

Each agent still gets an isolated *desktop* (container or multi-session logon
or a nested VM). They share the *silicon*. That is why it is cheaper: the
host sells the same cores more than once, the way a VPS overcommits, the way
AVD puts several users on one Windows multi-session VM.

Check-out / check-in (`pooled`) is the other cheap path: a warm image, minutes
of use, revert, back in the pool. Windows 365 for Agents already does this.
The mesh should too — as pooling, not as a second meaning of `shared`.

### Why Mac is never `shared`

Apple's SLA: one controller of the graphical desktop; public lease = exclusive
control of the **hardware** for ≥24 hours. A "cheap shared Mac VM" is a
license violation, not a SKU. Public Macs are `exclusive` only, and they are
machines **we (or a notified lessor) own**, not a parked MacBook. Private
Lume VMs on *your* Mac for *your* agents are `isolated` and unpaid.

## Resource weights (starting point)

Tune from invoices, not vibes. Seed values so a quote is computable on day one:

```
w_cpu  = 1.0    per vCPU-second
w_mem  = 0.25   per GiB-RAM-second
w_disk = 0.02   per GiB-disk-second   (boot disk; extra volumes quoted aside)
```

USD worked examples are in [MATH.md](MATH.md) §4–6. Do not keep a second
relative table here — it is how the 1.5/2.0 stubs survived.

Optional add-ons, multiplied after the formula:

- `gpu` (cuda / metal / dx12)
- `region` (latency to the agent loop)
- `ax-tree` (driver can emit accessibility, not just pixels)
- `sla` (preemptible vs guaranteed; preemptible is cheaper, like Vast interruptible)

## Dual sink: spend or cash out

Hosts accrue gas on every settled second.

1. **Spend.** Pay for a berth your own agents need. Park Linux at night,
   burn the balance on a Windows 365 Cloud PC or a 24h Mac when you have
   an iOS build. This is the flywheel: cheap shared Linux in, scarce OS out.
2. **Cash out.** Stripe / bank / USDC. KYC on the off-ramp, not on
   `berth node up`. Holdback (e.g. 7 days) against fraud and mid-lease
   disconnects.

Account balance is **gas credits**, USD-pegged. A token, if it ever exists,
is a portable wrapper of those credits — redeemable 1:1, not a second
economy. Akash spent years teaching people to buy AKT before they could
run a container. Do not repeat that. Card in, gas credited, node earns,
cash out or spend.

## Quotes, not surprises

The lease response carries the quote. The agent (or its operator) sees
the burn rate before the first screenshot.

```json
{
  "quote": {
    "vcpu": 2,
    "mem_gib": 4,
    "disk_gib": 40,
    "os": "linux",
    "os_mult": 1.0,
    "density": "shared",
    "density_mult": 0.30,
    "gas_per_second": "0.000333",
    "currency": "gas",
    "usd_per_gas": "0.01",
    "min_seconds": 300,
    "max_seconds": 14400,
    "preemptible": true
  }
}
```

Matcher rules:

- `macos` + `class=mesh` ⇒ `density=exclusive`, `min_seconds>=86400`,
  node operator is a notified §3 lessor (us, not a random MacBook).
- `windows` + `density=shared` ⇒ license must be multi-session (AVD /
  W11 Enterprise multi-session), not Home OEM, not "I pooled a Pro laptop."
- `windows` + `pooled` on W365 ⇒ `density=isolated` (one Cloud PC, sequential).
- `density=shared` ⇒ isolation is still per-session. Host desktop is
  never a session.
- `cpu_overcommit` advertised; public mesh cap 3.0.
- Public mesh: wired, no-sleep, default-deny egress. Laptops are
  `class=private` only.
- `os_mult` is omitted on `class=licensed-cloud` quotes.

## Host staking (the actually-Bitcoin part)

To join the public mesh a node locks a gas bond.

- Go dark mid-lease → slash, tenant refunded.
- Lie about `os` / `density` / resources (attestation mismatch) → slash.
- Clean settlement → bond released, reputation up.

Attestation is how a 2× Mac multiplier stays honest: TPM / Secure Enclave
proof that the guest is real macOS on Apple silicon, not a Linux box with
a wallpaper. Without that, everyone claims `macos` for the 2×.

## What this is not

- Metering **clicks**. Occupancy is the scarce thing.
- Letting a parked **daily-driver desktop** earn gas. Guest only.
- A shared Mac. Illegal, and one cursor.
- Multiplying cloud list prices by `os_mult`.
- Launching a ticker before `berth up --node home-nuc` settles a second of
  credit.

The token speech is: *desktop-seconds are scarce, OS-shaped, and now
transferable.* The implementation is a meter with vCPU, RAM, disk, an OS
multiplier, and a shared SKU that makes Linux actually cheap.
