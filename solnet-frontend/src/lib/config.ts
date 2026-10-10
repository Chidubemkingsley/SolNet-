import type { ContractKey } from "./contracts";
import { DEPLOYED_CONTRACT_IDS } from "./deployed-contracts";
import {
  NETWORKS,
  type StellarConnectionConfig,
  type StellarNetwork,
} from "./helper";

export const SETTINGS_STORAGE_KEY = "solnet.protocol.settings.v1";

export interface SolNetSettings extends StellarConnectionConfig {
  contracts: Record<ContractKey, string>;
}

// Fallback to deployed contract IDs if env vars not set
const FALLBACK_CONTRACTS: Record<ContractKey, string> = {
  grid_topology: DEPLOYED_CONTRACT_IDS.grid_topology,
  account_ledger: DEPLOYED_CONTRACT_IDS.account_ledger,
  micro_billing: DEPLOYED_CONTRACT_IDS.micro_billing,
  delinquency_disconnector: DEPLOYED_CONTRACT_IDS.delinquency_disconnector,
  yield_auction: DEPLOYED_CONTRACT_IDS.yield_auction,
  peak_load_adjuster: DEPLOYED_CONTRACT_IDS.peak_load_adjuster,
  battery_pool: DEPLOYED_CONTRACT_IDS.battery_pool,
  community_capital: DEPLOYED_CONTRACT_IDS.community_capital,
  iot_registrar: DEPLOYED_CONTRACT_IDS.iot_registrar,
  cryptographic_oracle: DEPLOYED_CONTRACT_IDS.cryptographic_oracle,
  firmware_verifier: DEPLOYED_CONTRACT_IDS.firmware_verifier,
  hardware_slash: DEPLOYED_CONTRACT_IDS.hardware_slash,
};

const ENV_CONTRACTS: Record<ContractKey, string | undefined> = {
  grid_topology: import.meta.env.VITE_CONTRACT_GRID_TOPOLOGY,
  account_ledger: import.meta.env.VITE_CONTRACT_ACCOUNT_LEDGER,
  micro_billing: import.meta.env.VITE_CONTRACT_MICRO_BILLING,
  delinquency_disconnector: import.meta.env.VITE_CONTRACT_DELINQUENCY_DISCONNECTOR,
  yield_auction: import.meta.env.VITE_CONTRACT_YIELD_AUCTION,
  peak_load_adjuster: import.meta.env.VITE_CONTRACT_PEAK_LOAD_ADJUSTER,
  battery_pool: import.meta.env.VITE_CONTRACT_BATTERY_POOL,
  community_capital: import.meta.env.VITE_CONTRACT_COMMUNITY_CAPITAL,
  iot_registrar: import.meta.env.VITE_CONTRACT_IOT_REGISTRAR,
  cryptographic_oracle: import.meta.env.VITE_CONTRACT_CRYPTOGRAPHIC_ORACLE,
  firmware_verifier: import.meta.env.VITE_CONTRACT_FIRMWARE_VERIFIER,
  hardware_slash: import.meta.env.VITE_CONTRACT_HARDWARE_SLASH,
};

export function getDefaultSettings(): SolNetSettings {
  const network = parseNetwork(import.meta.env.VITE_STELLAR_NETWORK);
  const rpcUrl =
    import.meta.env.VITE_STELLAR_RPC_URL?.trim() || NETWORKS[network].defaultRpcUrl;
  const contracts = Object.fromEntries(
    Object.entries(ENV_CONTRACTS).map(([key, value]) => [
      key,
      value?.trim() ?? FALLBACK_CONTRACTS[key as ContractKey] ?? "",
    ]),
  ) as Record<ContractKey, string>;
  return {
    network,
    rpcUrl,
    contracts,
  };
}

export function loadSettings(): SolNetSettings {
  const defaults = getDefaultSettings();
  try {
    const stored = localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!stored) return defaults;
    const parsed = JSON.parse(stored) as Partial<SolNetSettings>;
    return {
      ...defaults,
      ...parsed,
      network: parseNetwork(parsed.network ?? defaults.network),
      rpcUrl:
        typeof parsed.rpcUrl === "string" && parsed.rpcUrl.trim()
          ? parsed.rpcUrl.trim()
          : defaults.rpcUrl,
      contracts: {
        ...defaults.contracts,
        ...(parsed.contracts ?? {}),
      },
    };
  } catch {
    return defaults;
  }
}

export function saveSettings(settings: SolNetSettings): void {
  localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
}

function parseNetwork(value: unknown): StellarNetwork {
  return value === "mainnet" || value === "standalone" ? value : "testnet";
}