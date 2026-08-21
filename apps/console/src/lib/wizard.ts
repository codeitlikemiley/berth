export const UNPARKED_LEASE_COPY =
  "Park this node before leasing (inventory is off).";

export function wizardWhatReady(vcpu: number, memGib: number): boolean {
  return vcpu > 0 && memGib > 0;
}

export function wizardWhereNextEnabled(parked: boolean): boolean {
  return parked;
}

export function wizardMayPost(os: string): boolean {
  return os === "linux";
}
