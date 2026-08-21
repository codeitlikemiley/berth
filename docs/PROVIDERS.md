# Provider map (who we wrap, not who we become)

## Private (node on hardware you own)

| Host | Guest | How | Notes |
| --- | --- | --- | --- |
| Linux + KVM | Linux desktop | QEMU + XFCE/KDE + Cua Driver | Default mesh guest. |
| Linux + KVM | Windows eval | QEMU, 90-day Enterprise ISO | Private/dev only. ([Eval Center](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise)) |
| Apple silicon | macOS VM | [Lume](https://github.com/trycua/cua) / Virtualization.framework | Max two extra instances; not a service. |
| Apple silicon | Linux VM | Lume | Legal to mesh. The Mac is just a hypervisor. |
| Windows Pro | Windows VM | Hyper-V | Private outpost. Do not mesh a Home OEM. |

Tunnel: Cloudflare Tunnel or Tailscale. Driver: Cua Driver inside the guest.

## Licensed cloud (control plane calls their API)

| Need | Provider | GUI | Gotcha |
| --- | --- | --- | --- |
| Linux, cheap, fast | Hetzner, Fly, Cua Cloud Linux, GCE | Install XFCE or use Cua image | Easiest SKU. |
| Windows for agents | **Windows 365 for Agents** | Yes — 65 MCP tools, check-out/check-in | `density=isolated` + `pooled=true`. US **$0.40/VM/hr** + $5/VM/month always-on. Needs Agent 365 + Entra blueprint + Intune pool. **v0.2 provider, not v0.1.** Dev docs: [microsoft/windows-365-for-agents](https://github.com/microsoft/windows-365-for-agents). Mapping: [WINDOWS365.md](WINDOWS365.md). |
| Windows for external humans/ISV apps | **Azure Virtual Desktop** per-user access | Yes | You are the ISV; meter is per user/month who connected, plus VM. ([docs](https://learn.microsoft.com/en-us/azure/virtual-desktop/licensing)) |
| Windows, bursty, already licensed | AWS/Azure Windows Server + GPU optional | Add RDP/noVNC + driver | RDS CAL/SPLA if multi-session. |
| macOS, on-demand | **AWS EC2 Mac** Dedicated Host | VNC / Apple Screen Sharing / [Amazon DCV](https://aws.amazon.com/blogs/desktop-and-application-streaming/enabling-remote-macos-development-with-amazon-ec2-mac-and-amazon-dcv/) | **24h minimum**, billed while allocated. ~$1.08–$1.23/hr class. |
| macOS, monthly | MacStadium, MacinCloud, rentamac.io, Scaleway | VNC / ARD | Real Mac minis. Scaleway historically also 24h min. |
| macOS, agent-shaped | Cua Cloud macOS, Namespace Devboxes | Their viewer + API | Cua is partnership/waitlist for large fleets. Namespace is a Devin Outposts partner. |

## Do not wrap

- GitHub-hosted macOS runners — CI, no interactive desktop lease.
- Windows Home laptops on a "miner" binary.
- Hackintosh / macOS on KVM on a PC.
- Vast.ai / Akash GPU listings as if they were desktops. Use them later if we need GPU inside a Linux berth, not as the session itself.

## Suggested default routing

```
berth up --node home-nuc                         → private outpost (the wedge)
berth up --os linux --density shared             → overcommitted KVM / mesh (cheap)
berth up --os linux                              → local isolated, else Hetzner/Cua
berth up --os windows                            → W365 Agents (isolated+pooled) if creds, else local eval
berth up --os macos                              → local Lume if Apple silicon
berth up --os macos --cloud                      → AWS Mac, show 24h floor before confirm
berth up --os macos --mesh                       → refuse unless we are the §3 lessor
```
