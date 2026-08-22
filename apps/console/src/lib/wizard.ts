export const UNPARKED_LEASE_COPY =
  "Park this node before leasing (inventory is off).";

export const INVALID_RESOURCES_COPY =
  "vcpu and mem_gib must be greater than zero (0 is not unlimited)";

/** `null` on a field means the node could not ask Docker, not that it has none. */
export type NodeCapacity = { vcpu: number | null; mem_gib: number | null };

/**
 * Step-one gate: the message to show, or null when the draft is fine.
 *
 * The node refuses an oversized lease anyway, but only at Confirm -- after the
 * wizard has walked you through Where and priced something it was never going
 * to build. It knows its own capacity from the start, so ask here.
 *
 * Unknown capacity does not block. A failed probe is not evidence the request
 * is too big, and create_lease re-checks server-side regardless; this only
 * moves a refusal we can already make earlier.
 */
export function wizardWhatError(
  vcpu: number,
  memGib: number,
  capacity: NodeCapacity | null,
): string | null {
  if (vcpu <= 0 || memGib <= 0) return INVALID_RESOURCES_COPY;
  // Worded to match the node's own refusal, so the two surfaces agree.
  if (capacity?.vcpu != null && vcpu > capacity.vcpu) {
    return `vcpu ${vcpu} exceeds this node's ${capacity.vcpu} available CPUs`;
  }
  if (capacity?.mem_gib != null && memGib > capacity.mem_gib) {
    return `mem_gib ${memGib} exceeds this node's ${capacity.mem_gib} GiB of memory`;
  }
  return null;
}

/** What this node can host, for showing the bound before anyone hits it. */
export function capacityHint(capacity: NodeCapacity | null): string | null {
  if (capacity?.vcpu == null && capacity?.mem_gib == null) return null;
  const parts: string[] = [];
  if (capacity?.vcpu != null) parts.push(`${capacity.vcpu} vCPU`);
  if (capacity?.mem_gib != null) parts.push(`${capacity.mem_gib} GiB`);
  return `this node has ${parts.join(" · ")}`;
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
