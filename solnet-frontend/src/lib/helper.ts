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
  getAddress,
  getNetwork,
  isConnected,
  requestAccess,
  signTransaction,
} from "@stellar/freighter-api";

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
  | "json";

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
  return new rpc.Server(url);
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

export async function connectStellarWallet(
  config: StellarConnectionConfig,
): Promise<string> {
  const installed = await isConnected();
  if (installed.error) throw new Error(installed.error.message);
  if (!installed.isConnected) {
    throw new Error("Freighter is not installed or available in this browser.");
  }
  const access = await requestAccess();
  if (access.error) throw new Error(access.error.message);
  if (!access.address) throw new Error("Freighter did not return a public address.");
  await assertWalletNetwork(config);
  return access.address;
}

export async function readConnectedWallet(): Promise<string | null> {
  const installed = await isConnected();
  if (installed.error) throw new Error(installed.error.message);
  if (!installed.isConnected) return null;
  const { address, error } = await getAddress();
  if (error) throw new Error(error.message);
  return address || null;
}

export async function assertWalletNetwork(
  config: StellarConnectionConfig,
): Promise<void> {
  const walletNetwork = await getNetwork();
  if (walletNetwork.error) throw new Error(walletNetwork.error.message);
  const expected = getNetworkPassphrase(config);
  if (walletNetwork.networkPassphrase !== expected) {
    throw new Error(
      `Freighter is on ${walletNetwork.network || "a different network"}. Switch it to ${NETWORKS[config.network].label} before continuing.`,
    );
  }
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
  return createSorobanServer(config).getAccount(address);
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
  const signature = await signTransaction(prepared.toXDR(), {
    address: input.sourceAddress,
    networkPassphrase: passphrase,
  });
  if (signature.error) throw new Error(signature.error.message);
  const signed = TransactionBuilder.fromXDR(signature.signedTxXdr, passphrase);
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

export function shortenAddress(address: string, chars = 5): string {
  if (address.length <= chars * 2 + 3) return address;
  return `${address.slice(0, chars + 1)}…${address.slice(-chars)}`;
}
