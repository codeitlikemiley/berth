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
  // docker ping on GET /v1/node can outlast revoke; ignore stale snapshots
  const genRef = useRef(0);
  const [node, setNode] = useState<NodeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pending, setPending] = useState(true);
  const [unauthed, setUnauthed] = useState(false);

  useEffect(() => {
    const gen = ++genRef.current;
    setPending(true);
    void (async () => {
      try {
        const next = await api().node();
        if (gen !== genRef.current) {
          return;
        }
        setNode(next);
        setError(null);
      } catch (err) {
        if (gen !== genRef.current) {
          return;
        }
        if (!getToken()) {
          setUnauthed(true);
          return;
        }
        setError(err instanceof Error ? err.message : "doctor failed");
      } finally {
        if (gen === genRef.current) {
          setPending(false);
        }
      }
    })();
    return () => {
      genRef.current += 1;
    };
  }, []);

  if (unauthed || !getToken()) {
    return <Navigate to="/pair" replace />;
  }

  async function withNode(
    run: () => Promise<NodeStatus>,
    failed: string,
  ): Promise<void> {
    const gen = ++genRef.current;
    setError(null);
    setNotice(null);
    setPending(true);
    try {
      const next = await run();
      if (gen !== genRef.current) {
        return;
      }
      setNode(next);
    } catch (err) {
      if (gen !== genRef.current) {
        return;
      }
      if (!getToken()) {
        setUnauthed(true);
        return;
      }
      setError(err instanceof Error ? err.message : failed);
    } finally {
      if (gen === genRef.current) {
        setPending(false);
      }
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
    const gen = ++genRef.current;
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
      const nextToken = getToken();
      if (nextToken) {
        refreshRememberedToken(nextToken);
      }
      if (codeRef.current) {
        codeRef.current.value = "";
      }
      const next = await api().node();
      if (gen !== genRef.current) {
        return;
      }
      setNode(next);
      setNotice("Other clients revoked. CLI must re-pair.");
    } catch (err) {
      if (gen !== genRef.current) {
        return;
      }
      if (!getToken()) {
        setUnauthed(true);
        return;
      }
      setError(err instanceof Error ? err.message : "revoke failed");
    } finally {
      if (gen === genRef.current) {
        setPending(false);
      }
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
          ) : error ? (
            <div className="flex flex-col items-start gap-2">
              <p className="text-sm">Could not load node</p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={pending}
                onClick={() =>
                  void withNode(() => api().node(), "doctor failed")
                }
              >
                Retry
              </Button>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">Loading</p>
          )}
          {error ? <p className="text-sm text-destructive">{error}</p> : null}
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
