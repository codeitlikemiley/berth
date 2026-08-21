# Real numbers (not stubs)

The 1.0 / 1.5 / 2.0 OS multipliers were a guess. This file replaces them.
Sources are list prices and host costs as of August 2026. Pegs drift; the
*method* is the product.

## 1. What we are actually selling

An **agent desktop-second**: wall-clock occupancy of a graphical session
with a driver, not a SHA-256 hash, not active-CPU, not a click.

Computer-use loops spend most of the lease *waiting on the model*
(1–15 s per turn). Vercel bills active CPU; E2B bills **wall clock**.
We bill wall clock. If you only billed clicks, the host would earn
nothing while Chrome sat open and Claude thought.

That answers "is per-second impossible?" **No. E2B already meters per
second.** ([e2b.dev/pricing](https://e2b.dev/pricing): `$0.000014/s` per
vCPU, `$0.0000045/GiB/s` RAM.) The real question is the **minimum
charge** and the **reserved cap**, so a 12-second poke and a 12-month
bot are not the same SKU.

## 2. Comparables (the floor and the ceiling)

Convert everything to **USD per hour for a 2 vCPU / 4 GiB shape** where
the vendor publishes one. RAM-only extras noted.

| SKU | What it is | 2 vCPU / 4 GiB-class | Source |
| --- | --- | --- | --- |
| Hetzner CX23 | Naked shared VPS, hourly, monthly cap | **€0.0088/hr ≈ $0.010/hr** (2/4/40) | [costgoat/hetzner](https://costgoat.com/pricing/hetzner) after Jun 2026 hike |
| Mini PC host cost | Hardware+power, 100% sold | **~$0.019/hr** (see §3) | derived |
| E2B sandbox | Agent VM, wall-clock, per-second | **$0.166/hr** (2×$0.0504 + 4×$0.0162) | [e2b.dev/pricing](https://e2b.dev/pricing) |
| Cua Linux (older credit sheet) | Agent desktop | order **$0.05–$0.09/hr** | [cua.ai/pricing](https://www.cua.ai/pricing) (credits move; treat as band) |
| W365 for Agents | Windows Cloud PC, PAYG | **$0.40/hr** US (+ $5/VM/mo always-on) | [microsoft.com/windows-365/agents](https://www.microsoft.com/en-us/windows-365/agents) |
| AWS EC2 Mac | Dedicated Apple host | **~$1.08–$1.23/hr**, **24h min ≈ $26–$30** | [vantage mac1.metal](https://instances.vantage.sh/aws/ec2/mac1.metal), AWS SLA note |
| rentamac.io | Dedicated M4 mini, human remote | **$3.30/day ≈ $0.14/hr** if you use the whole day | [rentamac.io](https://rentamac.io/) |

**Do not compete with Hetzner on SSH Linux.** They sell a naked VPS in a
DC. We sell an *agent desktop* (display + driver + image + tunnel). The
right ceiling is E2B/Cua/W365, not CX23. The mesh exists to sit
**between host cost and E2B**, not below Hetzner.

Ratios at the agent-desktop layer (2/4-class, hourly, no min):

```
linux_agent   = E2B 2/4          = $0.166 / hr   →  1.00 ×
windows_agent = W365 Agents      = $0.400 / hr   →  2.41 ×
macos_agent   = AWS Mac hourly   = $1.20  / hr   →  7.23 ×
macos_job_1h  = AWS Mac 24h min  = $28.80 / 1h   → 173 × a 1h Linux job
```

So "Windows 1.5×, Mac 2×" was **wrong as a cloud ratio** and **wrong as
a task ratio**. Keep 1.5 / 2.0 only as *mesh bid floors* if we ever have
licensed Windows/Mac hosts; the broker passes through list prices.

## 3. Host cost (what a parked box must earn)

A typical wired mini PC (8c / 32 GiB, ~$400, 25 W average, $0.15/kWh,
36-month straight-line, ignore residential bandwidth):

```
depreciation = 400 / (36 × 730)     = $0.0152 / hr
electricity  = 0.025 kW × 0.15      = $0.0038 / hr
C_host (100% sold)                  ≈ $0.019  / hr
```

A Mac mini M4 (~$599, ~12 W):

```
depreciation = 599 / (36 × 730)     = $0.0228 / hr
electricity  = 0.012 × 0.15         = $0.0018 / hr
C_mac (100% sold)                   ≈ $0.025  / hr
C_mac per day                       ≈ $0.59   / day
```

AWS charges ~$30/day for the same silicon class. A notified §3 Mac mini
can sell a **day** at $3–8 and still clear 5–13× host cost. That is the
only Mac mesh math that works.

Occupancy is never 100%. Use **40% sold-through** as a planning load
(Helium/Vast hosts sit idle). Break-even per session-hour on a mini PC
selling **4 concurrent shared Linux sessions**:

```
C_host / (4 sessions × 0.40 occupancy) = 0.019 / 1.6 = $0.0119 / session-hr
× 3 (churn, fraud holdback, 12% protocol cut, support)
≈ $0.036 / session-hr  tenant price
```

$0.036/hr is **4.6× cheaper than E2B $0.166** and **3.6× Hetzner $0.010**
because the session includes the desktop image. That is the mesh wedge:
undercut E2B, do not pretend to undercut a DC VPS.

## 4. Billing terms (burst AND year)

Hetzner already solved this: **hourly with a monthly cap**. E2B solved
the burst side: **per-second of wall clock**. AWS Mac solved the legal
side: **24h minimum**. We use all three.

| `term` | Meter | Minimum | Who it is for |
| --- | --- | --- | --- |
| `on_demand` | per second, wall clock of the **session** | Linux shared **60s**; Linux isolated **300s** (VM boot); W365 **60s**; public Mac **86400s** | 5-minute tasks, CI, "try this UI" |
| `monthly` | same meter, **capped** at `0.70 × on_demand × 730h` | billed calendar month | Grok-Bot-shaped always-on |
| `annual` | cap at `0.50 × on_demand × 8760h` (paid monthly or up front) | 12-month | production fleets |

Worked 2 vCPU / 4 GiB Linux *shared* mesh at $0.036/hr on-demand:

| Use | Charge |
| --- | --- |
| 12-second poke | **60s min** → $0.00060 |
| 8-minute computer-use job | 480s → $0.0048 |
| Idle-thinking 40-minute job | 2400s → $0.024 |
| Always-on month (730h) on-demand | $26.28 |
| Same, `monthly` cap (0.70) | **$18.40** |
| Same, `annual` effective | **~$13.14/mo** |

Windows W365 on-demand 8-minute job: 60s min still, 480s × $0.40/3600 =
**$0.053** plus our cut. A month always-on W365: 730 × 0.40 = **$292**
+ $5 always-available if they pin a VM, or just PAYG if they check out.

Public Mac 8-minute job: still **one day** (~$3–8 mesh, ~$26–30 AWS).
That is not a bug. It is Apple §3. Sell Mac as `term=daily` or
`term=monthly`, never as a 5-minute SKU.

Prepaid **hour packs** (E2B Pro is 500 h / $150 = $0.30/hr included)
are a UX on top of on-demand, not a fourth meter.

## 5. Credit unit

User-facing: **USD**. Internally: integer microdollars.

```
1 credit = $0.000001
$1       = 1,000,000 credits
```

Hosts accrue credits. They **spend** them on a Windows/Mac berth or
**cash out** (Stripe / USDC, 7-day hold, KYC on the off-ramp). Buyers
who never park just buy credits with a card. No wallet required to
activate a node. A wallet is an optional cash-out rail.

Protocol cut: **12%** (between RunPod 7% and Vast ~15%).

`os_mult` on mesh bids (floor, hosts may bid above):

| os | floor | Basis |
| --- | --- | --- |
| linux | 1.00 | host-cost + margin in §3 |
| windows | **n/a on consumer mesh** | Win11 multi-session is **AVD-only** ([FAQ](https://learn.microsoft.com/en-us/azure/virtual-desktop/windows-multisession-faq)). Mesh Windows = SPLA datacenter or don't. Broker uses W365 list. |
| macos | **day price**, not hourly 2× | host $0.59/day × ~5–10 = **$3–8/day**, under AWS $30, next to rentamac $3.30 |

Density on Linux mesh (relative to isolated = 1.0):

| density | mult | Basis |
| --- | --- | --- |
| shared | **0.30** | An 8c/32G box holds ~4 isolated 2c/8G guests, or ~8 overcommitted shared 2c/4G sessions. Shared/isolated ≈ 1/3–1/2 of silicon. 0.40 was a stub. |
| isolated | 1.00 | One guest, guaranteed slice |
| exclusive | whole-box quote | Public Mac; beefy Linux. Not a multiplier on a 2/4 slice. |

Cap `cpu_overcommit` at 3.0 on the public mesh.

## 6. Formula (replace the stub)

Mesh on-demand:

```
usd = max(min_seconds, lease_seconds)
    × ( p_cpu × vcpu + p_mem × gib_ram + p_disk × gib_disk )
    × density_mult
    × (1 + protocol_cut)
```

Seed prices from §3, USD / second, Linux mesh:

```
p_cpu  = $0.0000035 / vCPU-s     # ≈ $0.0126 / vCPU-hr
p_mem  = $0.0000011 / GiB-s      # ≈ $0.0040 / GiB-hr
p_disk = $0.00000005 / GiB-s     # ≈ $0.00018 / GiB-hr  (boot disk)
```

Check 2 vCPU / 4 GiB / 40 GiB isolated Linux, 1 hour:

```
3600 × (0.0000035×2 + 0.0000011×4 + 0.00000005×40)
= 3600 × (0.000007 + 0.0000044 + 0.000002)
= 3600 × 0.0000134
= $0.0482 / hr isolated
× 0.30 shared = $0.0145 / hr shared
```

$0.048 isolated is near 3× host $0.019 (one isolated guest on a box that
could hold more) and still 3.4× below E2B $0.166. Shared $0.015 is at
host break-even under 40% sell-through with 4-pack — slightly low;
raise shared `density_mult` if occupancy is worse. **Recalibrate from
invoices after the first 1000 settled hours.** Do not invent 1.5 and 2.0
again.

Cloud:

```
usd = provider_list(seconds, shape) + protocol_cut
```

No `os_mult` on cloud. W365 is $0.40/hr because Microsoft said so.

## 7. Honesty about scale

The problem is real: agents need a computer. Microsoft is already
charging $0.40/hr for that computer. xAI bundled one into Grok Bot.
That is a large market (digital labor runtime). It is not, by itself, a
"trillion dollar protocol." The trillion-dollar story is the labor. We
are the socket the labor sits on. Price the socket like electricity,
not like a coin.
