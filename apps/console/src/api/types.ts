export type ConsoleMode = "node" | "control-plane";

export type PairResponse = {
  token: string;
};

export type Quote = {
  vcpu: number;
  mem_gib: number;
  disk_gib: number;
  os: string;
  os_mult: number;
  density: string;
  density_mult: number;
  term?: string;
  min_seconds: number;
  pooled: boolean;
  cpu_overcommit?: number;
  gas_per_second: string;
  currency: string;
  usd_per_gas: string;
  preemptible?: boolean;
};

export type LeaseView = {
  lease_id: string;
  session_id: string;
  ws_url: string;
  viewer_url?: string | null;
  quote: Quote;
  status: string;
  billable_seconds?: number | null;
  elapsed_seconds?: number | null;
  started_at: number;
  stopped_at?: number | null;
  workspace_id: string;
  live: boolean;
  end_reason?: string | null;
  forfeited: boolean;
};

export type LeaseList = {
  leases: LeaseView[];
  truncated: boolean;
};

export type DockerProbe = {
  ok: boolean;
  detail: string;
};

export type GuestImageProbe = {
  ok: boolean;
  name: string;
  /** Why the image is or is not usable — an image that merely exists may have no egress filter. */
  detail: string;
};

export type TunnelStatus =
  | { kind: "none" }
  | { kind: "cloudflare"; named: boolean; child_alive: boolean };

export type NodeStatus = {
  ok: boolean;
  parked: boolean;
  bind: string;
  origin: string | null;
  class: string;
  image: string;
  allowlist: string[];
  allowlist_source: string;
  docker: DockerProbe;
  guest_image: GuestImageProbe;
  home_writable: boolean;
  tunnel: TunnelStatus;
  active_bearers: number;
  live_sessions: number;
  shutting_down: boolean;
  host_desktop_driven: boolean;
};

export type NodeView = NodeStatus;

export type WizardLease = {
  os: "linux";
  density: "isolated" | "shared";
  resources: { vcpu: number; mem_gib: number; disk_gib: number };
};

export type BerthApi = {
  pair: (
    code: string,
    opts?: { revokeOthers?: boolean },
  ) => Promise<PairResponse>;
  pairingCode: () => Promise<{ code: string } | null>;
  listLeases: () => Promise<LeaseList>;
  getLease: (id: string) => Promise<LeaseView>;
  endLease: (id: string) => Promise<LeaseView>;
  forceEnd: (id: string) => Promise<LeaseView>;
  quote: (req: WizardLease) => Promise<Quote>;
  createLease: (req: WizardLease) => Promise<LeaseView>;
  node: () => Promise<NodeStatus>;
  park: () => Promise<NodeStatus>;
  unpark: () => Promise<NodeStatus>;
  preview: (sessionId: string) => Promise<Blob | null>;
};
