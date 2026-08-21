# Windows 365 for Agents — berth mapping

Primary sources:
[github.com/microsoft/windows-365-for-agents](https://github.com/microsoft/windows-365-for-agents),
[Getting Started](https://github.com/microsoft/windows-365-for-agents/blob/main/docs/getting-started.md),
[API](https://github.com/microsoft/windows-365-for-agents/blob/main/docs/api-reference.md),
[Sessions](https://github.com/microsoft/windows-365-for-agents/blob/main/docs/sessions.md).

## What it is

Microsoft's **pooled Windows Cloud PC for agents**. Check out a machine,
click/type/screenshot via **65 MCP tools**, check it back in. Entra-joined,
Intune-managed, PAYG (~$0.40/hr US). Idle reclaim at **30 minutes**.
Start Session can take up to **30 seconds**.

This is the correct Windows backend for berth. We do **not** write a
Windows driver. We translate `berth_lease` / `berth_click` into their
Computer-Get / Computer-Do MCP surface.

## Four planes → berth

| Their plane | Surface | berth |
| --- | --- | --- |
| Computer-Create | Graph + Intune pools | One-time tenant setup, not the CLI |
| Computer-Get | ATG MCP Start/End Session | `berth up --os windows` / `berth end` |
| Computer-Do | 65 MCP tools (desktop + browser) | `berth_click`, `berth_type`, … |
| Computer-See | WebRTC screen-share SDK | `berth view` |

Auth is **not** our pairing token. It is an **Agent 365 agent-user
bearer** (blueprint → FIC exchange). Endpoint:

```
https://agent365.svc.cloud.microsoft/agents/tenants/{tenantId}/servers/mcp_W365ComputerUse
```

Lease flags when we wrap it:

```
os=windows
class=licensed-cloud
license=w365-agents
density=isolated
pooled=true
term=on_demand   # min ~60s; they bill hourly PAYG
```

No `os_mult` on top of their list price (MATH.md).

## Why it is not v0.1 MVP

Onboarding is an **enterprise tenant**, not Docker on a Mac:

- Agent 365 license (or M365 E7)
- Windows Enterprise E3+
- Intune + Entra ID P1
- Global Admin (or Agent ID Developer) for blueprint + consent
- Windows 365 for Agents **billing plan**
- Agent blueprint, agent user, Cloud PC **pool with that agent assigned**

Until that exists, `berth up --os windows` cannot even 401-correctly.
Linux Docker outpost needs none of it. Putting W365 in v0.1 blocks
shipping for anyone without an A365 tenant.

## v0.2 shape (first post-MVP provider)

Keep v0.1 Linux. Then a **provider crate**, not a second protocol:

```
crates/berth-provider-w365/
  acquire  → Start Session, poll Ready
  act      → map berth Action → W365 MCP tool name
  release  → End Session
  view     → session link into their screen-share SDK
```

`berth up --os windows` picks this provider when Azure/A365 creds exist.
Claude Code still talks to **our** MCP (`berth_click`). We are the USB-C
plug; they are the Windows wall socket.

Do not re-export their 65 tools as 65 berth tools. Collapse to the
computer-session verbs. Browser-specific tools can wait.

## What we will not do

- Implement Win32 UIA ourselves when this MCP exists.
- Park a Windows Home laptop as "W365-like."
- Win11 Enterprise multi-session on a mini PC (AVD-only).
- Require Agent 365 to use Linux berths.
