# Computer Session Protocol (draft 0.1)

A **session** is a leased graphical desktop plus the driver that can act on
it. This spec is the socket. Transports and adapters sit on top.

Goals:

- One action vocabulary that can be projected onto Anthropic
  `computer_toolset_20260801`, OpenAI Responses `computer`, Gemini
  Computer Use, and MCP.
- Isolation is visible in the lease, not hoped for.
- OS class and license class are first-class fields so a Mac Home-and-Garden
  laptop cannot pretend to be a §3 developer lease.

Non-goals:

- Replacing Cua Driver / UIA / AT-SPI. The driver is a backend.
- Pixel-exact VNC. Humans may attach a viewer; agents use actions + frames.
- A speculative ticker. Gas is a meter; see [docs/ECONOMICS.md](../docs/ECONOMICS.md).

## Identifiers

```
berth_id     = opaque id of a node or cloud pool
lease_id     = id of a reservation
session_id   = id of a live desktop (usually 1:1 with lease)
```

## Lease

`POST /v1/leases`

```json
{
  "os": "linux" | "windows" | "macos",
  "class": "private" | "licensed-cloud" | "mesh",
  "license": "linux" | "w365-agents" | "avd-external" | "avd-multisession" | "eval" | "apple-private" | "apple-section-3",
  "density": "shared" | "isolated" | "exclusive",
  "pooled": false,
  "term": "on_demand" | "monthly" | "annual",
  "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 },
  "workspace": { "id": "ws_...", "disk_gib": 20 },
  "object": { "remote": "buyer-s3", "bucket": "…", "prefix": "berth/ws_…/" },
  "cpu_overcommit": 1.0,
  "min_seconds": 300,
  "max_seconds": 86400,
  "exclusive_hardware": false,
  "capabilities": ["ax-tree", "shell", "gpu", "xcode", "iphone-simulator"],
  "image": "berth/linux-xfce:2026-08",
  "region": "eu",
  "isolation": "vm",
  "network": { "egress": "allowlist", "domains": ["github.com", "pypi.org"] },
  "recording": true,
  "human_confirm": ["paste", "key:Return"],
  "preemptible": true
}
```

`object.remote` names a remote configured on the node; it is not a credential. The node stores the whole request as `request_json` and returns it from `GET /v1/leases`, so a key placed here would sit in plaintext. The node stages `remote:bucket/prefix` into `/mnt/s3` before the guest starts and syncs it back after the guest stops, so the guest never sees the credentials either.

`density` slices silicon. `pooled` is scheduling (warm check-out).

- `shared` — own session on an overcommitted host (many agents, one box).
  Not one desktop with many cursors.
- `isolated` — dedicated guest VM. Default. W365 for Agents is this
  plus `pooled: true`.
- `exclusive` — whole machine.

Rules the control plane must enforce:

- `os=macos` + `class=mesh` requires `license=apple-section-3`,
  `density=exclusive`, `exclusive_hardware=true`, `min_seconds >= 86400`.
- `os=macos` + `density=shared` is rejected.
- `os=windows` + `class=mesh` requires `license` in
  `w365-agents | avd-external | avd-multisession` (or documented SPLA).
- `os=windows` + `density=shared` requires `avd-multisession`. A W365
  agent pool is `isolated` + `pooled`, not `shared`. Home OEM is not a license.
- `license=eval` is `class=private` only.
- `isolation` is `vm` or `hypervisor`. `none` (host desktop) is refused for
  `class=mesh`. Shared still means per-session isolation.
- `class=mesh` nodes must advertise `cpu_overcommit` (cap 3.0) and a
  default-deny egress profile.
- `os_mult` is not applied on `class=licensed-cloud` quotes.
- `term=on_demand` minima: Linux shared 60s; Linux isolated 300s;
  W365 60s; public Mac 86400s. `monthly` / `annual` cap as in MATH.md.
- `workspace.disk_gib` bills GiB-month, separate from the session root.
- `object` is a buyer-owned bucket mount; we do not mark up S3.

Response:

```json
{
  "lease_id": "l_...",
  "session_id": "s_...",
  "expires_at": "2026-08-22T12:00:00Z",
  "endpoint": "wss://tun.example/s/s_...",
  "mcp_stdio": ["berth", "mcp", "--session", "s_..."],
  "viewer": "https://tun.example/view/s_...",
  "driver": "cua-driver",
  "quote": {
    "vcpu": 2,
    "mem_gib": 4,
    "disk_gib": 40,
    "os": "linux",
    "os_mult": 1.0,
    "density": "shared",
    "density_mult": 0.30,
    "term": "on_demand",
    "min_seconds": 60,
    "pooled": false,
    "cpu_overcommit": 2.0,
    "gas_per_second": "0.000333",
    "currency": "gas",
    "usd_per_gas": "0.01",
    "preemptible": true
  }
}
```

`DELETE /v1/leases/{lease_id}` reverts the snapshot and drops the tunnel.

A private node MAY also expose operator HTTP (list leases, park/unpark, force disconnect) on the same listener. That is the node console, not this socket. Agents remain on `POST /v1/leases`, `DELETE /v1/leases/{lease_id}`, and the session WebSocket.

## Action channel

WebSocket or QUIC. Client → server: actions. Server → client: frames,
acks, errors, policy denials.

Batching is required. Anthropic and OpenAI both emit several actions per
turn and require in-order execution, stopping at the first failure.

### Frame

```json
{
  "type": "frame",
  "session_id": "s_...",
  "ts": 0,
  "width": 1280,
  "height": 800,
  "mime": "image/png",
  "data": "<base64>",
  "cursor": [640, 400]
}
```

Coordinates in this protocol are always in the pixel space of the last
full frame, origin top-left. If the node downscales a screenshot before
sending, it must scale incoming coordinates back up before injecting
input. This matches Anthropic's contract.

Optional `ax` field: a JSON accessibility tree scoped to the active
window. Nodes that can provide it should; pixel-only nodes set
`capabilities` without `ax-tree`.

### Actions

```json
{
  "type": "actions",
  "id": "a_...",
  "session_id": "s_...",
  "items": [
    { "op": "screenshot" },
    { "op": "click", "button": "left", "xy": [100, 200], "mods": [] },
    { "op": "double_click", "xy": [100, 200] },
    { "op": "move", "xy": [100, 200] },
    { "op": "drag", "path": [[100, 200], [300, 200]] },
    { "op": "scroll", "xy": [100, 200], "dx": 0, "dy": 3 },
    { "op": "type", "text": "hello" },
    { "op": "key", "keys": ["META", "s"], "repeat": 1 },
    { "op": "hold_key", "keys": ["SHIFT"], "ms": 500 },
    { "op": "wait", "ms": 400 },
    { "op": "zoom", "region": [0, 0, 200, 200] },
    { "op": "cursor_position" },
    { "op": "shell", "cmd": "uname -a" }
  ]
}
```

Semantics:

- Execute `items` in order.
- On first failure, do not run the rest. Ack the failure and mark
  remaining items `skipped`.
- `screenshot` / `zoom` replies include a frame.
- `shell` is capability-gated. It is not part of Anthropic's computer
  toolset; expose it as a sibling MCP tool.
- `human_confirm` ops wait for a signed approval or time out.

Ack:

```json
{
  "type": "ack",
  "id": "a_...",
  "results": [
    { "i": 0, "ok": true, "frame": true },
    { "i": 1, "ok": false, "error": "policy:key:Return requires human" },
    { "i": 2, "ok": false, "error": "skipped" }
  ]
}
```

## Adapters

Each adapter is a pure translation. The session does not change.

| Incoming | Mapping |
| --- | --- |
| Anthropic member `left_click` `{coordinate}` | `click` `xy` |
| Anthropic `type` / `key` / `scroll` / `zoom` / `wait` | same op names |
| Anthropic batch in one `tool_use` list | one `actions` message, in order |
| OpenAI `computer_call.actions[]` | same |
| Gemini desktop `click` x,y in 0–1000 | rescale to last frame |
| MCP `computer_click` etc. | same |

Gemini's 0–1000 normalized coordinates are the only rescale. Everyone else
is screenshot pixels.

## Node requirements

A public mesh node MUST:

1. Boot from a snapshot; revert on lease end.
2. Run the driver **inside the guest**.
3. Expose the action channel over a tunnel it dials out.
4. Attest `os`, `license`, `isolation`, resources at register time.
5. Refuse host-desktop mode.
6. Record if the lease asked for recording.
7. Enforce default-deny egress (allowlist per lease).
8. Be a wired, no-sleep host. Laptops register as `class=private` only.

A private node SHOULD do all of the above and MAY skip attestation.

## MCP surface (minimum)

```
berth_lease     in: os, seconds, density, pooled, resources, capabilities
berth_screenshot
berth_click
berth_type
berth_key
berth_scroll
berth_ax        optional
berth_shell     optional
berth_end
```

`berth_lease` is the only extra verb the labs do not already have. That
verb is the product.
