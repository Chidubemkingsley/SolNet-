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

// Fallback to deployed contract IDs if env vars not set. The fallback set
// follows the selected network, so switching networks in Settings just works.
function fallbackContracts(network: StellarNetwork): Record<ContractKey, string> {
  return { ...(DEPLOYED_CONTRACT_IDS[network] ?? DEPLOYED_CONTRACT_IDS.standalone) };
}

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
  const fallbacks = fallbackContracts(network);
  const contracts = Object.fromEntries(
    Object.entries(ENV_CONTRACTS).map(([key, value]) => [
      key,
      value?.trim() ?? fallbacks[key as ContractKey] ?? "",
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
    const resolvedNetwork = parseNetwork(parsed.network ?? defaults.network);
    // Stored contract IDs belong to the network they were saved under.
    // Applying testnet IDs to a standalone session (or vice versa) is exactly
    // how you end up calling contracts that don't exist there.
    const storedContracts =
      parsed.network === resolvedNetwork ? (parsed.contracts ?? {}) : {};
    return {
      ...defaults,
      ...parsed,
      network: resolvedNetwork,
      rpcUrl:
        typeof parsed.rpcUrl === "string" && parsed.rpcUrl.trim()
          ? parsed.rpcUrl.trim()
          : defaults.rpcUrl,
      contracts: {
        ...defaults.contracts,
        ...storedContracts,
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
  // Default to testnet, where the live deployment lives. Other networks
  // remain one dropdown change away in Settings.
  return value === "mainnet" || value === "standalone" ? value : "testnet";
}