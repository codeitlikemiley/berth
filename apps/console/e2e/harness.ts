import { type Page, expect } from "@playwright/test";

import type { LeaseView, NodeStatus, Quote } from "../src/api/types";

/**
 * These specs exist because every bug they cover shipped past a green unit
 * suite. The console's pure functions were tested; what was broken was the
 * behaviour on the page -- a dialog that blocked the event loop, a label that
 * claimed forfeiture over live income, a health panel that never re-read, a
 * route with no link to it. So the API is stubbed at the network boundary and
 * everything else is real: real bundle, real React, real focus and key
 * handling, real fetch.
 */

/** The rate the node actually quotes for the v0.1 default shape. */
export const QUOTE: Quote = {
  vcpu: 2,
  mem_gib: 4,
  disk_gib: 40,
  os: "linux",
  os_mult: 1,
  density: "isolated",
  density_mult: 1,
  term: "on_demand",
  min_seconds: 60,
  pooled: false,
  gas_per_second: "0.00134",
  currency: "gas",
  usd_per_gas: "0.01",
};

export const TOKEN = "brt_e2e_token";

export function lease(over: Partial<LeaseView> = {}): LeaseView {
  return {
    lease_id: "l_e2e_1",
    session_id: "s_e2e_1",
    ws_url: "ws://127.0.0.1:7432/v1/sessions/s_e2e_1",
    viewer_url: "http://127.0.0.1:6080/vnc.html",
    quote: QUOTE,
    status: "active",
    // Fixed so the accrued figure is the 60s minimum and never drifts mid-run.
    started_at: Math.floor(Date.now() / 1000),
    workspace_id: "ws_e2e",
    live: true,
    forfeited: false,
    ...over,
  };
}

export function node(over: Partial<NodeStatus> = {}): NodeStatus {
  return {
    ok: true,
    parked: true,
    bind: "127.0.0.1:7432",
    origin: null,
    class: "private",
    image: "berthos-linux-xfce:dev",
    allowlist: ["github.com", "pypi.org", "registry.npmjs.org"],
    allowlist_source: "default",
    docker: { ok: true, detail: "bollard ping ok" },
    guest_image: { ok: true, name: "berthos-linux-xfce:dev", detail: "egress filter v2" },
    home_writable: true,
    tunnel: { kind: "none" },
    active_bearers: 1,
    live_sessions: 1,
    shutting_down: false,
    capacity: { vcpu: 8, mem_gib: 7 },
    host_desktop_driven: false,
    ...over,
  };
}

/** Mutable server state a spec can rewrite between assertions. */
export type Backend = {
  node: NodeStatus;
  leases: LeaseView[];
  /** Set to fail every request, to stand in for a node that went away. */
  down: boolean;
  calls: string[];
};

export async function mount(
  page: Page,
  init: { node?: NodeStatus; leases?: LeaseView[]; authed?: boolean } = {},
): Promise<Backend> {
  const backend: Backend = {
    node: init.node ?? node(),
    leases: init.leases ?? [lease()],
    down: false,
    calls: [],
  };

  if (init.authed !== false) {
    // RequireAuth only asks whether a token exists, so seeding storage is the
    // whole of "already paired".
    await page.addInitScript((t) => {
      sessionStorage.setItem("berth.bearer", t as string);
    }, TOKEN);
  }

  await page.route("**/v1/**", async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const path = url.pathname;
    backend.calls.push(`${req.method()} ${path}`);

    if (backend.down) {
      await route.abort("connectionrefused");
      return;
    }
    const json = async (status: number, body: unknown) =>
      route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

    if (path === "/v1/node" && req.method() === "GET") return json(200, backend.node);
    if (path === "/v1/node/park") {
      backend.node = { ...backend.node, parked: true };
      return json(200, backend.node);
    }
    if (path === "/v1/node/unpark") {
      const live = backend.leases.find((l) => l.live);
      if (live) {
        return json(409, {
          error: "cannot unpark while a lease is live",
          live_lease_id: live.lease_id,
        });
      }
      backend.node = { ...backend.node, parked: false };
      return json(200, backend.node);
    }
    if (path === "/v1/leases" && req.method() === "GET") {
      return json(200, { leases: backend.leases, truncated: false });
    }
    if (path === "/v1/quote") return json(200, { quote: QUOTE });
    if (path === "/v1/pairing") return json(200, { code: "4X8P-U59W" });
    if (path === "/v1/pair") return json(200, { token: TOKEN });

    const force = path.match(/^\/v1\/leases\/([^/]+)\/force$/);
    if (force) {
      backend.leases = backend.leases.map((l) =>
        l.lease_id === force[1]
          ? { ...l, live: false, status: "stopped", forfeited: true, end_reason: "forced", billable_seconds: 60 }
          : l,
      );
      return json(200, backend.leases.find((l) => l.lease_id === force[1]));
    }
    const one = path.match(/^\/v1\/leases\/([^/]+)$/);
    if (one) {
      const found = backend.leases.find((l) => l.lease_id === one[1]);
      if (!found) return json(404, { error: "not found" });
      if (req.method() === "DELETE") {
        backend.leases = backend.leases.map((l) =>
          l.lease_id === one[1]
            ? { ...l, live: false, status: "stopped", end_reason: "graceful", billable_seconds: 60 }
            : l,
        );
        return json(200, backend.leases.find((l) => l.lease_id === one[1]));
      }
      return json(200, found);
    }
    return json(404, { error: "not found" });
  });

  return backend;
}

/** The dialog is the thing a native confirm() could never be. */
export async function expectDialogOpen(page: Page, title: string) {
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute("aria-modal", "true");
  await expect(dialog.getByRole("heading", { name: title })).toBeVisible();
  return dialog;
}
