#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$ROOT/solnet_contracts"

echo "========= 🚀 Initializing SolNet Deployment Tool ========="

# 1. Start a local standalone network in the background if not already running
# docker run --rm -it -p 8000:8000 stellar/quickstart:latest --standalone --enable-soroban-rpc

# Network selection: NETWORK=standalone (default) or NETWORK=testnet.
# SOURCE is the local signing identity; it must be funded on the target network.
NETWORK="${NETWORK:-standalone}"
SOURCE="${SOURCE:-admin_account}"

# 2. Configure network identifiers (standalone only; testnet/mainnet are built in)
if [[ "$NETWORK" == "standalone" ]]; then
  stellar network add --rpc-url http://localhost:8000/soroban/rpc --network-passphrase "Standalone Network ; February 2017" standalone
fi

# 3. Create cryptographic keys for administrators
stellar keys address "$SOURCE" >/dev/null 2>&1 || stellar keys generate --network "$NETWORK" "$SOURCE"
ADMIN_ADDRESS=$(stellar keys address "$SOURCE")
echo "🎯 Admin Account Initialized: $ADMIN_ADDRESS (network: $NETWORK)"

# 4. Compile all workspace contracts down to optimized WebAssembly binaries
echo "📦 Building workspace contracts via Stellar CLI..."
stellar contract build --manifest-path "$CONTRACTS_DIR/Cargo.toml"

# 5. Per-contract initialization arguments
declare -A INIT_ARGS=(
    [grid_topology]=""
    [account_ledger]="--watts-per-unit ${WATTS_PER_UNIT:-1000000}"
    [micro_billing]="--price-per-mwh ${PRICE_PER_MWH:-250}"
    [delinquency_disconnector]="--grace-period ${GRACE_PERIOD:-86400}"
    [yield_auction]=""
    [peak_load_adjuster]="--base-price ${BASE_PRICE:-250} --floor-price ${FLOOR_PRICE:-100}"
    [battery_pool]="--fee-bps ${FEE_BPS:-50}"
    [community_capital]="--yield-bps ${YIELD_BPS:-800}"
    [iot_registrar]=""
    [cryptographic_oracle]=""
    [firmware_verifier]=""
    [hardware_slash]="--burn-bps ${BURN_BPS:-2500} --slash-bps-gen ${SLASH_BPS_GEN:-5000} --slash-bps-bypass ${SLASH_BPS_BYPASS:-5000} --appeal-timeout-ledgers ${APPEAL_TIMEOUT_LEDGERS:-100}"
)

CONTRACTS=(
    "grid_topology" "account_ledger" "micro_billing" "delinquency_disconnector"
    "yield_auction" "peak_load_adjuster" "battery_pool" "community_capital"
    "iot_registrar" "cryptographic_oracle" "firmware_verifier" "hardware_slash"
)

# 6. Install and initialize every individual contract module
declare -A DEPLOYED_IDS

for c in "${CONTRACTS[@]}"; do
    echo "--------------------------------------------------"
    echo "🚀 Installing WASM bytecode for: $c"
    
    WASM_PATH="$CONTRACTS_DIR/target/wasm32v1-none/release/solnet_${c}.wasm"
    
    if [[ ! -f "$WASM_PATH" ]]; then
        echo "❌ Missing WASM artifact: $WASM_PATH"
        exit 1
    fi

    # Use `stellar contract deploy` with init args directly
    if [[ -n "${INIT_ARGS[$c]}" ]]; then
        CONTRACT_ID=$(stellar contract deploy \
            --wasm "$WASM_PATH" \
            --source "$SOURCE" \
            --network "$NETWORK" \
            -- \
            init ${INIT_ARGS[$c]} --admin "$ADMIN_ADDRESS" 2>&1 | grep -oE 'C[A-Z0-9]{55}' | tail -1)
    else
        CONTRACT_ID=$(stellar contract deploy \
            --wasm "$WASM_PATH" \
            --source "$SOURCE" \
            --network "$NETWORK" \
            -- \
            init --admin "$ADMIN_ADDRESS" 2>&1 | grep -oE 'C[A-Z0-9]{55}' | tail -1)
    fi

    if [[ -z "$CONTRACT_ID" ]]; then
        echo "❌ Failed to deploy $c"
        exit 1
    fi

    DEPLOYED_IDS[$c]=$CONTRACT_ID
    echo "✅ Deployed $c! ID: $CONTRACT_ID"
done

# Save contract IDs for frontend, per network so deployments don't clobber each other
OUT="$ROOT/solnet-frontend/src/lib/deployed-contracts.${NETWORK}.ts"
echo "export const DEPLOYED_CONTRACT_IDS_${NETWORK^^} = {" > "$OUT"
for c in "${CONTRACTS[@]}"; do
    echo "  ${c}: \"${DEPLOYED_IDS[$c]}\"," >> "$OUT"
done
echo "} as const;" >> "$OUT"
echo "📝 Wrote $OUT"

echo "=================================================="
echo "🎉 Deployment Successful! All 12 contracts live."
echo "=================================================="
for c in "${CONTRACTS[@]}"; do
    echo "  $c = ${DEPLOYED_IDS[$c]}"
done
