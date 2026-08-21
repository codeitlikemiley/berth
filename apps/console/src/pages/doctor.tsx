import { useEffect, useRef, useState } from "react";
import { Link, Navigate } from "react-router-dom";

import { api, type NodeStatus } from "@/api/node";
import { DoctorList } from "@/components/doctor-list";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { getToken, refreshRememberedToken, url_is_loopback } from "@/lib/auth";

export function DoctorPage() {
  const loopback = url_is_loopback(window.location.origin);
  const codeRef = useRef<HTMLInputElement>(null);
  const [node, setNode] = useState<NodeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const next = await api().node();
        if (!cancelled) {
          setNode(next);
        }
      } catch (err) {
        if (cancelled) {
          return;
        }
        if (!getToken()) {
          setError("unauthorized");
          return;
        }
        setError(err instanceof Error ? err.message : "doctor failed");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (!getToken() || error === "unauthorized") {
    return <Navigate to="/pair" replace />;
  }

  async function withNode(
    run: () => Promise<NodeStatus>,
    failed: string,
  ): Promise<void> {
    setError(null);
    setNotice(null);
    setPending(true);
    try {
      setNode(await run());
    } catch (err) {
      if (!getToken()) {
        setError("unauthorized");
        return;
      }
      setError(err instanceof Error ? err.message : failed);
    } finally {
      setPending(false);
    }
  }

  async function onRevoke() {
    if (
      !window.confirm(
        "Revoke other clients? CLI and other browsers must pair again.",
      )
    ) {
      return;
    }
    setError(null);
    setNotice(null);
    setPending(true);
    try {
      let code = "";
      if (loopback) {
        // fetched only to POST; never rendered on this page
        code = (await api().pairingCode())?.code ?? "";
        if (!code) {
          throw new Error("pairing code is not available on this origin");
        }
      } else {
        code = codeRef.current?.value.trim() ?? "";
        if (!code) {
          throw new Error("pairing code required");
        }
      }
      await api().pair(code, { revokeOthers: true });
      const next = getToken();
      if (next) {
        refreshRememberedToken(next);
      }
      if (codeRef.current) {
        codeRef.current.value = "";
      }
      setNode(await api().node());
      setNotice("Other clients revoked. CLI must re-pair.");
    } catch (err) {
      if (!getToken()) {
        setError("unauthorized");
        return;
      }
      setError(err instanceof Error ? err.message : "revoke failed");
    } finally {
      setPending(false);
    }
  }

  return (
    <main className="mx-auto flex min-h-dvh max-w-xl flex-col gap-4 p-6">
      <p className="text-sm">
        <Link to="/" className="text-muted-foreground hover:underline">
          Home
        </Link>
      </p>
      <Card>
        <CardHeader>
          <CardTitle>Doctor</CardTitle>
          <CardDescription>
            Node health from this process. Host desktop is never driven.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-6">
          {node ? (
            <DoctorList
              node={node}
              pending={pending}
              onPark={() => void withNode(() => api().park(), "park failed")}
              onUnpark={() =>
                void withNode(() => api().unpark(), "unpark failed")
              }
            />
          ) : (
            <p className="text-sm text-muted-foreground">Loading</p>
          )}
          {error && error !== "unauthorized" ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : null}
          {notice ? <p className="text-sm">{notice}</p> : null}
          <div className="flex flex-col gap-3 border-t border-border pt-4">
            <p className="text-sm text-muted-foreground">
              Revoke signs out CLI and other browsers. This browser stays
              paired.
            </p>
            {!loopback ? (
              <label className="flex flex-col gap-2 text-sm">
                Pairing code
                <Input
                  ref={codeRef}
                  type="password"
                  name="revoke-code"
                  autoComplete="off"
                  autoCorrect="off"
                  spellCheck={false}
                  disabled={pending}
                />
              </label>
            ) : null}
            <Button
              type="button"
              variant="destructive"
              disabled={pending}
              onClick={() => void onRevoke()}
            >
              Revoke other clients
            </Button>
          </div>
        </CardContent>
      </Card>
    </main>
  );
}
