import type {
  BerthApi,
  ConsoleMode,
  LeaseList,
  LeaseView,
  NodeView,
} from "./types";

export type { BerthApi, ConsoleMode } from "./types";

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
      opts.setToken(null);
    }
    return res;
  }

  function leasePath(id: string): string {
    return `/v1/leases/${encodeURIComponent(id)}`;
  }

  return {
    async pair(code: string) {
      const res = await request("/v1/pair", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ code }),
      });
      if (res.status === 401) {
        throw new Error("invalid pairing code");
      }
      if (!res.ok) {
        const payload = (await res.json().catch(() => null)) as {
          error?: string;
        } | null;
        throw new Error(payload?.error ?? `pair failed (${res.status})`);
      }
      const data = (await res.json()) as { token: string };
      opts.setToken(data.token);
      return data;
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
    async node() {
      return readJson<NodeView>(await request("/v1/node"));
    },
    async park() {
      return readJson<NodeView>(
        await request("/v1/node/park", { method: "POST" }),
      );
    },
    async unpark() {
      return readJson<NodeView>(
        await request("/v1/node/unpark", { method: "POST" }),
      );
    },
  };
}
