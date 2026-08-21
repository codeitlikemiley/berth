import type { ReactNode } from "react";

import type { NodeStatus } from "@/api/types";
import { Button } from "@/components/ui/button";

function originHost(origin: string | null): string {
  if (!origin) {
    return "none";
  }
  try {
    return new URL(origin).hostname || "none";
  } catch {
    // unknown origin strings are not shown; they must never include TUNNEL_TOKEN
    return "none";
  }
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:gap-4">
      <dt className="text-sm text-muted-foreground sm:w-36 sm:shrink-0">{label}</dt>
      <dd className="min-w-0 text-sm">{children}</dd>
    </div>
  );
}

function okText(ok: boolean): string {
  return ok ? "ok" : "fail";
}

export function DoctorList({
  node,
  pending,
  onPark,
  onUnpark,
}: {
  node: NodeStatus;
  pending: boolean;
  onPark: () => void;
  onUnpark: () => void;
}) {
  const tunnel =
    node.tunnel.kind === "cloudflare"
      ? `cloudflare named=${node.tunnel.named ? "yes" : "no"} child_alive=${node.tunnel.child_alive ? "yes" : "no"}`
      : "none";

  return (
    <dl className="flex flex-col gap-3">
      <Row label="docker">
        <span className={node.docker.ok ? undefined : "text-destructive"}>
          {okText(node.docker.ok)} {node.docker.detail}
        </span>
      </Row>
      <Row label="guest image">
        <span className={node.guest_image.ok ? undefined : "text-destructive"}>
          {okText(node.guest_image.ok)} {node.guest_image.name}
        </span>
      </Row>
      <Row label="home writable">
        <span className={node.home_writable ? undefined : "text-destructive"}>
          {okText(node.home_writable)}
        </span>
      </Row>
      <Row label="parked">
        <div className="flex flex-wrap items-center gap-2">
          <span>{node.parked ? "parked" : "unparked"}</span>
          {node.parked ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={pending}
              onClick={onUnpark}
            >
              Unpark
            </Button>
          ) : (
            <Button type="button" size="sm" disabled={pending} onClick={onPark}>
              Park
            </Button>
          )}
        </div>
      </Row>
      <Row label="allowlist">
        <div className="flex flex-wrap items-center gap-1">
          {node.allowlist.length === 0 ? (
            <span className="text-muted-foreground">none</span>
          ) : (
            node.allowlist.map((domain) => (
              <span
                key={domain}
                className="rounded-md border border-border px-2 py-0.5 text-xs"
              >
                {domain}
              </span>
            ))
          )}
          <span className="text-xs text-muted-foreground">{node.allowlist_source}</span>
        </div>
      </Row>
      <Row label="bind">{node.bind}</Row>
      <Row label="origin">{originHost(node.origin)}</Row>
      <Row label="tunnel">{tunnel}</Row>
      <Row label="active bearers">{String(node.active_bearers)}</Row>
      <Row label="host desktop">host desktop is never driven</Row>
    </dl>
  );
}
