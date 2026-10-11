import {
  Address,
  BASE_FEE,
  Networks,
  Operation,
  StrKey,
  TransactionBuilder,
  nativeToScVal,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import { rpc } from "@stellar/stellar-sdk";
import {
  StellarWalletsKit,
  Networks as KitNetworks,
  type ISupportedWallet,
} from "@creit.tech/stellar-wallets-kit";
import {
  FreighterModule,
  FREIGHTER_ID,
} from "@creit.tech/stellar-wallets-kit/modules/freighter";
import {
  AlbedoModule,
  ALBEDO_ID,
} from "@creit.tech/stellar-wallets-kit/modules/albedo";
import {
  xBullModule,
  XBULL_ID,
} from "@creit.tech/stellar-wallets-kit/modules/xbull";
import {
  RabetModule,
  RABET_ID,
} from "@creit.tech/stellar-wallets-kit/modules/rabet";
import {
  LobstrModule,
  LOBSTR_ID,
} from "@creit.tech/stellar-wallets-kit/modules/lobstr";
import {
  HanaModule,
  HANA_ID,
} from "@creit.tech/stellar-wallets-kit/modules/hana";

export type StellarNetwork = "testnet" | "mainnet" | "standalone";

export interface StellarConnectionConfig {
  network: StellarNetwork;
  rpcUrl: string;
  customNetworkPassphrase?: string;
}

export interface ContractArgument {
  name: string;
  type: SorobanArgumentType;
  value: string | boolean;
}

export type SorobanArgumentType =
  | "address"
  | "bool"
  | "string"
  | "symbol"
  | "bytes"
  | "bytes32"
  | "i32"
  | "u32"
  | "i64"
  | "u64"
  | "i128"
  | "u128"
  | "i256"
  | "u256"
  | "json"
  | "telemetry"
  | "offence";

export interface NetworkStatus {
  network: StellarNetwork;
  rpcUrl: string;
  latestLedger: number;
  latencyMs: number;
  checkedAt: string;
}

export interface SubmittedTransaction {
  hash: string;
  status: "SUCCESS" | "PENDING";
  result?: unknown;
}

export const NETWORKS: Record<
  StellarNetwork,
  { label: string; passphrase: string; defaultRpcUrl: string }
> = {
  testnet: {
    label: "Testnet",
    passphrase: Networks.TESTNET,
    defaultRpcUrl: "https://soroban-testnet.stellar.org",
  },
  mainnet: {
    label: "Mainnet",
    passphrase: Networks.PUBLIC,
    defaultRpcUrl: "https://mainnet.sorobanrpc.com",
  },
  standalone: {
    label: "Standalone",
    passphrase: Networks.STANDALONE,
    defaultRpcUrl: "http://localhost:8000/soroban/rpc",
  },
};

export function getNetworkPassphrase(
  config: StellarConnectionConfig,
): string {
  return config.network === "standalone" && config.customNetworkPassphrase?.trim()
    ? config.customNetworkPassphrase.trim()
    : NETWORKS[config.network].passphrase;
}

export function createSorobanServer(config: StellarConnectionConfig): rpc.Server {
  const url = config.rpcUrl.trim();
  if (!url) throw new Error("Enter a Stellar RPC URL in network settings.");
  let parsedUrl: URL;
  try {
    parsedUrl = new URL(url);
  } catch {
    throw new Error("The Stellar RPC URL is not a valid URL.");
  }
  if (
    parsedUrl.protocol !== "https:" &&
    !(config.network === "standalone" && parsedUrl.protocol === "http:")
  ) {
    throw new Error("Use HTTPS for public networks. HTTP is allowed only for Standalone.");
  }
  // The SDK refuses plain HTTP unless explicitly opted in. Our own check
  // above already restricts HTTP to standalone networks only.
  return new rpc.Server(url, {
    allowHttp: parsedUrl.protocol === "http:",
  });
}

export function isValidContractAddress(address: string): boolean {
  return StrKey.isValidContract(address.trim());
}

export async function checkRpcStatus(
  config: StellarConnectionConfig,
): Promise<NetworkStatus> {
  const server = createSorobanServer(config);
  const start = performance.now();
  const [networkInfo, latestLedger] = await Promise.all([
    server.getNetwork(),
    server.getLatestLedger(),
  ]);
  assertRpcPassphrase(networkInfo.passphrase, config);
  return {
    network: config.network,
    rpcUrl: config.rpcUrl,
    latestLedger: latestLedger.sequence,
    latencyMs: Math.round(performance.now() - start),
    checkedAt: new Date().toISOString(),
  };
}

function assertRpcPassphrase(
  actualPassphrase: string,
  config: StellarConnectionConfig,
): void {
  const expected = getNetworkPassphrase(config);
  if (actualPassphrase !== expected) {
    throw new Error(
      `RPC network mismatch: the endpoint uses a different passphrase than ${NETWORKS[config.network].label}. Check the network and RPC URL in Settings.`,
    );
  }
}

export interface ConnectedWallet {
  address: string;
  walletId: string;
  walletName: string;
  networkWarning?: string;
}

export const WALLET_STORAGE_KEY = "solnet.wallet.v1";

function kitNetworkFor(config: StellarConnectionConfig): KitNetworks {
  switch (config.network) {
    case "mainnet":
      return KitNetworks.PUBLIC;
    case "standalone":
      return KitNetworks.STANDALONE;
    case "testnet":
    default:
      return KitNetworks.TESTNET;
  }
}

let kitInitKey: string | null = null;

function ensureKit(config: StellarConnectionConfig): void {
  if (kitInitKey === config.network) return;
  StellarWalletsKit.init({
    modules: [
      new FreighterModule(),
      new AlbedoModule(),
      new xBullModule(),
      new RabetModule(),
      new LobstrModule(),
      new HanaModule(),
    ],
    network: kitNetworkFor(config),
  });
  kitInitKey = config.network;
}

export async function listWallets(
  config: StellarConnectionConfig,
): Promise<ISupportedWallet[]> {
  ensureKit(config);
  return StellarWalletsKit.refreshSupportedWallets();
}

export function readStoredWallet(): ConnectedWallet | null {
  try {
    const stored = localStorage.getItem(WALLET_STORAGE_KEY);
    if (!stored) return null;
    const parsed = JSON.parse(stored) as Partial<ConnectedWallet>;
    if (
      typeof parsed.address !== "string" ||
      typeof parsed.walletId !== "string" ||
      !parsed.address ||
      !parsed.walletId
    ) {
      return null;
    }
    return {
      address: parsed.address,
      walletId: parsed.walletId,
      walletName:
        typeof parsed.walletName === "string" && parsed.walletName
          ? parsed.walletName
          : parsed.walletId,
    };
  } catch {
    return null;
  }
}

function storeWallet(wallet: ConnectedWallet): void {
  localStorage.setItem(WALLET_STORAGE_KEY, JSON.stringify(wallet));
}

function clearStoredWallet(): void {
  localStorage.removeItem(WALLET_STORAGE_KEY);
}

function walletDisplayName(walletId: string): string {
  switch (walletId) {
    case FREIGHTER_ID:
      return "Freighter";
    case ALBEDO_ID:
      return "Albedo";
    case XBULL_ID:
      return "xBull";
    case RABET_ID:
      return "Rabet";
    case LOBSTR_ID:
      return "Lobstr";
    case HANA_ID:
      return "Hana";
    default:
      return walletId;
  }
}

export async function connectStellarWallet(
  config: StellarConnectionConfig,
  walletId: string,
): Promise<ConnectedWallet> {
  ensureKit(config);
  StellarWalletsKit.setWallet(walletId);
  const { address } = await StellarWalletsKit.fetchAddress();
  if (!address) {
    throw new Error("The selected wallet did not return a public address.");
  }
  // Deliberately not blocking here: reads need no signing, and not every
  // wallet can report its network. The mismatch (if any) is surfaced as a
  // warning, and writes still enforce it before anything is signed.
  const networkWarning = await probeWalletNetwork(config);
  const connected: ConnectedWallet = {
    address,
    walletId,
    walletName: walletDisplayName(walletId),
    ...(networkWarning ? { networkWarning } : {}),
  };
  storeWallet(connected);
  return connected;
}

export async function readConnectedWallet(
  config: StellarConnectionConfig,
): Promise<ConnectedWallet | null> {
  const stored = readStoredWallet();
  if (!stored) return null;
  // Only extension wallets with persistent site permissions (Freighter) can be
  // re-read silently. Others would pop a window on page load, so for those the
  // stored selection is kept for one-click reconnect instead.
  if (stored.walletId !== FREIGHTER_ID) return null;
  try {
    ensureKit(config);
    StellarWalletsKit.setWallet(stored.walletId);
    const { address } = await StellarWalletsKit.fetchAddress();
    if (!address) return null;
    const connected: ConnectedWallet = { ...stored, address };
    storeWallet(connected);
    return connected;
  } catch {
    return null;
  }
}

export async function disconnectStellarWallet(): Promise<void> {
  try {
    await StellarWalletsKit.disconnect();
  } catch {
    // Disconnect is best-effort; the local session is cleared regardless.
  } finally {
    clearStoredWallet();
  }
}

function walletNetworkMismatchMessage(config: StellarConnectionConfig): string {
  const expected = getNetworkPassphrase(config);
  const base =
    `The connected wallet is on a different network than this app (expected passphrase "${expected}"). ` +
    `Reads still work, but signing will fail until the wallet matches.`;
  if (config.network !== "standalone") {
    return `${base} Switch the wallet to ${NETWORKS[config.network].label} before signing.`;
  }
  return (
    `${base} In Freighter: open Settings → Networks → add a custom network with ` +
    `Name "Standalone", URL "http://localhost:8000/soroban/rpc", passphrase ` +
    `"Standalone Network ; February 2017", Allow HTTP connection enabled — then switch to it. ` +
    `Other wallets must likewise be set to that exact passphrase.`
  );
}

async function probeWalletNetwork(
  config: StellarConnectionConfig,
): Promise<string | null> {
  const expected = getNetworkPassphrase(config);
  let reported: string;
  try {
    const network = await StellarWalletsKit.getNetwork();
    reported = network.networkPassphrase;
  } catch {
    return null;
  }
  if (!reported || reported === expected) return null;
  return walletNetworkMismatchMessage(config);
}

export async function assertWalletNetwork(
  config: StellarConnectionConfig,
): Promise<void> {
  const expected = getNetworkPassphrase(config);
  let reported: string;
  try {
    const network = await StellarWalletsKit.getNetwork();
    reported = network.networkPassphrase;
  } catch {
    // Not every wallet reports its network. Signing below always carries the
    // explicit passphrase and the RPC-side check still guards the endpoint,
    // so an unreportable network is not a blocker.
    return;
  }
  if (reported !== expected) {
    throw new Error(walletNetworkMismatchMessage(config));
  }
}

export interface TelemetryRow {
  node_id: number;
  milli_wh: string;
  recorded_at: number;
}

function scSymbol(name: string) {
  return nativeToScVal(name, { type: "symbol" });
}

/// Encodes oracle telemetry rows exactly as the contract's `TelemetryEntry`
/// struct layout demands: one `ScMap` per entry with symbol keys, inside an
/// outer `ScVec`. Struct field order follows declaration order
/// (node_id, milli_wh, recorded_at). Verified against the derive macro in
/// soroban-sdk, which serializes contracttype structs as maps — *not* vecs,
/// which is the easy mistake here.
export function encodeTelemetryEntries(raw: unknown): xdr.ScVal {
  let rows: unknown;
  if (typeof raw === "string") {
    try {
      rows = JSON.parse(raw);
    } catch {
      throw new Error(`entries must be a JSON array of telemetry rows.`);
    }
  } else {
    rows = raw;
  }
  if (!Array.isArray(rows) || rows.length === 0) {
    throw new Error(`entries must be a non-empty JSON array.`);
  }
  const entryVals = rows.map((row, index) => {
    if (typeof row !== "object" || row === null) {
      throw new Error(`entries[${index}] must be an object.`);
    }
    const record = row as Record<string, unknown>;
    const nodeId = Number(record.node_id);
    const recordedAt = Number(record.recorded_at);
    if (!Number.isInteger(nodeId) || nodeId < 0) {
      throw new Error(`entries[${index}].node_id must be a non-negative whole number.`);
    }
    if (typeof record.milli_wh !== "string" && typeof record.milli_wh !== "number") {
      throw new Error(`entries[${index}].milli_wh must be an integer, decimal string preferred.`);
    }
    if (!/^-?\d+$/.test(String(record.milli_wh))) {
      throw new Error(`entries[${index}].milli_wh must be an integer written in decimal.`);
    }
    if (!Number.isInteger(recordedAt) || recordedAt < 0) {
      throw new Error(`entries[${index}].recorded_at must be a non-negative whole number.`);
    }
    // ScMap keys must be byte-sorted or the host's map.validate() rejects
    // the whole argument ("failed to convert ScVal to host value").
    // Sorted byte order: milli_wh < node_id < recorded_at.
    const entries: Array<{ name: string; val: xdr.ScVal }> = [
      { name: "milli_wh", val: nativeToScVal(BigInt(String(record.milli_wh)), { type: "i128" }) },
      { name: "node_id", val: nativeToScVal(nodeId, { type: "u32" }) },
      { name: "recorded_at", val: nativeToScVal(recordedAt, { type: "u64" }) },
    ];
    return xdr.ScVal.scvMap(
      entries.map(
        ({ name, val }) => new xdr.ScMapEntry({ key: scSymbol(name), val }),
      ),
    );
  });
  return xdr.ScVal.scvVec(entryVals);
}

/// Encodes a hardware-slash offence code as the contract's `Offence` enum.
/// Unit-variant contracttype enums serialize as a one-element vec holding
/// the variant symbol — *not* a bare symbol and *not* an integer. Read
/// straight from soroban-sdk-macros `derive_enum.rs` (`into_xdr` builds
/// `(symbol,).try_into()` = ScVec, `try_from_val` only accepts ScVal::Vec),
/// after a bare symbol trapped the live contract in testing.
export function encodeOffenceCode(raw: unknown): xdr.ScVal {
  const code = String(raw).trim();
  if (code === "1") return xdr.ScVal.scvVec([scSymbol("SpoofedGeneration")]);
  if (code === "2") return xdr.ScVal.scvVec([scSymbol("RouterBypass")]);
  throw new Error(`offence must be 1 (spoofed generation) or 2 (router bypass).`);
}

export function bytesToHex(value: unknown): string {
  const bytes =
    value instanceof Uint8Array
      ? value
      : typeof value === "string" && /^(?:[0-9a-f]{2})+$/i.test(value)
        ? Uint8Array.from(value.match(/.{2}/g)!.map((byte) => parseInt(byte, 16)))
        : null;
  if (!bytes) throw new Error("Expected bytes for hex conversion.");
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function encodeArgument(argument: ContractArgument) {
  const raw = argument.value;
  switch (argument.type) {
    case "address":
      return Address.fromString(String(raw)).toScVal();
    case "bool":
      if (typeof raw !== "boolean") throw new Error(`${argument.name} must be true or false.`);
      return nativeToScVal(raw, { type: "bool" });
    case "bytes":
    case "bytes32": {
      const value = String(raw).replace(/^0x/i, "");
      if (!/^(?:[0-9a-f]{2})+$/i.test(value)) {
        throw new Error(`${argument.name} must be even-length hexadecimal bytes.`);
      }
      if (argument.type === "bytes32" && value.length !== 64) {
        throw new Error(`${argument.name} must be exactly 32 bytes (64 hex characters).`);
      }
      return nativeToScVal(Uint8Array.from(value.match(/.{2}/g)!.map((byte) => parseInt(byte, 16))), {
        type: "bytes",
      });
    }
    case "i32":
    case "u32": {
      const value = Number(raw);
      if (!Number.isInteger(value)) throw new Error(`${argument.name} must be a whole number.`);
      return nativeToScVal(value, { type: argument.type });
    }
    case "i64":
    case "u64":
    case "i128":
    case "u128":
    case "i256":
    case "u256": {
      if (!/^-?\d+$/.test(String(raw))) {
        throw new Error(`${argument.name} must be an integer written in decimal.`);
      }
      return nativeToScVal(BigInt(String(raw)), { type: argument.type });
    }
    case "json": {
      try {
        return nativeToScVal(JSON.parse(String(raw)));
      } catch {
        throw new Error(`${argument.name} must contain valid JSON.`);
      }
    }
    case "telemetry": {
      return encodeTelemetryEntries(raw);
    }
    case "offence": {
      return encodeOffenceCode(raw);
    }
    case "symbol":
      return nativeToScVal(String(raw), { type: "symbol" });
    default:
      return nativeToScVal(String(raw), { type: "string" });
  }
}

function contractOperation(contractId: string, method: string, args: ContractArgument[]) {
  const id = contractId.trim();
  if (!isValidContractAddress(id)) {
    throw new Error("This contract does not have a valid deployed Stellar contract ID.");
  }
  if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(method)) {
    throw new Error("Enter a valid Soroban function name.");
  }
  return Operation.invokeContractFunction({
    contract: id,
    function: method,
    args: args.map(encodeArgument),
  });
}

function getTransactionSource(
  sourceAddress: string,
  config: StellarConnectionConfig,
) {
  const address = sourceAddress.trim();
  if (!StrKey.isValidEd25519PublicKey(address)) {
    throw new Error("Connect a valid Stellar wallet before invoking this contract.");
  }
  return createSorobanServer(config).getAccount(address).catch((error: unknown) => {
    if (config.network === "standalone") {
      const faucet = config.rpcUrl.replace(/\/soroban\/rpc\/?$/, "") + `/friendbot?addr=${address}`;
      throw new Error(
        "This account does not exist on the standalone network yet. " +
          `Fund it first via the local faucet: ${faucet}`,
      );
    }
    throw error instanceof Error ? error : new Error("Could not load the source account.");
  });
}

export async function simulateContractRead(input: {
  config: StellarConnectionConfig;
  sourceAddress: string;
  contractId: string;
  method: string;
  args: ContractArgument[];
}): Promise<unknown> {
  const server = createSorobanServer(input.config);
  const networkInfo = await server.getNetwork();
  assertRpcPassphrase(networkInfo.passphrase, input.config);
  const account = await getTransactionSource(input.sourceAddress, input.config);
  const transaction = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: getNetworkPassphrase(input.config),
  })
    .addOperation(contractOperation(input.contractId, input.method, input.args))
    .setTimeout(30)
    .build();

  const simulation = await server.simulateTransaction(transaction);
  if ("error" in simulation && simulation.error) {
    throw new Error(simulation.error);
  }
  if (!("result" in simulation) || !simulation.result) {
    throw new Error("The RPC server returned no contract result.");
  }
  return scValToNative(simulation.result.retval);
}

export async function submitContractTransaction(input: {
  config: StellarConnectionConfig;
  sourceAddress: string;
  contractId: string;
  method: string;
  args: ContractArgument[];
  pollAttempts?: number;
}): Promise<SubmittedTransaction> {
  await assertWalletNetwork(input.config);
  const passphrase = getNetworkPassphrase(input.config);
  const server = createSorobanServer(input.config);
  const networkInfo = await server.getNetwork();
  assertRpcPassphrase(networkInfo.passphrase, input.config);
  const account = await getTransactionSource(input.sourceAddress, input.config);
  const transaction = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: passphrase,
  })
    .addOperation(contractOperation(input.contractId, input.method, input.args))
    .setTimeout(180)
    .build();

  const prepared = await server.prepareTransaction(transaction);
  ensureKit(input.config);
  const { signedTxXdr } = await StellarWalletsKit.signTransaction(
    prepared.toXDR(),
    {
      address: input.sourceAddress,
      networkPassphrase: passphrase,
    },
  );
  if (!signedTxXdr) throw new Error("The connected wallet did not return a signed transaction.");
  const signed = TransactionBuilder.fromXDR(signedTxXdr, passphrase);
  const submitted = await server.sendTransaction(signed);
  if (submitted.status === "ERROR") {
    throw new Error(`Stellar RPC rejected the transaction (hash ${submitted.hash}).`);
  }
  if (submitted.status !== "PENDING") {
    throw new Error(`Unexpected Stellar RPC submission status: ${submitted.status}.`);
  }

  const result = await server.pollTransaction(submitted.hash, {
    attempts: input.pollAttempts ?? 30,
  });
  if (result.status === "SUCCESS") {
    return {
      hash: submitted.hash,
      status: "SUCCESS",
      result: "resultMetaXdr" in result && result.resultMetaXdr
        ? result.resultMetaXdr.toXDR("base64")
        : undefined,
    };
  }
  if (result.status === "FAILED") {
    throw new Error(`The transaction failed on-chain. Hash: ${submitted.hash}`);
  }
  return { hash: submitted.hash, status: "PENDING" };
}

export async function verifyContractDeployment(
  contractId: string,
  config: StellarConnectionConfig,
): Promise<{ contractId: string; executable: string }> {
  const id = contractId.trim();
  if (!isValidContractAddress(id)) {
    throw new Error("Enter a valid deployed Stellar contract ID.");
  }
  const server = createSorobanServer(config);
  const networkInfo = await server.getNetwork();
  assertRpcPassphrase(networkInfo.passphrase, config);
  await server.getContractData(
    id,
    xdr.ScVal.scvLedgerKeyContractInstance(),
    rpc.Durability.Persistent,
  );
  return {
    contractId: id,
    executable: "Soroban contract instance exists on this network",
  };
}

export async function fetchContractEvents(
  config: StellarConnectionConfig,
  contractId: string,
  startLedger: number,
  limit = 100,
): Promise<{
  latestLedger: number;
  oldestLedger: number;
  events: Array<Record<string, unknown>>;
}> {
  const id = contractId.trim();
  if (!isValidContractAddress(id)) {
    throw new Error("Enter a valid deployed Stellar contract ID.");
  }
  if (!Number.isSafeInteger(startLedger) || startLedger < 1) {
    throw new Error("The first ledger must be a positive whole number.");
  }
  const server = createSorobanServer(config);
  const [networkInfo, latest] = await Promise.all([
    server.getNetwork(),
    server.getLatestLedger(),
  ]);
  assertRpcPassphrase(networkInfo.passphrase, config);
  const response = await server.getEvents({
    startLedger,
    endLedger: latest.sequence,
    filters: [{ type: "contract", contractIds: [id] }],
    limit: Math.max(1, Math.min(200, Math.floor(limit))),
  });
  return {
    latestLedger: latest.sequence,
    oldestLedger: response.oldestLedger,
    events: response.events.map((event) => ({
      id: event.id,
      type: event.type,
      contractId: event.contractId,
      ledger: event.ledger,
      ledgerClosedAt: event.ledgerClosedAt,
      txHash: event.txHash,
      transactionIndex: event.transactionIndex,
      operationIndex: event.operationIndex,
      inSuccessfulContractCall: event.inSuccessfulContractCall,
      topic: event.topic.map((topic) => scValToNative(topic)),
      value: scValToNative(event.value),
    })),
  };
}

export function toDisplayJson(value: unknown): string {
  return JSON.stringify(
    value,
    (_key, item: unknown) => {
      if (typeof item === "bigint") return item.toString();
      if (item instanceof Uint8Array) {
        return `0x${Array.from(item, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
      }
      return item;
    },
    2,
  );
}

export function stellarAddressToRawHex(address: string): string {
  const trimmed = address.trim();
  if (!StrKey.isValidEd25519PublicKey(trimmed)) {
    throw new Error("Enter a valid Stellar (G…) account address.");
  }
  const raw = StrKey.decodeEd25519PublicKey(trimmed);
  return Array.from(raw, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function shortenAddress(address: string, chars = 5): string {
  if (address.length <= chars * 2 + 3) return address;
  return `${address.slice(0, chars + 1)}…${address.slice(-chars)}`;
}
