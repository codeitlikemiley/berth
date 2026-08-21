# Computer-use landscape (August 2026)

Primary sources only. Secondary roundups are ignored unless they quote a first party.

## 1. Demand: the models learned to click. They still need a machine.

### Anthropic — computer use is GA

On 20 Aug 2026 Anthropic made computer use, the browser tool, the Skills API, and the Files API generally available. The current computer toolset is `computer_toolset_20260801`: Claude returns several actions per turn (click, type, key, screenshot) instead of one per round trip. Early-access customers saw 20–40% fewer round trips per task. ([@ClaudeDevs](https://x.com/ClaudeDevs/status/2090540270219567575), [platform docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool))

The toolset has 17 members: `screenshot`, `zoom`, `left_click` / `right_click` / `middle_click` / `double_click` / `triple_click`, `left_click_drag`, `mouse_move`, `left_mouse_down` / `left_mouse_up`, `cursor_position`, `scroll`, `type`, `key`, `hold_key`, `wait`. Coordinates live in screenshot-pixel space. The API does **not** host a desktop. Your application runs every call in an environment you control. Anthropic's own reference is a Linux Docker image with Xvfb, Mutter, Tint2, Firefox. ([docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool), [computer-use-demo](https://github.com/anthropics/anthropic-quickstarts/tree/main/computer-use-demo))

Security guidance from Anthropic is explicit: dedicated VM/container, no secrets in the environment, domain allowlists, human confirmation for consequential actions. Prompt-injection classifiers fire on screenshots. ([same docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool))

For web-only tasks they now recommend `browser_toolset_20260801`, which acts on element references plus coordinates instead of pixels. That is a *browser*, not a computer.

### OpenAI — Responses API `computer` tool

OpenAI's Computer-Using Agent originally powered Operator (Jan 2025). The developer surface in 2026 is the Responses API `computer` tool: the model inspects screenshots and returns batched `actions[]` (`screenshot`, `click`, `type`, `scroll`, `keypress`, `drag`, `move`, `wait`). Your harness executes them in a browser (Playwright) or a VM (`xdotool` on Xvfb+XFCE). Docs still say: isolate, keep a human in the loop, treat page content as untrusted. There is no hosted Windows/macOS desktop in the API. ([OpenAI computer-use guide](https://developers.openai.com/api/docs/guides/tools-computer-use))

### Gemini — Computer Use across browser, mobile, desktop

Gemini 3.x exposes a Computer Use tool with `ENVIRONMENT_DESKTOP` (normalized 0–999 coordinates, click/scroll/hotkey/screenshot) plus browser and mobile environments. The application still executes actions. Google's earlier standalone Computer Use preview was browser-first; 3.5 Flash folded it into the main model. ([Gemini generateContent computer-use](https://ai.google.dev/gemini-api/docs/generate-content/computer-use), [Interactions API](https://ai.google.dev/gemini-api/docs/interactions/computer-use))

### Grok Bot — a computer, but Linux

xAI launched Grok Bot on 11 Aug 2026 as always-on agents with "a computer of their own in the cloud." Bots share one dedicated cloud machine per account, with a browser, filesystem, and terminal, so work continues when you close your laptop. ([x.ai/news/introducing-grok-bot](https://x.ai/news/introducing-grok-bot)) Independent writeups describe that machine as a small dedicated **Linux** instance. The Grok Bot *app* is macOS/Windows/iOS; the *guest OS the bot drives* is not advertised as Windows or macOS. ([docs.x.ai get-started](https://docs.x.ai/grok-bot/get-started))

That is the user's gripe, and it matches the public record: Grok Bot solved "the agent has no computer" by giving it a Linux PC, not a Windows or Mac PC.

### The common schema

Every lab converged on the same loop:

1. Screenshot (sometimes plus accessibility tree / DOM).
2. Model returns one or more input actions.
3. Harness executes, in order, on a display you own.
4. Repeat.

Nobody ships the display as part of the model API except the closed products (Grok Bot, Devin Cloud, Operator). The open surface is: *bring your own computer*.

---

## 2. Supply-side software: drivers and sandboxes already exist

### Cua (trycua/cua) — do not reinvent this

[Cua](https://github.com/trycua/cua) is MIT-licensed infrastructure for computer-use agents. Four products, one repo:

| Piece | What it is |
| --- | --- |
| **Cua Driver** | Background computer-use on macOS (Sequoia+Tahoe), Windows 11 / Server 2025, Linux X11 + XWayland. MCP stdio + daemon + one-shot CLI. Clicks a *window*, not the system cursor. Used by Hermes, Clicky, H Company, Factory Droid. ([cua.ai/cua-driver](https://cua.ai/cua-driver)) |
| **Cua Sandbox** | One Python API for Linux container / Linux VM / macOS / Windows / Android. Cloud (`cua.ai`) and local QEMU. |
| **Lume** | macOS/Linux VMs on Apple silicon via Virtualization.framework. `--unattended` presets, SSH, autologin. |
| **Cua Bench** | OSWorld / ScreenSpot / Windows Arena-style eval. |

Cua Cloud: Linux and Windows sandboxes are generally available; macOS was preview/waitlist and is now sold as **Cloud macOS Fleets** for "partnerships with companies that need fleets of thousands." ([cua.ai/macos](https://cua.ai/macos), [blog: Cloud Windows GA + macOS Preview](https://cua.ai/blog/cloud-windows-ga-macos-preview))

Cua is the driver and the hosted sandbox. It is **not** an agent-agnostic lease protocol, **not** a BYO-hardware outpost for Claude/Codex/Grok, and **not** a mesh. That is the seam.

### Other drivers

- Anthropic `computer-use-demo`: Linux Docker reference for the Claude tool.
- [computer-use-linux](https://github.com/agent-sh/computer-use-linux): Rust MCP server, AT-SPI / GNOME / KWin / Hyprland, Wayland-first.
- axstream (this org): streaming action language on top of Cua Driver; accessibility-tree first, pixels as fallback. Complementary, not competing.

### Browser-only is a different market

Browserbase, Steel, Hyperbrowser, Kernel, Skyvern give agents a browser, not an OS. Fine for web RPA. Useless for Xcode, WPF, Finder, Excel desktop, iPhone Simulator, Visual Studio. Do not confuse the two.

---

## 3. Who already gives an agent a computer

### Devin — the closest full suite, and it is closed

Devin Computer Use works on:

| Platform | Support |
| --- | --- |
| Linux (Devin Cloud default) | Full desktop |
| Windows (Devin Cloud) | Full desktop, native Win32/WPF/WinForms. ~9% more usage than Linux. Enterprise beta as of May 2026. |
| macOS (Devin Cloud) | **Not available** |
| Outposts Linux | If `DISPLAY` is set |
| Outposts macOS | Reuses the machine's desktop. Needs Screen Recording + Accessibility |

Source: [Devin Computer Use docs](https://docs.devin.ai/work-with-devin/computer-use), [Devin is Getting a Windows PC](https://cognition.ai/blog/devin-is-getting-a-windows-pc), [Introducing Devin Outposts](https://devin.ai/blog/introducing-devin-outposts) (21 Jul 2026).

**Outposts** is the important product: run Devin's agent loop in Devin Cloud, but put the *computer* on a GPU box, a private VM, a Kubernetes cluster, or a Mac mini on your desk. Launch partners include Namespace (Linux + Apple-silicon macOS, including M5), Daytona (Linux + Windows snapshots), Cloudflare, E2B, NVIDIA Brev. Namespace is "the only Outposts provider that runs Devin sessions on macOS" as of that launch. ([Namespace blog](https://namespace.so/blog/devin-outposts-devboxes))

So Devin has the suite the user described — Linux + Windows first-party, macOS via BYO/partner — and it is locked to Devin. An open Outposts for every agent does not exist.

### Cua Cloud

Linux + Windows desktops you can screenshot and click, billed per minute. macOS fleets behind a waitlist / partnership. Same company as the open driver. This is the commercial sandbox, not a marketplace of other people's hardware.

### Everyone else

**E2B Desktop** ([e2b-dev/desktop](https://github.com/e2b-dev/desktop)): Linux + Xfce sandbox with screenshot, mouse, keyboard, app launch, one live stream, bash. This is the closest OSS+SaaS analogue to Anthropic's reference container. Not Windows, not macOS.

**Windows 365 for Agents:** Microsoft's hosted Windows (and some Linux) Cloud PC for agents. Check-out/check-in. Copilot Studio / Agent 365 first; ISVs are a named audience. Not a Mac. Not an open protocol.

Daytona, Modal, Fly: Linux (sometimes Windows) **code** sandboxes. Some expose noVNC. They are not a Mac, and they are not a mesh.

Cua Driver on Windows requires an **interactive desktop session**. Session 0 / SSH cannot see the GUI. ([Cua no-foreground contract](https://cua.ai/docs/concepts/the-no-foreground-contract))

---

## 4. Renting the OS itself

### Linux — commodity

Any VPS + XFCE/KDE + xdotool/Cua Driver. Hetzner, Fly, GCE, AWS, Cua, Akash containers. Seconds to boot. No OS-vendor lease clause. This is why Grok Bot and Devin default here, and why a mesh is tractable.

### Windows — licensed service, not a spare laptop

You cannot legally rent a Windows Home OEM install to third-party agents.

Paths that exist:

- **Windows 365 for Agents (first-party, 2026).** Microsoft's Cloud PC SKU built for computer-using agents: check-out / check-in pools, Intune + Entra, APIs for provisioning and UI automation. PAYG **$0.40/VM/hr** (US) plus **$5/VM/month** for always-available capacity; EU $0.52/hr. 50 free hours on the Copilot Studio trial. Works with Copilot Studio computer use, Agent 365, Project Opal, and Researcher (Linux Cloud PCs). Explicitly sold to ISVs as "a standardized, secured execution environment — supporting Windows and Linux — without building their own runtime." ([Learn: What is Windows 365 for Agents?](https://learn.microsoft.com/en-us/windows-365/agents/introduction-windows-365-for-agents), [product + pricing FAQ](https://www.microsoft.com/en-us/windows-365/agents))
- **Azure Virtual Desktop, external commercial (per-user access pricing).** Designed for ISVs who sell remote desktops/apps to *external* users. Enroll an Azure subscription; pay a flat per-user/month meter on top of VM/storage, only for users who connected that month. Two tiers: Apps vs Desktops+apps. Does **not** cover your own employees (those need M365 E3/E5 etc.). ([Microsoft Learn: Licensing AVD](https://learn.microsoft.com/en-us/azure/virtual-desktop/licensing))
- **Windows 365 (human Cloud PCs).** Flat per-user, $28–$765+/month. Human-shaped, not bursty agent-shaped.
- **AWS/Azure Windows Server VMs** with license included. Fine for a Windows desktop if you add a GUI and RDP/noVNC.
- **Windows 11 Enterprise Evaluation.** 90-day, no product key, then hourly shutdowns and "not genuine" nags. Cua's local QEMU path uses this ISO. Legal for evaluation, not a production fleet. ([Microsoft Evaluation Center](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise))

A parked Windows gaming PC running Home is not a supply node.

### macOS — Apple hardware only, and leasing is a 24-hour exclusive developer contract

macOS Tahoe SLA, in Apple's own PDF ([macOSTahoe.pdf](https://www.apple.com/legal/sla/docs/macOSTahoe.pdf)):

- Install/run only on **Apple-branded** hardware (§2A, §2D, §2J).
- Up to **two additional VM copies** on a Mac you own/control, for software development, testing, macOS Server, or personal non-commercial use (§2B(iii)). Those VMs may **not** be used for "service bureau, time-sharing, terminal sharing, relay service or other similar types of services."
- Remote desktop: one device may *control* the graphical session at a time (§2I). Observation by many is allowed; control is not.
- "You may not rent, lease, lend, sell, redistribute or sublicense the Apple Software" except as §3 allows (§2J).

**§3 Leasing for Permitted Developer Services** is the door:

1. Lease the *entire* licensed macOS + the Apple-branded hardware.
2. Use is **only** Permitted Developer Services: CI, building from source, automated testing, developer tools.
3. **Minimum 24 consecutive hours.**
4. During the lease the lessee has **sole and exclusive** use and control of the software *and the hardware*. Lessor may only do admin support.
5. **Advance notice to Apple** via [developer.apple.com/contact/macos-license](https://developer.apple.com/contact/macos-license/).
6. Sublease allowed under the same terms.
7. Virtualization: lessor or lessee (not both) may add VMs per §2B(iii); lessor may virtualize a *single* instance as a provisioning tool.

This is why **AWS EC2 Mac is a Dedicated Host with a 24-hour minimum allocation**, billed even if the instance is stopped. AWS says so in as many words: the 24-hour minimum exists "to comply with the Apple macOS Software License Agreement." ([EC2 Mac](https://aws.amazon.com/ec2/instance-types/mac/), [EC2 Mac user guide](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ec2-mac-instances.html), [M4 announcement](https://aws.amazon.com/blogs/aws/announcing-amazon-ec2-m4-and-m4-pro-mac-instances/)) Community-reported on-demand is on the order of **~$1.08–$1.23/hour** (mac1.metal / M4 class), so a 24-hour floor is **~$26–$30** before storage and IPv4. ([vantage.sh mac1.metal](https://instances.vantage.sh/aws/ec2/mac1.metal), Reddit report of M4 ~$1.23/hr)

Other Mac clouds, all on real Apple hardware:

- **MacStadium** — bare-metal Mac minis, monthly. CI and Orka virtualization.
- **MacinCloud** — from ~$25/month, macOS 15/26, multiple DCs.
- **Scaleway Apple silicon** — Mac mini as-a-service, historically ~$0.12/hr with a **24-hour minimum** (same SLA).
- **rentamac.io** — dedicated M4 mini from $3.30/day.
- **Cua Cloud macOS** — M1/M2/M4 hosts, partnership waitlist for large fleets.
- **Namespace Devboxes** — Apple silicon including M5, Devin Outposts launch partner.
- **GitHub-hosted macOS runners** — CI, not an interactive desktop.

There is no legal "run macOS on a parked Windows PC." Hackintosh is a SLA violation (§2J). Nested virt of macOS on non-Apple hardware is the same violation.

---

## 5. Decentralized compute: they sell GPUs, not desktops

| Network | What it actually sells |
| --- | --- |
| [Vast.ai](https://vast.ai) | GPU/CPU VMs. Host agent on Linux. Interruptible bids. Pays BTC/USD. ~15% cut. |
| [Akash](https://akash.network) | Container marketplace. SDL spec, reverse auction, AKT. Resource Reclamation (2026) adds a notice period before a provider yanks a lease. |
| io.net, Salad, Render, Nosana, Clore | GPU inference/training/render. Tokens or USD. |
| Helium / Grass | Wireless / bandwidth, not computers. |

None of these rent a *graphical desktop session* with mouse/keyboard/screenshot semantics. The workloads are batch and fungible (an H100 is an H100). A computer-use session is interactive, latency-sensitive, OS-specific, and stateful (cookies, Finder, Xcode derived data).

Akash's 2026 UX work is a warning: even *container* compute needed four-step onboarding and a trial-without-wallet because "acquire tokens, manage a wallet" killed conversion. A computer-use mesh that starts with a token will not get agents. ([Akash Console onboarding](https://x.com/akashnet/article/2079959461867712788))

---

## 6. Plumbing: how a remote desktop reaches an agent

Computer-use is not RDP-for-humans. The agent needs:

- **Action channel** with 50–200 ms round trip for click→screenshot. WebRTC data channel or a persistent QUIC/WebSocket beats VNC-the-pixels-then-OCR.
- **Optional human view** (noVNC, Guacamole, Selkies, DCV, Sunshine/Moonlight) so a person can watch or take over.
- **Tunnel from NAT** without opening inbound ports: Cloudflare Tunnel, Tailscale, WireGuard, frp. This is the "download a tunnel" step in the user's sketch.
- **Driver inside the guest**: Cua Driver / xdotool / UIA, not "send input to the host cursor."

AWS already ships **Amazon DCV on EC2 Mac** for a high-quality GUI. That is a human streaming protocol. The agent protocol is smaller: actions + frames + maybe an AX tree.

---

## 7. What nobody has

True as of 21 Aug 2026:

- An **open Outposts**: point Claude Code, Codex, Grok Build, or a custom loop at a leased Linux/Windows/macOS desktop without joining Devin or Cua Cloud.
- A **legal Mac mesh**. Consumer "park your MacBook" is SLA-incompatible. A 24-hour exclusive Apple-hardware developer lease *with notice to Apple* is the only P2P-shaped Mac path, and it is a different product from five-minute RPA.
- A **Windows mesh of Home PCs**. Windows 365 for Agents and AVD per-user access are the ISV paths; they are Azure, not a gaming laptop in a bedroom.
- A **settlement layer for desktop-seconds**. GPU markets exist. Desktop-seconds do not. That is the "bitcoin for agents" slot — and it is the last thing to build, not the first.

Cua has the driver and a cloud. Devin has the agent and Outposts. Anthropic/OpenAI/Gemini have the hands. The missing object is the **berth**.
