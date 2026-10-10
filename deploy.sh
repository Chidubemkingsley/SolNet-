#!/bin/bash
# Deploys all 12 SolNet contracts to a local standalone network.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$ROOT/solnet_contracts"

echo "========= 🚀 Initializing SolNet Deployment Tool ========="

# 1. Start a local standalone network in the background if not already running
# docker run --rm -it -p 8000:8000 stellar/quickstart:latest --standalone --enable-soroban-rpc

# 2. Configure network identifiers
stellar network add --rpc-url http://localhost:8000/soroban/rpc --network-passphrase "Standalone Network ; February 2017" standalone

# 3. Create cryptographic keys for administrators
stellar keys generate --network standalone admin_account
ADMIN_ADDRESS=$(stellar keys address admin_account)
echo "🎯 Admin Account Initialized: $ADMIN_ADDRESS"

# 4. Compile all workspace contracts down to optimized WebAssembly binaries
echo "📦 Building workspace contracts via Cargo..."
cargo build --manifest-path "$CONTRACTS_DIR/Cargo.toml" \
    --target wasm32-unknown-unknown --release

# 5. Per-contract initialization arguments.
#    Contracts with no extra args initialize with the admin address alone.
#    Values are overridable via the environment.
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
    [hardware_slash]="--burn-bps ${BURN_BPS:-2500}"
)

CONTRACTS=(
    "grid_topology" "account_ledger" "micro_billing" "delinquency_disconnector"
    "yield_auction" "peak_load_adjuster" "battery_pool" "community_capital"
    "iot_registrar" "cryptographic_oracle" "firmware_verifier" "hardware_slash"
)

# 6. Install and initialize every individual contract module
for c in "${CONTRACTS[@]}"; do
    echo "--------------------------------------------------"
    echo "🚀 Installing WASM bytecode for: $c"

    # Cargo names the artifact after the package, not the directory.
    WASM_PATH="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/solnet_${c}.wasm"

    if [[ ! -f "$WASM_PATH" ]]; then
        echo "❌ Missing WASM artifact: $WASM_PATH"
        exit 1
    fi

    CONTRACT_ID=$(stellar contract deploy \
        --wasm "$WASM_PATH" \
        --source admin_account \
        --network standalone)

    echo "✅ Deployed $c! ID: $CONTRACT_ID"

    # Invoke each module's initialization hook with its own arguments
    echo "⚙️  Initializing $c state machine..."
    # shellcheck disable=SC2086
    stellar contract invoke \
        --id "$CONTRACT_ID" \
        --source admin_account \
        --network standalone \
        -- \
        init --admin "$ADMIN_ADDRESS" ${INIT_ARGS[$c]}
done

echo "=================================================="
echo "🎉 Deployment Successful! All 12 contracts live."