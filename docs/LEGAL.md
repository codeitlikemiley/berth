# Legal constraints (operating systems)

This is not legal advice. It is a reading of the licenses that kill naive designs. Product decisions in [THESIS.md](THESIS.md) follow from these.

## Linux

Park it, virtualize it, rent it, mesh it. Distro licenses (GPL, etc.) do not forbid offering a Linux desktop as a service. You still own:

- Abuse (the node becomes a botnet, fraud farm, or phishing kit).
- Data protection (tenant data on the host).
- Export / sanctions.
- Whatever the *applications* inside the guest require (Adobe, Office, games).

Isolation is a product requirement, not a license requirement. Public mesh
Linux still defaults to **deny egress** except an allowlist — a desktop with
a browser is a fraud appliance even when the OS license is clean.

## Windows

A Windows **Home / Pro OEM** install on a personal laptop is licensed to the owner for their use. It is not a service-provider license. Listing that laptop as a public berth for third-party agents is the Windows equivalent of Apple's forbidden time-sharing.

Legal supply for a service that *sells Windows desktops to agents that are not your employees*:

1. **Windows 365 for Agents.** Microsoft's first-party Cloud PC for computer-using agents. Check-out/check-in pools, PAYG hourly, Intune-managed. FAQ names ISVs as a customer. Default Windows backend for berth cloud. ([Learn](https://learn.microsoft.com/en-us/windows-365/agents/introduction-windows-365-for-agents), [pricing FAQ](https://www.microsoft.com/en-us/windows-365/agents))
2. **Azure Virtual Desktop per-user access pricing** for external commercial purposes. You enroll an Azure subscription, you are the ISV, the agent (or its operator) is the external user, you pay the Apps or Desktops+apps meter plus the VM. Internal users (your staff) must **not** ride this meter; they need M365 E3/E5 / Windows E3/E5 / VDA. ([Licensing AVD](https://learn.microsoft.com/en-us/azure/virtual-desktop/licensing))
3. **Windows 365** (human) Cloud PCs, per user per month. Poor fit for bursty agent sessions.
4. **Windows Server** session hosts with RDS CALs / SPLA. AVD per-user access is **not** available for Windows Server.
5. **Evaluation ISOs** (Windows 11 Enterprise, 90 days, then hourly shutdown). Acceptable for local **dev** of berth itself and for short-lived research VMs. Not a production fleet. ([Evaluation Center](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise))

Private outpost (your Windows machine, your agent) is a different question: you are using your own license, not selling Windows. Still isolate the agent in a VM. Do not let it drive the logged-in user session.

## macOS

Source: [Software License Agreement for macOS Tahoe 26](https://www.apple.com/legal/sla/docs/macOSTahoe.pdf).

### Hard no

- macOS on non-Apple hardware (§2J).
- Using the two allowed VMs as a service bureau / time-sharing / terminal sharing / relay (§2B(iii) closing paragraph, §2I closing paragraph).
- Renting, leasing, lending, selling, redistributing, or sublicensing except as §3 allows (§2J).
- More than one device *controlling* the graphical desktop at a time (§2I(i)).

### Allowed without §3

- Your Mac, your agent, your VM via Virtualization.framework (Lume). Development, testing, personal non-commercial, up to two extra instances (§2B(iii)). This is the **private outpost**.
- Screen sharing: one controller, many observers (§2I).

### Allowed with §3 (the only public-Mac path)

Lease the **whole Mac** (software + Apple-branded hardware) for **Permitted Developer Services** only:

- CI, building from source, automated testing during software development, developer tools.
- Minimum **24 consecutive hours**.
- Lessee has **exclusive** control of that hardware for the lease.
- **Notify Apple in advance** ([macos-license contact](https://developer.apple.com/contact/macos-license/)).
- Lessee agrees to the SLA.

This is why AWS, and historically Scaleway, impose a 24-hour minimum. A "Mac for 10 minutes of computer use" product is not a §3 lease. A "Mac overnight so an agent can build and test an iOS app" can be, if we notify Apple and keep the hardware exclusive.

Permitted Developer Services does **not** obviously include generic RPA (inbox, CRM, booking flights). Frame public Mac berths as **developer machines**. Put generic computer-use on Linux and Windows.

### Cloud Mac operators

AWS, MacStadium, MacinCloud, Scaleway, Cua, Namespace run on Apple hardware under their own arrangements with Apple. We consume them as **providers**, we do not pretend a consumer MacBook is the same thing.

## What the mesh may advertise

| Guest | Private outpost (your agent) | Public mesh (strangers' agents) |
| --- | --- | --- |
| Linux VM, isolated | yes | yes |
| Linux **shared** (many sessions / overcommit on one host) | yes | yes — cheapest SKU |
| Windows, W365-for-Agents / AVD / SPLA, isolated | yes | yes, as the ISV |
| Windows **shared** (Win11 Ent. multi-session) | n/a | **AVD in Azure only** — not a parked PC ([FAQ](https://learn.microsoft.com/en-us/azure/virtual-desktop/windows-multisession-faq)) |
| Windows **pooled** (W365 check-out) | n/a | yes, as the ISV |
| Windows Home on a parked PC | yes, isolate it | **no** |
| macOS VM on your Mac (Lume) | yes | **no** (time-sharing) |
| macOS **shared** (several controllers, one Mac) | **no** (§2I) | **no** |
| Whole Apple Mac, 24h exclusive, §3 + notice, developer services | yes | **only if we (or a notified lessor) own the mini**. Not a consumer MacBook |
| macOS on non-Apple hardware | **no** | **no** |

If a node cannot prove isolation and OS license class, it does not join the public mesh.
