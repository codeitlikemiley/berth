export type AccountRole = "owner" | "operator" | "billing";

export type Invoice = {
  id: string;
  amount_usd: number;
};

export type Payout = {
  id: string;
  amount_usd: number;
};

export type NodeRegistration = {
  id: string;
  name: string;
};
