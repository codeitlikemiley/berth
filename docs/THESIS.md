# Thesis: the computer layer for agents

## The observation

Agents got brains and then they got tools. The tool they still don't have is a
body. A computer-use model without a computer is a pianist without a piano.

Grok Bot noticed this and shipped a piano — a Linux one. Devin shipped Linux
and Windows, and macOS if you bring the instrument. Cua shipped the hands
(Driver) and a shop that rents pianos (Cloud), with Macs behind a velvet rope.
Claude, GPT, and Gemini will happily play any piano you put in front of them.
Nobody open-sourced the *room where the piano lives*.

That room is a **berth**.

## Why "bitcoin for agents" is half right

Bitcoin did not invent hashing. It invented a **scarce, attested, permissionless
settlement** of hashing. The analog here is not "we will have a coin." It is:

> Desktop-seconds are scarce, OS-shaped, and currently trapped inside closed
> clouds. A protocol that can lease, attest, and settle them lets any agent
> plug into any legal computer.

Where the analogy breaks, and must break on purpose:

| Bitcoin | Computer-use |
| --- | --- |
| Hashes are fungible | A Mac is not a Windows box is not a Linux container |
| Batch, high latency OK | Click→screenshot wants <200 ms |
| No vendor license on SHA-256 | Apple §3, Microsoft AVD, evaluation ISOs |
| Anonymous work | An agent on a desktop is a loaded gun |
| Token is the product | Token is how GPU meshes died in onboarding |

So the shape is **Airbnb inventory + USB-C protocol + a gas meter**.
Inventory first. Protocol first. The meter is derived in [MATH.md](MATH.md):
wall-clock seconds after a minimum (60s Linux shared, 300s isolated VM, 24h
public Mac), monthly/annual caps for always-on, W365 at $0.40/hr list, mesh
Linux aimed between mini-PC host cost (~$0.02/hr) and E2B (~$0.17/hr).
The 1.5×/2.0× stubs are dead. Hosts spend credits or cash them out.

Vast.ai already proved people will park GPUs. Akash already proved a
decentralized *container* market can exist. Neither one will grow a GUI
session layer by accident: the unit of work is different. We do not build on
their tokens. We steal their *host-agent* idea and apply it to isolated
desktops.

## What we actually are

Three layers, in the order they must ship:

### 1. Protocol (`computer-session`)

A session is a leased display plus a driver.

The agent does not speak VNC. It speaks the same verbs the labs already
standardized: screenshot, click, type, key, scroll, wait, optional
accessibility tree, optional shell. Adapters translate:

- Anthropic `computer_toolset_20260801`
- OpenAI Responses `computer` tool
- Gemini Computer Use (`ENVIRONMENT_DESKTOP`)
- MCP (Claude Code, Codex, Grok Build, OpenClaw)

One session, four mouths. Spec: [../spec/computer-session.md](../spec/computer-session.md).

### 2. Node

A node is a daemon on hardware *you* control. It never offers the host
desktop. It offers a **guest**:

- Linux: QEMU/KVM or a nested container with a real X/Wayland session.
- Windows: Hyper-V / QEMU evaluation for *private* use; W365 for Agents
  or AVD for *public*.
- macOS: Lume VM on Apple silicon for *private* use; whole-Mac 24h exclusive
  for *public developer* use after Apple is notified.

Inside the guest: Cua Driver (or equivalent). Out to the world: a tunnel
(Cloudflare Tunnel / WireGuard) so there is no inbound port. Snapshot on
lease start, revert on lease end. Record the session. Kill on policy
violation.

This is Devin Outposts, opened, and it is useful **even if the mesh never
exists**. "Park your spare Mac mini for your own fleet of agents" is already
a product.

### 3. Control plane, then mesh

Private: your nodes, your keys, your agents. `berth up --os linux` hits your
LAN first.

Licensed cloud: the control plane can also mint a berth on Hetzner (Linux),
Azure (Windows), MacStadium/AWS (Mac). This is how we have Windows and Mac
on day one without a single parked node.

Mesh (Linux first): nodes advertise capacity. The control plane matches
`os`, `resources`, `density`, holds a lease, routes the action channel,
settles **gas**. Preemption gets a notice period (steal this from Akash
Resource Reclamation). Reputation is uptime + isolation attestations, not
vibes.

**Shared** is concurrent overcommit: many agents, each with their own
session, one host. Linux containers and Windows multi-session can do this.
macOS cannot.

**Pooled** is sequential: check out a warm guest, use it, revert, check in.
Windows 365 for Agents is pooled, not shared. That is how minutes of
Windows get cheap without noisy neighbors.

Mac on the public mesh is a **different SKU**: 24-hour exclusive Apple
hardware we (or a notified lessor) own, developer-services only,
`density=exclusive`. Price it like a day of CI. Not your MacBook.

## The wedge (what to ship before anyone earns a cent)

Local Linux is bootstrap. The thing nobody open-sources is **the other
machine**:

```
# on the NUC / Mac mini in the closet
berth node up

# on the laptop, in Claude Code / Codex / Grok Build
berth up --node home-nuc
claude mcp add berth -- berth mcp
```

That is Devin Outposts for any agent. Grok Bot cannot point at your box.
Cua Driver can drive the box you are sitting at; it does not lease the one
downstairs.

Then the same command grows inventory:

1. Local Linux (prove the loop).
2. Remote private outpost (the wedge).
3. `--provider w365-agents --os windows` — the "Grok Bot but Windows"
   headline, ~$0.40/hr, pooled check-out.
4. `--provider aws --os macos` with the 24h floor visible before confirm.
5. `--mesh --os linux --density shared` on wired mini PCs.
6. Company-run Mac §3 fleet if Apple answers the notice. Consumer
   MacBooks never land here.

The mesh is a flag on the same `berth up`. It is not a whitepaper.

## Competitive position

| | Agent | Driver | Linux desktop | Windows | macOS | BYO hardware | Open | Other agents can use it |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Grok Bot | yes | closed | yes | no | no | no | no | no |
| Devin | yes | closed | yes | yes | Outposts only | Outposts | no | no |
| Cua | no | **yes (MIT)** | cloud + local | cloud + local | local Lume; cloud waitlist | local only | driver yes | if you integrate |
| E2B Desktop | no | SDK | hosted Xfce | no | no | no | SDK | if you integrate |
| W365 for Agents | no | MS APIs | some (Researcher) | **yes, pooled Cloud PC** | no | no | no | Copilot Studio / Agent 365 first |
| Operator / CUA API | yes | you | you | you | you | you | API | n/a |
| Claude computer use | yes | you | demo Docker | you | you | you | API | n/a |
| **berth** | no | wrap Cua | yes | W365 Agents + private | Lume private + §3 public | **yes** | **yes** | **yes** |

We do not fight Cua for the driver. We do not fight Devin for the SWE agent.
We fight for the *socket*: BYO hardware for any agent, then a broker, then
a Linux mesh. If Cua or Devin open that socket, berth becomes an
implementation with inventory. That is still a company.

Cua is a complement. Wrap their driver. Do not race `pip install cua`.

## Creative SKUs (the product, not the protocol)

Once the socket exists, the interesting inventory is not "a VM."

1. **Golden images for agents.** "Windows 11 + VS 2022 + a signed-in
   store account + this `.vsix`." "macOS + Xcode + this simulator pair."
   Snapshot boot in seconds (Daytona already does this for Devin). The
   mesh's job is to keep those images warm.

2. **Proof of desktop.** TPM / Secure Enclave attestation that the guest
   is real Windows/macOS on real hardware, not a screenshot farm. This is
   the actually-Bitcoin part: scarce, attested work. Without it the mesh
   is full of fraud.

3. **Session recording as a receipt.** Devin already records tests. A
   berth that can prove *what the agent did* is how enterprises say yes.

4. **Capability matching, not just OS.** `needs: ["iphone-simulator",
   "ax-tree", "gpu:metal"]`. A Mac mini in a closet in Berlin that is
   already running Lume is worth more than a cold AWS host.

5. **Human-in-the-chair.** Optional. The lease can require a person to
   approve `key:Return` on a bank page. Anthropic's own classifier is
   the prompt; the berth is the enforcement point.

6. **The dark-fleet problem, inverted.** Today, malware parks machines.
   Tomorrow, people *want* to park a dedicated mini PC next to the
   router the way they parked a Helium hotspot — but only if isolation
   is default and payout is USD. Design the node like a Helium miner:
   one image, one tunnel, one dashboard, no SSH folklore.

## What would make this fail

- Shipping a token before a `berth up` that works on a laptop.
- Letting the agent drive the host cursor. One viral "it opened my
  bank" clip and the mesh is dead.
- Advertising 10-minute Mac rentals. Apple will not play, and AWS
  already told us the SLA number is 24 hours.
- Competing with Cua on sandboxes. They have YC, a driver, and a
  cloud. Be the lease layer they can speak, or the Outpost they don't
  want to build for rival agents.
- Building for "any GUI task" on Mac. Public Macs are developer
  machines. Inbox-zero bots live on Linux.
- Default-allow egress on the public mesh. A Linux desktop with a
  browser is a fraud appliance. Allowlist or KYC'd unrestricted, not both
  as the default.
- Parking laptops on the public mesh. Lid close is not an SLA. Mini PC
  on Ethernet, or stay a private outpost.

## Recommended sequencing

1. Spec + this research (this repo, now).
2. `berth node` + `berth up --os linux` locally. MCP + Anthropic adapter.
3. Remote private outpost: node downstairs, agent upstairs, tunnel.
   This is the first thing a stranger should feel.
4. Lume private macOS on Apple silicon. SLA: private, two VMs, not a
   service.
5. Windows 365 for Agents provider (`isolated` + `pooled`). AVD is the
   fallback.
6. AWS/MacStadium provider. 24h floor in the UX before confirm.
7. Linux mesh: wired mini PCs, snapshot/revert, shared + isolated,
   default-deny egress, credits (spend or cash out).
8. Company-run Mac §3 fleet if Apple answers. Not a consumer Mac mesh.

Slide 1 is: **the Mac mini in the closet is now Claude Code's computer.**
Slide 2 is: **and it can be a Windows Cloud PC by Friday.**
The bitcoin speech is still slide 20. See [REVIEW.md](REVIEW.md).
