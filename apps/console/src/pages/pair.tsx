import { type FormEvent, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api } from "@/api/node";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { rememberToken, url_is_loopback } from "@/lib/auth";

export function PairPage() {
  const navigate = useNavigate();
  const loopback = url_is_loopback(window.location.origin);
  const [code, setCode] = useState("");
  const [remember, setRemember] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setPending(true);
    try {
      const { token } = await api().pair(code.trim());
      if (remember && loopback) {
        rememberToken(token);
      }
      navigate("/", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "pairing failed");
    } finally {
      setPending(false);
    }
  }

  return (
    <main className="mx-auto flex min-h-dvh max-w-md items-center p-6">
      <Card className="w-full">
        <CardHeader>
          <CardTitle>Pair this browser</CardTitle>
          <CardDescription>
            Enter the pairing code printed by the node.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit} className="flex flex-col gap-4">
            <label className="flex flex-col gap-2 text-sm">
              Code
              <Input
                name="code"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                placeholder="ABCD-EFGH"
                required
                autoComplete="off"
                autoCorrect="off"
                spellCheck={false}
              />
            </label>
            {loopback ? (
              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(event) => setRemember(event.target.checked)}
                />
                Remember this browser
              </label>
            ) : null}
            {error ? (
              <p className="text-sm text-destructive">{error}</p>
            ) : null}
            <Button type="submit" disabled={pending}>
              Pair
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}
