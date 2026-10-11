// Per-currency settlement stacks on testnet - DO NOT EDIT MANUALLY
// Each stack is one account_ledger + billing + disconnector bound to a single
// backing token. The other nine contracts are shared across all currencies.

import type { ContractKey } from "./contracts";

export type StackKey = "account_ledger" | "micro_billing" | "delinquency_disconnector";

export const STACK_KEYS: readonly StackKey[] = [
  "account_ledger",
  "micro_billing",
  "delinquency_disconnector",
] as const;

// SNUSD is the base testnet deployment, so it has no overlay here.
export const STACK_OVERLAYS: Record<string, Record<StackKey, string>> = {
  SNEUR: {
    account_ledger: "CC2FLOLU4AGGPQZYNBAKENNRELSXSU56BHFLHMPNXMV6EEIUVCVZI4G2",
    micro_billing: "CCQ6RBXRTV4PW46PY7NWUKM4RILUL3J6INK3IG4UVLVYONGAVNT2EDSY",
    delinquency_disconnector:
      "CBVDIPDZZULUUIUOKC5NUNB76J2LVG4YN5PFDTFMZ3UECMNETFW6542Z",
  },
  USDC: {
    account_ledger: "CDO63AUAFUFZBILM7BWR3QN6Q6VUYZSVOSXZDZ55PHAFUS7ISWNGLPLI",
    micro_billing: "CBCLTR5LH77QWXVDBKEA2L5TRR3GXYTT6GTP7AZZ72YMQ3EWXZZYILBN",
    delinquency_disconnector:
      "CD6MBK5SGL5K7JVW5WFVI2R5XM353LDMXVKZXCZRIKVAX6FK66TGYD35",
  },
  EURC: {
    account_ledger: "CBEYQBMOVVON2CYM6BZGNEPHUPQ3MTWRN6KAXQUTKHTIC7W3TSYPIDDB",
    micro_billing: "CCEBP4NCOOH3H5SO77XGWV3JN7EB4IMPC7CY3SAQCUVL6Q4SJOYEUQPY",
    delinquency_disconnector:
      "CCDGMEK3QTW2ZQCRZY6IMXX32JDSNBD4VNY5QFIHLUGQMPAOMKG2DFOA",
  },
};

export function stackOverlayFor(
  currency: string,
): Record<StackKey, string> | undefined {
  return STACK_OVERLAYS[currency];
}

export function isStackKey(key: string): key is StackKey {
  return (STACK_KEYS as readonly string[]).includes(key);
}
