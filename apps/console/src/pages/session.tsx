import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import { api } from "@/api/node";
import type { LeaseView } from "@/api/types";
import { QuoteLabel } from "@/components/quote-label";
import { SessionViewer } from "@/components/session-viewer";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/dialog";
import { getToken } from "@/lib/auth";
import {
  FORCE_CONFIRM_COPY,
  incomeUsd,
  occupancyBadge,
  occupancyBadgeCopy,
  quotedUsd,
  usdPerHour,
} from "@/lib/ledger";

function badgeClass(lease: LeaseView): string {
  const kind = occupancyBadge(lease);
  if (kind === "live") return "text-[var(--success)]";
  if (kind === "forfeited") return "text-destructive";
  return "text-muted-foreground";
}

export function SessionPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const [lease, setLease] = useState<LeaseView | null>(null);
  const [nowUnix, setNowUnix] = useState(() => Date.now() / 1000);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const genRef = useRef(0);
  const busyRef = useRef(false);

  const refresh = useCallback(async () => {
    if (!id) return;
    const gen = ++genRef.current;
    try {
      const next = await api().getLease(id);
      if (gen !== genRef.current) return;
      setLease(next);
      setNowUnix(Date.now() / 1000);
      setError(null);
    } catch (err) {
      if (gen !== genRef.current) return;
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, [id, navigate]);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      if (cancelled || busyRef.current) return;
      await refresh();
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [refresh]);

  async function runStop(
    action: (leaseId: string) => Promise<LeaseView>,
    fail: string,
  ) {
    if (!id) return;
    busyRef.current = true;
    setBusy(true);
    // Guest stop can outlive a GET that still saw live; drop that result.
    genRef.current += 1;
    try {
      await action(id);
      await refresh();
    } catch (err) {
      if (!getToken()) {
        navigate("/pair", { replace: true });
        return;
      }
      setError(err instanceof Error ? err.message : fail);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  async function onEnd() {
    await runStop((leaseId) => api().endLease(leaseId), "end failed");
  }

  async function onForce() {
    await runStop((leaseId) => api().forceEnd(leaseId), "force disconnect failed");
  }

  const stopped = lease?.status === "stopped";

  return (
    <main className="mx-auto flex min-h-dvh max-w-6xl flex-col gap-6 p-6">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <Link
            to="/"
            className="text-sm text-muted-foreground underline-offset-4 hover:underline"
          >
            Leases
          </Link>
          <h1 className="text-xl font-medium">{id ?? "Lease"}</h1>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!lease || stopped || busy}
            onClick={() => void onEnd()}
          >
            End
          </Button>
          <Button
            type="button"
            size="sm"
            variant="destructive"
            disabled={!lease || stopped || busy}
            onClick={() => setConfirmOpen(true)}
          >
            Force disconnect
          </Button>
        </div>
      </header>

      {error ? <p className="text-sm text-destructive">{error}</p> : null}

      {lease ? (
        <>
          <Card>
            <CardHeader>
              <CardTitle className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                <span>{lease.lease_id}</span>
                <span className={badgeClass(lease)}>{occupancyBadge(lease)}</span>
              </CardTitle>
              <CardDescription>{lease.session_id}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3 text-sm">
              {occupancyBadge(lease) === "forfeited" ? (
                <p className="text-destructive">{occupancyBadgeCopy(lease)}</p>
              ) : null}
              <p>
                Quoted{" "}
                <QuoteLabel
                  usd={quotedUsd(lease, nowUnix)}
                  ratePerHour={usdPerHour(lease.quote)}
                />
              </p>
              <p>
                Income{" "}
                <QuoteLabel
                  usd={incomeUsd(lease, nowUnix)}
                  ratePerHour={usdPerHour(lease.quote)}
                />
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Viewer</CardTitle>
            </CardHeader>
            <CardContent>
              <SessionViewer
                sessionId={lease.session_id}
                viewerUrl={lease.viewer_url}
                live={lease.live}
              />
            </CardContent>
          </Card>
        </>
      ) : null}

      <ConfirmDialog
        open={confirmOpen}
        title="Force disconnect"
        description={FORCE_CONFIRM_COPY}
        confirmLabel="Force disconnect"
        confirmVariant="destructive"
        onCancel={() => setConfirmOpen(false)}
        onConfirm={() => {
          setConfirmOpen(false);
          void onForce();
        }}
      />
    </main>
  );
}
