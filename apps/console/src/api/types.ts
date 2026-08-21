export type ConsoleMode = "node" | "control-plane";

export type PairResponse = {
  token: string;
};

export type BerthApi = {
  pair: (code: string) => Promise<PairResponse>;
};
