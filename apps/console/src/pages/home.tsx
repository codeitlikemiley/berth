import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";

import { api } from "@/api/node";
import type { LeaseView, NodeView } from "@/api/types";
import { MeterChart } from "@/components/meter-chart";
import { ParkControl } from "@/components/park-control";
import { QuoteLabel } from "@/components/quote-label";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { getToken } from "@/lib/auth";
import {
  FORCE_CONFIRM_COPY,
  incomeUsd,
  occupancyBadge,
  occupancyBadgeCopy,
  quotedUsd,
} from "@/lib/ledger";

function badgeClass(lease: LeaseView): string {
  const kind = occupancyBadge(lease);
  if (kind === "live") return "text-[var(--success)]";
  if (kind === "forfeited") return "text-destructive";
  return "text-muted-foreground";
}

function durationLabel(lease: LeaseView, nowUnix: number): string {
  if (lease.status === "stopped") {
    return `${lease.billable_seconds ?? lease.quote.min_seconds}s billable`;
  }
  if (!lease.live) {
    return `${lease.elapsed_seconds ?? lease.quote.min_seconds}s elapsed`;
  }
  const elapsed = Math.max(0, Math.floor(nowUnix - lease.started_at));
  return `${elapsed}s elapsed`;
}

function startedLabel(unix: number): string {
  return new Date(unix * 1000).toISOString().replace("T", " ").replace(/\.\d+Z$/, " UTC");
}

export function HomePage() {
  const navigate = useNavigate();
  const [leases, setLeases] = useState<LeaseView[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [node, setNode] = useState<NodeView | null>(null);
  const [nowUnix, setNowUnix] = useState(() => Date.now() / 1000);
  const [error, setError] = useState<string | null>(null);
  const [parkPending, setParkPending] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [list, n] = await Promise.all([api().listLeases(), api().node()]);
      setLeases(list.leases);
      setTruncated(list.truncated);
      setNode(n);
      setNowUnix(Date.now() / 1000);
      setError(null);
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, [navigate]);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      if (cancelled) return;
      await refresh();
    };
    void tick();
    const id = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [refresh]);

  async function onPark() {
    setParkPending(true);
    try {
      setNode(await api().park());
      setError(null);
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      try {
        setNode(await api().node());
      } catch {
        /* 401 redirects */
      }
      setError(err instanceof Error ? err.message : "park failed");
    } finally {
      setParkPending(false);
    }
  }

  async function onUnpark() {
    setParkPending(true);
    try {
      setNode(await api().unpark());
      setError(null);
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      // 409 while live: stay parked from the node round-trip.
      try {
        setNode(await api().node());
      } catch {
        /* 401 redirects */
      }
      setError(err instanceof Error ? err.message : "unpark failed");
    } finally {
      setParkPending(false);
    }
  }

  async function onEnd(id: string) {
    setBusyId(id);
    try {
      await api().endLease(id);
      await refresh();
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      setError(err instanceof Error ? err.message : "end failed");
    } finally {
      setBusyId(null);
    }
  }

  async function onForce(id: string) {
    if (!window.confirm(FORCE_CONFIRM_COPY)) return;
    setBusyId(id);
    try {
      await api().forceEnd(id);
      await refresh();
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      setError(err instanceof Error ? err.message : "force disconnect failed");
    } finally {
      setBusyId(null);
    }
  }

  const rows = leases ?? [];
  const hasLive = rows.some((lease) => lease.live);
  const empty = leases !== null && rows.length === 0;

  return (
    <main className="mx-auto flex min-h-dvh max-w-6xl flex-col gap-6 p-6">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-medium">Berth</h1>
        <div className="flex flex-wrap items-center gap-2">
          <ThemeToggle />
          <Button asChild variant="outline" size="sm">
            <Link to="/doctor">Doctor</Link>
          </Button>
          <Button asChild>
            <Link to="/leases/new">New lease</Link>
          </Button>
        </div>
      </header>

      {node ? (
        <ParkControl
          parked={node.parked}
          hasLive={hasLive}
          pending={parkPending}
          onPark={() => void onPark()}
          onUnpark={() => void onUnpark()}
        />
      ) : null}

      {error ? <p className="text-sm text-destructive">{error}</p> : null}

      {empty ? (
        <Card>
          <CardContent className="pt-6">
            <p className="text-muted-foreground">
              no leases; agents use MCP, or start the wizard.
            </p>
          </CardContent>
        </Card>
      ) : null}

      {rows.length > 0 ? (
        <>
          <MeterChart leases={rows} nowUnix={nowUnix} truncated={truncated} />
          <Card>
            <CardHeader>
              <CardTitle>Leases</CardTitle>
            </CardHeader>
            <CardContent className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead className="text-muted-foreground">
                  <tr>
                    <th className="pb-2 pr-3 font-medium">id</th>
                    <th className="pb-2 pr-3 font-medium">badge</th>
                    <th className="pb-2 pr-3 font-medium">session</th>
                    <th className="pb-2 pr-3 font-medium">started</th>
                    <th className="pb-2 pr-3 font-medium">billable/elapsed</th>
                    <th className="pb-2 pr-3 font-medium">quoted USD</th>
                    <th className="pb-2 pr-3 font-medium">income USD</th>
                    <th className="pb-2 font-medium">actions</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((lease) => {
                    const stopped = lease.status === "stopped";
                    const busy = busyId === lease.lease_id;
                    return (
                      <tr key={lease.lease_id} className="border-t border-border">
                        <td className="py-3 pr-3 align-top">
                          <Link
                            to={`/leases/${lease.lease_id}`}
                            className="text-primary underline-offset-4 hover:underline"
                          >
                            {lease.lease_id}
                          </Link>
                        </td>
                        <td className="py-3 pr-3 align-top">
                          <span className={badgeClass(lease)}>
                            {occupancyBadge(lease)}
                          </span>
                          {occupancyBadge(lease) === "forfeited" ? (
                            <span className="mt-1 block text-xs text-destructive">
                              {occupancyBadgeCopy(lease)}
                            </span>
                          ) : null}
                        </td>
                        <td className="py-3 pr-3 align-top">{lease.session_id}</td>
                        <td className="py-3 pr-3 align-top whitespace-nowrap">
                          {startedLabel(lease.started_at)}
                        </td>
                        <td className="py-3 pr-3 align-top">
                          {durationLabel(lease, nowUnix)}
                        </td>
                        <td className="py-3 pr-3 align-top">
                          <QuoteLabel usd={quotedUsd(lease, nowUnix)} />
                        </td>
                        <td className="py-3 pr-3 align-top">
                          <QuoteLabel usd={incomeUsd(lease, nowUnix)} />
                        </td>
                        <td className="py-3 align-top">
                          <div className="flex flex-wrap gap-2">
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              disabled={stopped || busy}
                              onClick={() => void onEnd(lease.lease_id)}
                            >
                              End
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              variant="destructive"
                              disabled={stopped || busy}
                              onClick={() => void onForce(lease.lease_id)}
                            >
                              Force disconnect
                            </Button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </CardContent>
          </Card>
        </>
      ) : null}
    </main>
  );
}
