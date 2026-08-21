export const UNPARKED_LEASE_COPY =
  "Park this node before leasing (inventory is off).";

export function wizardWhatReady(vcpu: number, memGib: number): boolean {
  return vcpu > 0 && memGib > 0;
}

/** Next on Where: retry after a failed load; block only a known-unparked node. */
export function wizardWhereNextEnabled(
  node: { parked: boolean } | null,
  pending: boolean,
): boolean {
  if (pending) return false;
  if (!node) return true;
  return node.parked;
}

export function wizardMayPost(os: string): boolean {
  return os === "linux";
}

export function quoteMatchesDraft(
  quote: {
    vcpu: number;
    mem_gib: number;
    disk_gib: number;
    density: string;
  },
  draft: {
    vcpu: number;
    mem_gib: number;
    disk_gib: number;
    density: string;
  },
): boolean {
  return (
    quote.vcpu === draft.vcpu &&
    quote.mem_gib === draft.mem_gib &&
    quote.disk_gib === draft.disk_gib &&
    quote.density === draft.density
  );
}
