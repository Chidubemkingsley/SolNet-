# SolNet Microgrid

SolNet is a responsive operating console for Stellar Soroban microgrid contracts. It reads live ledger/contract state and submits wallet-signed calls through Freighter. No account balances, device readings, orders, votes, or transaction results are seeded or simulated.

## Configure a real deployment

1. Choose the network where the contracts are actually deployed.
2. Copy `.env.example` to `.env.local`.
3. Set `VITE_STELLAR_NETWORK` to `testnet`, `mainnet`, or `standalone`; set `VITE_STELLAR_RPC_URL` to a Stellar RPC endpoint for that same network.
4. Add each real Soroban contract ID to its matching `VITE_CONTRACT_*` variable. IDs are public `C...` contract StrKeys. The provided brief's `CASOLNET...` examples are placeholders, not valid Stellar contract IDs; they must not be used.
5. Restart the web workflow after changing environment values. Alternatively, enter network, RPC, and IDs in the in-app Settings screen; browser settings are saved in local storage.
6. Install/unlock Freighter, connect the wallet, and make sure Freighter's network matches the app's selected network.
7. Use Settings to verify each configured ID against the selected RPC. The app enables contract reads and writes only for valid, deployed IDs.

The Testnet RPC URL is the default for safe development. Mainnet uses a public provider by default; replace it with the RPC provider selected for your deployment if necessary. Standalone defaults to `http://localhost:8000/soroban/rpc`.

## Contract interface accuracy

The frontend can only safely encode function arguments whose exact types are known. The supplied brief gives signatures for some operations and method names only for others. Those known signatures are in `src/lib/contracts.ts`; methods with unknown argument lists remain editable in Developer Tools and are not guessed by the product UI.

For reliable production use, compare those signatures with the deployed contract's Rust source or contract spec/XDR. A valid contract ID alone does not establish that the deployed contract has the expected methods or argument types.

## Development

From the Replit workspace root:

```sh
pnpm --filter @workspace/solnet-microgrid run dev
pnpm --filter @workspace/solnet-microgrid run typecheck
pnpm --filter @workspace/solnet-microgrid run build
```

In the standalone ZIP, run `pnpm install`, then `pnpm dev` from the unzipped project folder. The ZIP manifest is self-contained and does not require the surrounding Replit monorepo.

## Security

Wallet secrets and signing keys stay in Freighter and are never stored by this app. Network configuration and public contract IDs are stored in browser local storage unless supplied via environment variables. Review every transaction in Freighter before approving it.
