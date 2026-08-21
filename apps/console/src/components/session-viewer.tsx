import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api } from "@/api/node";
import { getToken } from "@/lib/auth";
import { canEmbedViewer } from "@/lib/viewer";

const EMPTY_COPY = "use MCP / open the viewer on the parked box";
const GONE_COPY = "no live guest";

export function SessionViewer({
  sessionId,
  viewerUrl,
  live,
}: {
  sessionId: string;
  viewerUrl?: string | null;
  live: boolean;
}) {
  if (!live) {
    return <p className="text-muted-foreground">{GONE_COPY}</p>;
  }

  if (viewerUrl && canEmbedViewer(window.location.origin, viewerUrl)) {
    return (
      <iframe
        src={viewerUrl}
        title="Guest desktop"
        className="aspect-[8/5] w-full bg-muted"
      />
    );
  }

  return <LastFrame sessionId={sessionId} />;
}

function LastFrame({ sessionId }: { sessionId: string }) {
  const navigate = useNavigate();
  const [frameUrl, setFrameUrl] = useState<string | null>(null);
  const [empty, setEmpty] = useState(false);
  const [gone, setGone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;
    let timer: number | undefined;

    const forgetFrame = () => {
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
        objectUrl = null;
      }
      setFrameUrl(null);
    };

    const tick = async () => {
      try {
        const blob = await api().preview(sessionId);
        if (cancelled) return;
        if (!getToken()) {
          navigate("/pair", { replace: true });
          return;
        }
        if (blob === null) {
          forgetFrame();
          setEmpty(true);
          setGone(false);
          setError(null);
          return;
        }
        const next = URL.createObjectURL(blob);
        if (cancelled) {
          URL.revokeObjectURL(next);
          return;
        }
        if (objectUrl) URL.revokeObjectURL(objectUrl);
        objectUrl = next;
        setFrameUrl(next);
        setEmpty(false);
        setGone(false);
        setError(null);
      } catch (err) {
        if (cancelled) return;
        if (!getToken()) {
          navigate("/pair", { replace: true });
          return;
        }
        forgetFrame();
        const message = err instanceof Error ? err.message : "preview failed";
        if (message === "not found") {
          setGone(true);
          setEmpty(false);
          setError(null);
        } else {
          setGone(false);
          setEmpty(false);
          setError(message);
        }
      } finally {
        if (!cancelled) {
          timer = window.setTimeout(() => void tick(), 2000);
        }
      }
    };

    void tick();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [sessionId, navigate]);

  if (gone) {
    return <p className="text-muted-foreground">{GONE_COPY}</p>;
  }
  if (error) {
    return <p className="text-sm text-destructive">{error}</p>;
  }
  if (empty) {
    return <p className="text-muted-foreground">{EMPTY_COPY}</p>;
  }
  if (frameUrl) {
    return (
      <img
        src={frameUrl}
        alt="Last guest frame"
        className="max-h-[50rem] w-full object-contain"
      />
    );
  }
  return null;
}
