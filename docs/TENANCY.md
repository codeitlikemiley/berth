# Many agents, one box, separate workspaces

The cheap SKU only works if two tenants cannot see each other's
desktop, files, or apps. "Share the VM" in the product sense is
**share the silicon**, not share the login. This is a solved problem on
Linux, a licensed Azure-only problem on Windows, and mostly forbidden
on macOS.

## Linux — this is the shared SKU

One physical host, many **sessions**. Each session is a container (or a
lightweight VM) with:

- its own PID/user namespace
- its own filesystem (ephemeral root + optional home volume)
- its own network namespace (egress allowlist)
- its own display (`Xvfb :N` or Wayland socket)
- its own Cua Driver / xdotool talking only to that display
- cgroup caps for `vcpu`, `mem_gib`, `disk_gib`

This is [Kasm Workspaces](https://kasm.com/): one Docker container per
desktop, streamed out, destroyed at end. We do not need to fork Kasm;
we need the same isolation shape, with an agent action channel instead
of only a human VNC.

App limiting:

- Image is the allowlist. The golden ISO/container has the apps. The
  tenant does not `apt install` on the host.
- Optional: drop capabilities, seccomp, no-new-privs.
- Persistent *workspace* is a volume mounted at `/workspace`, not `$HOME`
  of a shared user.

Do **not** put two agents in one X session with two logins. That is one
cursor. Fast user switching is not a mesh feature.

## Windows — isolation is a license, not a kernel trick

| Want | How | Where it is legal |
| --- | --- | --- |
| Many interactive sessions on one Windows 11 | **Windows 11 Enterprise multi-session** + FSLogix profile containers | **Azure Virtual Desktop only** (Citrix/Omnissa approved). *Not* on a parked PC. ([multi-session FAQ](https://learn.microsoft.com/en-us/azure/virtual-desktop/windows-multisession-faq): "We don't allow customers to run Windows Enterprise multi-session in production environments outside of the Azure Virtual Desktop service.") |
| Many sessions on one box, not AVD | Windows Server + RDS CALs / SPLA | Datacenter host with SPLA. Not Windows Home/Pro OEM. |
| One agent, one Windows desktop | W365 for Agents (check-out) or Hyper-V VM | Cloud / private outpost |
| Limit apps per user | Intune / AppLocker / WDAC; provision apps into the *image*, not the profile | AVD / W365. FSLogix **deletes store-installed apps on sign-out** unless provisioned for all users ([same FAQ](https://learn.microsoft.com/en-us/azure/virtual-desktop/windows-multisession-faq)). |

Microsoft's own density guide (computer-use is a **heavy** workload:
browser + IDE + screenshots): **2 users per vCPU**, min 8 vCPU / 16 GB
for multi-session. ([session-host sizing](https://learn.microsoft.com/en-us/windows-server/remote/remote-desktop-services/session-host-virtual-machine-sizing-guidelines))
So a D8 is ~16 light humans or ~4–8 agent sessions, not 32.

**Consequence:** the cheap Windows SKU is **pooled isolated Cloud PCs**
(W365 check-out), not "park your gaming PC and stack 8 agents on it."
A parked Windows Home box is private-outpost only.

FSLogix is the workspace: the profile VHD is the user's disk; it
follows them across hosts. That is the Windows analogue of our
`/workspace` volume.

## macOS — one controller

- Fast user switching exists; only **one** GUI controller at a time
  (SLA §2I).
- Up to **two extra VMs** on Apple silicon you own, for *your* dev/test
  (§2B(iii)) — private outpost, not a service.
- Public: exclusive hardware, 24h, developer services, Apple notified.

There is no supported "8 macOS workspaces on one mini" product we can
ship. Two Lume VMs on *your* Mac for *your* two agents is the cap.

App limiting: the image (Lume preset). TCC (Screen Recording,
Accessibility) granted to the driver once per VM.

## What a "workspace" is in the protocol

```
workspace_id
  volume     persistent disk (home, git, caches)   billed GiB-month
  object     S3-compatible mount at /mnt/s3        user's bucket or ours
  image      which golden root they booted
  apps       subset of the image's app catalog (optional allowlist)
```

Two agents on one host ⇒ two `workspace_id`s ⇒ two volumes, two
displays, two drivers. They never share `/home`. They may share the
*image cache* (read-only).

## Mapping to density

| density | Linux | Windows | macOS |
| --- | --- | --- | --- |
| shared | Kasm-style containers | AVD multi-session **in Azure** only | illegal |
| isolated | one VM | one W365 Cloud PC or Hyper-V VM | one Lume VM (private) |
| exclusive | whole box | whole box (SPLA) | whole mini, 24h public |
| pooled (scheduling) | warm container/VM pool | W365 check-out | warm Mac, still 24h min public |
