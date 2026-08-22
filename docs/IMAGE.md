# Golden image, storage, first boot

The node is not "any Linux with xdotool." It is a **known image** so an
agent can assume Chromium, a driver, rclone, and a tunnel. The host
flashes it (or pulls a container), signs in, and starts earning. That is
the Helium-miner moment, without a wallet requirement.

## Images (what we ship)

One family, three skins. Versioned. Agents pin `image: berthos-linux-xfce:2026.08`.

### `berthos-linux` (the mesh default)

Debian or Ubuntu LTS, XFCE, one display server per session.

Preinstalled (the bells):

- **Driver:** Cua Driver (or AT-SPI fallback)
- **Browser:** Chromium, ungoogled flags, no saved host profiles
- **Agent tools:** git, python3, node LTS, ffmpeg, jq, curl, unzip
- **Storage:** rclone, awscli, mc (MinIO client); FUSE ready
- **Node:** `berth-agent`, `cloudflared` or wireguard
- **Hardening:** unattended-upgrades, no host SSH from the guest,
  default-deny egress nftables (allowlist injected at lease)
- **Workspace:** `/workspace` is the only persistent mount

Build: Dockerfile + Packer. Same idea as
[E2B desktop template](https://github.com/e2b-dev/desktop) and
[trycua/cua-xfce](https://github.com/trycua/cua). We do not invent a
kernel. We invent the *contract* (paths, users, services).

Session = container from this image (shared) or VM from this image
(isolated).

### `berthos-windows` (cloud / private)

Not an ISO we pirate. Paths:

- W365 / AVD gallery image + Intune app catalog (cloud)
- Private: Windows 11 Enterprise **eval** for bringing the node up,
  90 days ([Eval Center](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise))
- Production mesh Windows: SPLA or don't

Golden: sysprep, Cua Driver, git, rclone, Chromium, `berth-agent`.
Apps the tenant needs are in the image, not installed into a
throwaway profile (FSLogix will wipe Store apps on sign-out).

### `berthos-macos` (private Lume + §3 minis)

Lume unattended preset (Cua already: user `lume`, SSH, autologin, no
sleep). Add: Cua Driver, Xcode CLT optional (`image: …-xcode`), rclone,
`berth-agent`. Public minis are this image on hardware we notified
Apple about.

## Storage (compute is not enough)

Three layers, billed separately. E2B already splits RAM and disk;
humans already split EBS from EC2.

| Mount | Lifetime | Bill | Use |
| --- | --- | --- | --- |
| `/` (root) | session | in the compute quote (`disk_gib`) | the image |
| `/workspace` | workspace_id, survives leases | **GiB-month** | git, caches, agent files |
| `/mnt/s3` | synced from a bucket, per lease | **we don't bill S3**; the bucket owner does | "connect my S3" |

`/mnt/s3` config at lease:

```json
{
  "object": {
    "remote": "buyer-s3",
    "bucket": "my-agent-data",
    "prefix": "berth/ws_123"
  }
}
```

**Sync, not mount.** rclone copies the prefix into `/mnt/s3` before the guest
starts and syncs it back after the guest is gone. A FUSE mount would need
`SYS_ADMIN` in the guest, which would undo the capability posture above for a
convenience; a sync keeps `cap_drop: ALL` intact. The cost is that the bucket
sees the result at lease end, not continuously — writes are not live, and a
node that dies mid-lease loses the delta.

**Credentials stay on the node.** `remote` names an rclone remote configured in
`$BERTH_HOME/rclone.conf`; it is not a credential. This matters because the node
stores every `LeaseRequest` verbatim as `request_json` and `GET /v1/leases`
hands it back — a secret in the lease would be a secret in the API's output.
rclone itself runs in a short-lived helper container with that config bind-mounted
read-only, so the guest never has the credentials, only the bytes. Staged files
are chowned to the guest user, since the helper runs as root and the guest
does not.

`prefix`, `bucket` and `remote` are validated before anything runs: a `/` or `:`
in the remote or bucket, or a `..` in the prefix, would re-point rclone
somewhere the lease never asked for.

We can also run a **Berth object store** (Garage / MinIO) for buyers
who do not have a bucket. That is a separate SKU: `$/GiB-month` at
Backblaze/Wasabi-class (~$0.006/GiB-mo), not compute gas.

Snapshots: `/workspace` is the snapshot unit, not the whole VM, so a
5-minute Linux shared session does not pay to persist a 12 GB root.

## First boot (earn without a coin)

1. Flash USB or `docker pull berthos-linux`.
2. Boot. `berth-agent` starts a setup TUI / local web UI.
3. Sign in with **account** (email, GitHub, SSO). Not a wallet.
4. Pick `class=private` (only my agents) or `class=mesh` (earn).
5. Mesh: show estimated $/hr from MATH.md given detected CPU/RAM;
   require wired Ethernet; refuse laptop batteries as public.
6. Agent prints a node token. Control plane attests image hash.
7. Optional: connect Stripe / USDC for cash-out. Optional: connect a
   bucket for default `/mnt/s3`.

The Helium mistake was: buy token → run miner. The Vast.ai pattern is:
run agent → get paid USD. Copy Vast.

Image hash is part of attestation. A host that swapped the ISO for a
keylogger does not stay in the mesh.

## Why this is the real-world fix

Agents fail today because "install Docker, XFCE, xdotool, grant TCC,
open a tunnel, hope the resolution is 1280×800." The model APIs assume
a computer; they do not ship one.

A pinned image + a workspace volume + an S3 mount + a 60-second
on-demand clock is that computer. Windows and Mac are the same *lease
verb* with different images and legal minima.
