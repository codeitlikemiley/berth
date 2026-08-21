import { useState } from "react";

import { Button } from "@/components/ui/button";

export function PairingCode({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <p className="select-all font-mono text-lg tracking-wide">{code}</p>
      <Button type="button" variant="outline" size="sm" onClick={() => void copy()}>
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}
