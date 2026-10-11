import type { ContractArgument, SorobanArgumentType } from "./helper";

export type ContractKey =
  | "grid_topology"
  | "account_ledger"
  | "micro_billing"
  | "delinquency_disconnector"
  | "yield_auction"
  | "peak_load_adjuster"
  | "battery_pool"
  | "community_capital"
  | "iot_registrar"
  | "cryptographic_oracle"
  | "firmware_verifier"
  | "hardware_slash";

export interface ContractMethod {
  name: string;
  access: "read" | "write" | "unknown";
  args?: { name: string; type: SorobanArgumentType }[];
}

export interface ContractDefinition {
  key: ContractKey;
  label: string;
  description: string;
  methods: ContractMethod[];
}

const method = (
  name: string,
  access: ContractMethod["access"],
  args?: ContractMethod["args"],
): ContractMethod => ({ name, access, args });

const address = (name: string) => ({ name, type: "address" as const });
const bytes32 = (name: string) => ({ name, type: "bytes32" as const });
const u32 = (name: string) => ({ name, type: "u32" as const });
const u64 = (name: string) => ({ name, type: "u64" as const });
const i128 = (name: string) => ({ name, type: "i128" as const });
const u128 = (name: string) => ({ name, type: "u128" as const });
const bytes = (name: string) => ({ name, type: "bytes" as const });

export const CONTRACTS: ContractDefinition[] = [
  {
    key: "grid_topology",
    label: "Grid Topology",
    description: "Manages the physical topology map of the microgrid nodes and connections.",
    methods: [
      method("init", "write", [address("admin")]),
      method("admin", "read"),
      method("register_node", "write", [address("admin"), u32("id"), u32("kind"), i128("capacity_mwh")]),
      method("get_node", "read", [u32("id")]),
      method("is_registered", "read", [u32("id")]),
      method("node_count", "read"),
      method("link_edge", "write", [address("admin"), u32("a"), u32("b")]),
      method("neighbors", "read", [u32("id")]),
      method("are_connected", "read", [u32("a"), u32("b")]),
      method("deactivate_node", "write", [address("admin"), u32("id")]),
      method("total_capacity", "read"),
      method("set_traversal_limit", "write", [address("admin"), u32("limit")]),
      method("traversal_limit", "read"),
    ],
  },
  {
    key: "account_ledger",
    label: "Account Ledger",
    description: "Tracks stablecoin balances and converts to internal Watt-credit accounting.",
    methods: [
      method("init", "write", [address("admin"), i128("watts_per_unit")]),
      method("admin", "read"),
      method("rate", "read"),
      method("deposit", "write", [address("account"), i128("amount")]),
      method("withdraw", "write", [address("account"), i128("credits")]),
      method("credit_watts", "write", [address("admin"), address("account"), i128("watts_delta")]),
      method("stable_balance", "read", [address("account")]),
      method("watt_balance", "read", [address("account")]),
      method("authorize_router", "write", [address("admin"), address("router")]),
      method("revoke_router", "write", [address("admin"), address("router")]),
      method("is_router", "read", [address("router")]),
      method("apply_consumption", "write", [address("router"), address("account"), i128("credits")]),
      method("set_token", "write", [address("admin"), address("token")]),
      method("token", "read"),
      method("deposit_secured", "write", [address("account"), i128("amount")]),
      method("withdraw_secured", "write", [address("account"), i128("credits")]),
    ],
  },
  {
    key: "micro_billing",
    label: "Micro Billing",
    description: "Processes high-frequency consumption payloads from physical meters.",
    methods: [
      method("init", "write", [address("admin"), i128("price_per_mwh")]),
      method("admin", "read"),
      method("price", "read"),
      method("wire", "write", [address("admin"), address("ledger")]),
      method("ledger", "read"),
      method("record", "write", [address("account"), u32("node_id"), i128("milli_wh"), u64("nonce")]),
      method("settle", "write", [address("account")]),
      method("unsettled", "read", [address("account")]),
      method("reading_count", "read", [address("account")]),
      method("seen_nonce", "read", [address("account"), u64("nonce")]),
    ],
  },
  {
    key: "delinquency_disconnector",
    label: "Delinquency Disconnector",
    description: "Evaluates negative balances and issues signed disconnect instructions.",
    methods: [
      method("init", "write", [address("admin"), u64("grace_period")]),
      method("admin", "read"),
      method("grace_period", "read"),
      method("wire", "write", [address("admin"), address("ledger")]),
      method("ledger", "read"),
      method("mark_delinquent", "write", [address("admin"), address("account"), u64("ledger_seq")]),
      method("assess", "read", [address("account"), u64("current_seq")]),
      method("issue_disconnect", "read", [address("account")]),
      method("clear", "write", [address("admin"), address("account")]),
      method("delinquent_since", "read", [address("account")]),
      method("is_delinquent", "read", [address("account")]),
    ],
  },
  {
    key: "yield_auction",
    label: "Yield Auction",
    description: "Double-sided matching engine for prosumer energy auctions.",
    methods: [
      method("init", "write", [address("admin")]),
      method("admin", "read"),
      method("create_auction", "write", [address("seller"), i128("energy_mwh"), i128("min_price")]),
      method("place_bid", "write", [address("buyer"), u64("auction_id"), i128("price")]),
      method("match_auction", "write", [u64("auction_id")]),
      method("cancel_auction", "write", [address("seller"), u64("auction_id")]),
      method("auction", "read", [u64("auction_id")]),
      method("best_bid", "read", [u64("auction_id")]),
      method("bid_count", "read", [u64("auction_id")]),
      method("is_cleared", "read", [u64("auction_id")]),
    ],
  },
  {
    key: "peak_load_adjuster",
    label: "Peak Load Adjuster",
    description: "Dynamic pricing based on battery state-of-charge.",
    methods: [
      method("init", "write", [address("admin"), i128("base_price"), i128("floor_price")]),
      method("admin", "read"),
      method("update_soc", "write", [address("admin"), i128("soc_bps")]),
      method("current_price", "read"),
      method("set_max_price", "write", [address("admin"), i128("max_price")]),
      method("soc", "read"),
      method("price_multiplier_bps", "read"),
    ],
  },
  {
    key: "battery_pool",
    label: "Battery Pool",
    description: "Fee reserves for battery maintenance and replacement.",
    methods: [
      method("init", "write", [address("admin"), i128("fee_bps")]),
      method("admin", "read"),
      method("contribute", "write", [address("account"), i128("amount")]),
      method("collect_fee", "write", [i128("settlement_amount")]),
      method("reserve_for_maintenance", "write", [address("admin"), i128("amount"), bytes32("reason_hash")]),
      method("total_reserve", "read"),
      method("reserved_for_maintenance", "read"),
      method("available", "read"),
      method("is_reserved", "read", [bytes32("reason_hash")]),
      method("fee_bps", "read"),
    ],
  },
  {
    key: "community_capital",
    label: "Community Capital",
    description: "Crowdfunding for grid expansion with programmatic dividend yields.",
    methods: [
      method("init", "write", [address("admin"), i128("yield_bps")]),
      method("admin", "read"),
      method("invest", "write", [address("investor"), i128("amount")]),
      method("accrue_yield", "write", [address("admin")]),
      method("claim_dividend", "write", [address("investor")]),
      method("shares_of", "read", [address("investor")]),
      method("total_shares", "read"),
      method("undistributed", "read"),
      method("yield_bps", "read"),
      method("share_price", "read"),
      method("total_capital", "read"),
      method("dividend_index", "read"),
    ],
  },
  {
    key: "iot_registrar",
    label: "IoT Registrar",
    description: "On-chain whitelist of authorized physical meter public keys.",
    methods: [
      method("init", "write", [address("admin")]),
      method("admin", "read"),
      method("register_device", "write", [bytes32("public_key"), { name: "model", type: "string" as const }]),
      method("revoke", "write", [address("admin"), bytes32("public_key"), bytes("reason")]),
      method("is_registered", "read", [bytes32("public_key")]),
      method("device", "read", [bytes32("public_key")]),
      method("registered_count", "read"),
      method("version", "read", [bytes32("public_key")]),
      method("transfer", "write", [address("admin"), bytes32("from_key"), bytes32("to_key")]),
      method("revocation_reason", "read", [bytes32("public_key")]),
    ],
  },
  {
    key: "cryptographic_oracle",
    label: "Cryptographic Oracle",
    description: "Ingests batch-compiled offline telemetry from offline periods.",
    methods: [
      method("init", "write", [address("admin")]),
      method("admin", "read"),
      method("compute_digest", "read", [bytes("entries")]),
      method("submit_batch", "write", [address("submitter"), bytes32("batch_id"), bytes("entries"), bytes32("digest")]),
      method("settle", "write", [address("admin"), bytes32("batch_id")]),
      method("is_settled", "read", [bytes32("batch_id")]),
      method("batch", "read", [bytes32("batch_id")]),
      method("total_mwh", "read", [bytes32("batch_id")]),
      method("batch_count", "read"),
    ],
  },
  {
    key: "firmware_verifier",
    label: "Firmware Verifier",
    description: "Verifies field hardware matches community-approved firmware hashes.",
    methods: [
      method("init", "write", [address("admin")]),
      method("admin", "read"),
      method("approve_firmware", "write", [address("admin"), bytes32("firmware_hash"), u32("version"), u32("min_version")]),
      method("revoke_firmware", "write", [address("admin"), bytes32("firmware_hash")]),
      method("verify_device", "read", [bytes32("device_key"), bytes32("firmware_hash")]),
      method("report_device", "write", [address("admin"), bytes32("device_key"), bytes32("firmware_hash"), u32("current_version")]),
      method("is_approved", "read", [bytes32("firmware_hash")]),
      method("approved_version", "read", [bytes32("firmware_hash")]),
      method("latest_version", "read"),
      method("floor_version", "read"),
      method("stale_report_count", "read", [bytes32("firmware_hash")]),
      method("device_version", "read", [bytes32("device_key")]),
      method("report_digest", "read", [bytes32("firmware_hash")]),
      method("verdict_of", "read", [bytes32("device_key"), bytes32("firmware_hash")]),
    ],
  },
  {
    key: "hardware_slash",
    label: "Hardware Slash",
    description: "Seizes collateral deposits from malicious or faulty meters.",
    methods: [
      method("init", "write", [address("admin"), i128("burn_bps"), i128("slash_bps_gen"), i128("slash_bps_bypass"), u64("appeal_timeout_ledgers")]),
      method("admin", "read"),
      method("deposit_collateral", "write", [address("owner"), i128("amount")]),
      method("slash", "write", [address("admin"), address("owner"), u32("offence")]),
      method("appeal_window", "write", [address("admin"), address("owner"), u32("offence")]),
      method("resolve_appeal", "write", [address("admin"), address("owner"), u32("offence")]),
      method("collateral_of", "read", [address("owner")]),
      method("compensation_pool", "read"),
      method("total_burned", "read"),
      method("total_slashed", "read"),
      method("total_deposited", "read"),
      method("burn_bps", "read"),
      method("was_slashed", "read", [address("owner"), u32("offence")]),
      method("is_appealed", "read", [address("owner"), u32("offence")]),
    ],
  },
];

export const CONTRACT_BY_KEY = Object.fromEntries(
  CONTRACTS.map((contract) => [contract.key, contract]),
) as Record<ContractKey, ContractDefinition>;

export function defaultArguments(methodDefinition: ContractMethod): ContractArgument[] {
  return (methodDefinition.args ?? []).map((argument) => ({
    ...argument,
    value: argument.type === "bool" ? false : "",
  }));
}