import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";

import { api } from "@/api/node";
import type { NodeView, Quote, WizardLease } from "@/api/types";
import { QuoteLabel } from "@/components/quote-label";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { getToken } from "@/lib/auth";
import { occupancyUsd, usdPerHour } from "@/lib/ledger";
import {
  UNPARKED_LEASE_COPY,
  quoteMatchesDraft,
  wizardMayPost,
  capacityHint,
  wizardWhatError,
  wizardWhereNextEnabled,
} from "@/lib/wizard";

const STEPS = ["What", "Where", "Review"] as const;

const WINDOWS_V01_COPY =
  "Windows is not supported in v0.1; only os=linux is available (Windows guests are not implemented)";
const MACOS_V01_COPY =
  "macOS is not supported in v0.1; only os=linux is available (macOS guests are not implemented)";
function parseCount(raw: string): number {
  const n = Number.parseInt(raw, 10);
  if (!Number.isFinite(n) || n < 0) return 0;
  return n;
}

function draftLease(
  density: WizardLease["density"],
  vcpu: number,
  memGib: number,
  diskGib: number,
): WizardLease {
  return {
    os: "linux",
    density,
    resources: { vcpu, mem_gib: memGib, disk_gib: diskGib },
  };
}

function Chip({ children }: { children: string }) {
  return (
    <span className="rounded-md border border-border bg-muted px-2 py-0.5 text-xs">
      {children}
    </span>
  );
}

export function LeaseWizardPage() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [density, setDensity] = useState<WizardLease["density"]>("isolated");
  const [vcpu, setVcpu] = useState(2);
  const [memGib, setMemGib] = useState(4);
  const [diskGib, setDiskGib] = useState(40);
  const [node, setNode] = useState<NodeView | null>(null);
  const [quote, setQuote] = useState<Quote | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nodePending, setNodePending] = useState(false);
  const [quotePending, setQuotePending] = useState(false);
  const [confirmPending, setConfirmPending] = useState(false);

  const whatError = wizardWhatError(vcpu, memGib, node?.capacity ?? null);
  const whatReady = whatError === null;
  const capacityNote = capacityHint(node?.capacity ?? null);
  const parked = node?.parked === true;
  const draft = {
    vcpu,
    mem_gib: memGib,
    disk_gib: diskGib,
    density,
  };
  const liveQuote =
    quote && quoteMatchesDraft(quote, draft) ? quote : null;

  const onAuthError = useCallback(() => {
    if (!getToken()) {
      navigate("/pair", { replace: true });
      return true;
    }
    return false;
  }, [navigate]);

  const refreshNode = useCallback(async () => {
    setNodePending(true);
    try {
      const next = await api().node();
      setNode(next);
      return next;
    } catch (err) {
      if (onAuthError()) return null;
      setError(err instanceof Error ? err.message : "load failed");
      return null;
    } finally {
      setNodePending(false);
    }
  }, [onAuthError]);

  useEffect(() => {
    let cancelled = false;
    setNodePending(true);
    void (async () => {
      try {
        const next = await api().node();
        if (cancelled) return;
        setNode(next);
      } catch (err) {
        if (cancelled) return;
        if (!getToken()) {
          navigate("/pair", { replace: true });
          return;
        }
        setError(err instanceof Error ? err.message : "load failed");
      } finally {
        if (!cancelled) setNodePending(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [navigate]);

  useEffect(() => {
    if (step !== 2) {
      setQuote(null);
      setQuotePending(false);
      return;
    }
    if (!whatReady) {
      setQuote(null);
      setQuotePending(false);
      return;
    }
    let cancelled = false;
    setQuote(null);
    setQuotePending(true);
    void (async () => {
      try {
        const next = await api().quote(
          draftLease(density, vcpu, memGib, diskGib),
        );
        if (cancelled) return;
        setQuote(next);
        setError(null);
      } catch (err) {
        if (cancelled) return;
        if (!getToken()) {
          navigate("/pair", { replace: true });
          return;
        }
        setQuote(null);
        setError(err instanceof Error ? err.message : "quote failed");
      } finally {
        if (!cancelled) setQuotePending(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [step, density, vcpu, memGib, diskGib, whatReady, navigate]);

  async function onNext() {
    if (step === 0) {
      if (whatError) {
        setError(whatError);
        return;
      }
      // Advancing clears whatever step one was complaining about; Where
      // re-fetches and will set its own error if there still is one.
      setError(null);
      setStep(1);
      return;
    }
    if (step === 1) {
      const next = await refreshNode();
      if (!next || !next.parked) {
        if (next && !next.parked) setError(UNPARKED_LEASE_COPY);
        return;
      }
      setError(null);
      setStep(2);
    }
  }

  async function onConfirm() {
    if (!wizardMayPost("linux") || !whatReady || !liveQuote) return;
    if (!parked) {
      setError(UNPARKED_LEASE_COPY);
      return;
    }
    setConfirmPending(true);
    setError(null);
    try {
      const lease = await api().createLease(
        draftLease(density, vcpu, memGib, diskGib),
      );
      navigate(`/leases/${lease.lease_id}`);
    } catch (err) {
      if (onAuthError()) return;
      try {
        setNode(await api().node());
      } catch {
        /* 401 redirects */
      }
      setError(err instanceof Error ? err.message : "create failed");
    } finally {
      setConfirmPending(false);
    }
  }

  const nextDisabled =
    confirmPending ||
    (step === 0 && !whatReady) ||
    (step === 1 && !wizardWhereNextEnabled(node, nodePending));
  const confirmDisabled =
    confirmPending ||
    quotePending ||
    !liveQuote ||
    !parked ||
    !whatReady ||
    !wizardMayPost("linux");

  return (
    <main className="mx-auto flex min-h-dvh max-w-xl flex-col gap-6 p-6">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-medium">New lease</h1>
        <Button asChild variant="outline">
          <Link to="/">Home</Link>
        </Button>
      </header>

      <p className="text-sm text-muted-foreground">
        {step + 1} / {STEPS.length} · {STEPS[step]}
      </p>

      {step === 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>What</CardTitle>
            <CardDescription>Linux guest on this node.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-6">
            <fieldset className="flex flex-col gap-2">
              <legend className="mb-1 text-sm font-medium">OS</legend>
              <label className="flex items-center gap-2 text-sm">
                <input type="radio" name="os" value="linux" defaultChecked />
                Linux
              </label>
              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                <input type="radio" name="os" value="windows" disabled />
                Windows
              </label>
              <p className="text-xs text-muted-foreground">{WINDOWS_V01_COPY}</p>
              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                <input type="radio" name="os" value="macos" disabled />
                macOS
              </label>
              <p className="text-xs text-muted-foreground">{MACOS_V01_COPY}</p>
            </fieldset>

            <fieldset className="flex flex-col gap-2">
              <legend className="mb-1 text-sm font-medium">Density</legend>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name="density"
                  checked={density === "isolated"}
                  onChange={() => setDensity("isolated")}
                />
                Isolated
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name="density"
                  checked={density === "shared"}
                  onChange={() => setDensity("shared")}
                />
                Shared
              </label>
            </fieldset>

            <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
              <label className="flex flex-col gap-2 text-sm">
                vCPU
                <Input
                  type="number"
                  min={1}
                  step={1}
                  value={vcpu}
                  onChange={(event) => setVcpu(parseCount(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-2 text-sm">
                mem GiB
                <Input
                  type="number"
                  min={1}
                  step={1}
                  value={memGib}
                  onChange={(event) => setMemGib(parseCount(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-2 text-sm">
                disk GiB
                <Input
                  type="number"
                  min={0}
                  step={1}
                  value={diskGib}
                  onChange={(event) =>
                    setDiskGib(parseCount(event.target.value))
                  }
                />
              </label>
            </div>
            {capacityNote ? (
              <p className="text-xs text-muted-foreground">{capacityNote}</p>
            ) : null}
            {whatError ? (
              <p className="text-sm text-destructive">{whatError}</p>
            ) : null}
          </CardContent>
        </Card>
      ) : null}

      {step === 1 ? (
        <Card>
          <CardHeader>
            <CardTitle>This node</CardTitle>
            <CardDescription>
              {node
                ? node.parked
                  ? "Parked"
                  : "Unparked"
                : nodePending
                  ? "Loading"
                  : "This node"}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {node ? (
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <dt className="text-muted-foreground">id</dt>
                <dd>local</dd>
                <dt className="text-muted-foreground">bind</dt>
                <dd>{node.bind}</dd>
                <dt className="text-muted-foreground">image</dt>
                <dd className="break-all">{node.image}</dd>
                <dt className="text-muted-foreground">parked</dt>
                <dd>{node.parked ? "yes" : "no"}</dd>
              </dl>
            ) : nodePending ? (
              <p className="text-sm text-muted-foreground">Loading node…</p>
            ) : null}
            {node && !node.parked ? (
              <p className="text-sm text-destructive">{UNPARKED_LEASE_COPY}</p>
            ) : null}
          </CardContent>
        </Card>
      ) : null}

      {step === 2 ? (
        <Card>
          <CardHeader>
            <CardTitle>Review</CardTitle>
            <CardDescription>
              Occupancy is quoted, not charged.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4 text-sm">
            <p>
              {vcpu} vCPU / {memGib} GiB / {diskGib} GiB · {density} · linux
            </p>
            {quotePending && !liveQuote ? (
              <p className="text-muted-foreground">Loading quote…</p>
            ) : null}
            {liveQuote ? (
              <>
                <p>
                  Occupancy{" "}
                  <QuoteLabel
                    usd={occupancyUsd(liveQuote)}
                    ratePerHour={usdPerHour(liveQuote)}
                  />
                </p>
                <p className="text-muted-foreground">
                  {liveQuote.min_seconds}s min
                </p>
              </>
            ) : null}
            {node ? (
              <div className="flex flex-col gap-2">
                <p className="text-muted-foreground">Allowlist</p>
                <div className="flex flex-wrap gap-2">
                  <Chip>{node.allowlist_source}</Chip>
                  {node.allowlist.map((host) => (
                    <Chip key={host}>{host}</Chip>
                  ))}
                </div>
              </div>
            ) : null}
            {node && !node.parked ? (
              <p className="text-destructive">{UNPARKED_LEASE_COPY}</p>
            ) : null}
          </CardContent>
        </Card>
      ) : null}

      {error ? <p className="text-sm text-destructive">{error}</p> : null}

      <div className="flex flex-wrap gap-2">
        {step > 0 ? (
          <Button
            type="button"
            variant="outline"
            disabled={confirmPending}
            onClick={() => {
              setError(null);
              setStep((current) => current - 1);
            }}
          >
            Back
          </Button>
        ) : null}
        {step < 2 ? (
          <Button type="button" disabled={nextDisabled} onClick={() => void onNext()}>
            Next
          </Button>
        ) : (
          <Button
            type="button"
            disabled={confirmDisabled}
            onClick={() => void onConfirm()}
          >
            Confirm
          </Button>
        )}
      </div>
    </main>
  );
}
