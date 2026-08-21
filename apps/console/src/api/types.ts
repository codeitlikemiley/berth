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

export type NodeView = {
  ok: boolean;
  parked: boolean;
  bind: string;
  origin: string | null;
  class: string;
  image: string;
  allowlist: string[];
  allowlist_source: string;
  docker: { ok: boolean; detail: string };
  guest_image: { ok: boolean; name: string };
  home_writable: boolean;
  tunnel: { kind: string; named?: boolean; child_alive?: boolean };
  active_bearers: number;
  live_sessions: number;
  shutting_down: boolean;
  host_desktop_driven: boolean;
};

export type BerthApi = {
  pair: (code: string) => Promise<PairResponse>;
  listLeases: () => Promise<LeaseList>;
  getLease: (id: string) => Promise<LeaseView>;
  endLease: (id: string) => Promise<LeaseView>;
  forceEnd: (id: string) => Promise<LeaseView>;
  node: () => Promise<NodeView>;
  park: () => Promise<NodeView>;
  unpark: () => Promise<NodeView>;
};
