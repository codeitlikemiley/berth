import type {
  BerthApi,
  ConsoleMode,
  LeaseList,
  LeaseView,
  NodeStatus,
  Quote,
  WizardLease,
} from "./types";

export type { BerthApi, ConsoleMode, NodeStatus, WizardLease } from "./types";

export function leaseRequestBody(req: WizardLease) {
  // class/license/term match crates/berthos-cli mvp_lease_request; resources/density from the wizard
  return {
    os: req.os,
    class: "private",
    license: "linux",
    density: req.density,
    term: "on_demand",
    resources: req.resources,
  };
}

let instance: BerthApi | null = null;

export function bindApi(api: BerthApi): void {
  instance = api;
}

export function api(): BerthApi {
  if (!instance) {
    throw new Error("api not initialized");
  }
  return instance;
}

async function readError(res: Response, fallback: string): Promise<string> {
  const payload = (await res.json().catch(() => null)) as {
    error?: string;
    live_lease_id?: string;
  } | null;
  const message = payload?.error ?? fallback;
  if (payload?.live_lease_id) {
    return `${message} (${payload.live_lease_id})`;
  }
  return message;
}

async function readJson<T>(res: Response): Promise<T> {
  const payload = (await res.json().catch(() => null)) as
    | (T & { error?: string })
    | null;
  if (!res.ok) {
    throw new Error(payload?.error ?? `request failed (${res.status})`);
  }
  if (payload === null) {
    throw new Error("empty response");
  }
  return payload;
}

export function createApi(
  mode: ConsoleMode,
  opts: {
    base: string;
    getToken: () => string | null;
    setToken: (t: string | null) => void;
    dropRejectedBearer: (rejected: string | null) => void;
  },
): BerthApi {
  if (mode === "control-plane") {
    throw new Error("control-plane adapter is not implemented");
  }

  const base = opts.base.replace(/\/$/, "");

  async function request(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    const token = opts.getToken();
    if (token) {
      headers.set("Authorization", `Bearer ${token}`);
    }
    const res = await fetch(`${base}${path}`, { ...init, headers });
    if (res.status === 401 && path !== "/v1/pair") {
      opts.dropRejectedBearer(token);
      if (!opts.getToken()) {
        opts.setToken(null);
      }
    }
    return res;
  }

  function leasePath(id: string): string {
    return `/v1/leases/${encodeURIComponent(id)}`;
  }

  async function readNode(res: Response, fallback: string): Promise<NodeStatus> {
    if (!res.ok) {
      throw new Error(await readError(res, fallback));
    }
    return (await res.json()) as NodeStatus;
  }

  function previewPath(sessionId: string): string {
    return `/v1/sessions/${encodeURIComponent(sessionId)}/preview`;
  }

  return {
    async pair(code: string, pairOpts?: { revokeOthers?: boolean }) {
      const res = await request("/v1/pair", {
        method: "POST",
        headers: { "content-type": "application/json" },
        // omit revoke_others so a second pair does not rotate the CLI bearer
        body: JSON.stringify(
          pairOpts?.revokeOthers
            ? { code, revoke_others: true }
            : { code },
        ),
      });
      if (res.status === 401) {
        throw new Error("invalid pairing code");
      }
      if (!res.ok) {
        throw new Error(await readError(res, `pair failed (${res.status})`));
      }
      const data = (await res.json()) as { token: string };
      opts.setToken(data.token);
      return data;
    },

    async pairingCode() {
      const res = await request("/v1/pairing");
      // trycloudflare / non-loopback: operator types the stderr code
      if (res.status === 404) {
        return null;
      }
      if (!res.ok) {
        throw new Error(await readError(res, `pairing failed (${res.status})`));
      }
      return (await res.json()) as { code: string };
    },

    async listLeases() {
      return readJson<LeaseList>(await request("/v1/leases"));
    },
    async getLease(id: string) {
      return readJson<LeaseView>(await request(leasePath(id)));
    },
    async endLease(id: string) {
      return readJson<LeaseView>(
        await request(leasePath(id), { method: "DELETE" }),
      );
    },
    async forceEnd(id: string) {
      return readJson<LeaseView>(
        await request(`${leasePath(id)}/force`, { method: "POST" }),
      );
    },
    async quote(req: WizardLease) {
      return readJson<Quote>(
        await request("/v1/quote", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(leaseRequestBody(req)),
        }),
      );
    },
    async createLease(req: WizardLease) {
      return readJson<LeaseView>(
        await request("/v1/leases", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(leaseRequestBody(req)),
        }),
      );
    },
    async node() {
      return readNode(await request("/v1/node"), "node failed");
    },
    async park() {
      return readNode(
        await request("/v1/node/park", { method: "POST" }),
        "park failed",
      );
    },
    async unpark() {
      return readNode(
        await request("/v1/node/unpark", { method: "POST" }),
        "unpark failed",
      );
    },
    async preview(sessionId: string) {
      // Bearer stays on Authorization so iframe/img src never carry the token.
      // 204 has no Cache-Control; a cached empty would hide a later last_frame.
      const res = await request(previewPath(sessionId), { cache: "no-store" });
      if (res.status === 204) {
        return null;
      }
      if (!res.ok) {
        const payload = (await res.json().catch(() => null)) as {
          error?: string;
        } | null;
        throw new Error(payload?.error ?? `request failed (${res.status})`);
      }
      return res.blob();
    },
  };
}
