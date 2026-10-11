// Per-currency settlement stacks on testnet - DO NOT EDIT MANUALLY
// Each stack is one account_ledger + billing + disconnector bound to a single
// backing token. The other nine contracts are shared across all currencies
// (see deployed-contracts.testnet.ts, which carries the USDC stack).

import type { ContractKey } from "./contracts";

export type StackKey = "account_ledger" | "micro_billing" | "delinquency_disconnector";

export const STACK_KEYS: readonly StackKey[] = [
  "account_ledger",
  "micro_billing",
  "delinquency_disconnector",
] as const;

// USDC is the base testnet deployment, so it has no overlay here.
export const STACK_OVERLAYS: Record<string, Record<StackKey, string>> = {
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
