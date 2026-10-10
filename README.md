Here is your comprehensive hackathon documentation. I have formatted this specifically for you to drop directly into the README.md at the root of your GitHub repository.
------------------------------
## SolNet: Autonomous DePIN Microgrid Protocol
SolNet is a decentralized physical infrastructure network (DePIN) built on Stellar’s Soroban smart contract engine. It enables autonomous, localized peer-to-peer energy routing, real-time micro-billing, and hardware-enforced access control for shared community solar grids—operating seamlessly even during local telecommunication or internet blackouts.
------------------------------
## 🛑 The Core Problem & Friction
In emerging economies and off-grid settlements, over 600 million people rely on local solar microgrids. However, current operations suffer from three major issues:

* Infrastructure Defaults: High-frequency resource hoarding occurs when one shop over-consumes power during peak hours, draining shared battery banks and leaving neighbors in the dark.
* Cloud Dependency: Traditional smart meters rely on centralized cloud servers. When local cell towers drop offline, payment processing stops, and the grid freezes.
* Lack of Trust: Off-grid operators must manually disconnect non-paying users, leading to disputes, human bias, and unstable localized micro-economies.

------------------------------
## 💡 The SolNet Solution
SolNet replaces centralized coordination with an interconnected mesh of 12 specialized smart contracts deployed on Stellar.

   1. Asset Stability: Translates local fiat value into stablecoins (USDC/EURC) via Stellar Anchors to insulate working capital from local inflation.
   2. Offline Resilience: Allows hardware meters to batch-sign telemetry updates locally and settle them on-chain asynchronously via localized mesh networks when backhaul connectivity resumes.
   3. Automated Enforcement: Programmatically cuts off access when balances hit zero by transmitting a signed cryptographic disconnect instruction back down to the physical meter relay.

------------------------------
## 🏗️ Technical Architecture (12-Contract Mesh)
SolNet bypasses monolithic design vulnerabilities by dividing responsibilities into a highly modular, decoupled architecture using Soroban Rust SDK v22.

                       [ 1. Grid Topology Registry ]
                                     |
       +-----------------------------+-----------------------------+

       |                             |                             |
 [ Consumption & Billing ]    [ Energy Markets ]            [ Hardware & Security ]

       |                             |                             |
  2. Account Balance Ledger     5. Real-time Yield Auction    9. IoT Device Registrar
  3. Micro-Billing Router       6. Peak-Load Pricing Adjuster 10. Cryptographic Oracle Hub
  4. Delinquency Disconnector   7. Infrastructure Battery Pool11. Firmware Hash Verifier
                                8. Community Capital Fund    12. Hardware Slash Engine

## 1. Consumption & Settlement Module

* grid_topology: Defines the physical layout of the grid, mapping downstream nodes, generation points, and storage arrays.
* account_ledger: Tracks stablecoin balances and converts value into internal, hyper-fractionalized "Watt-credit" accounting.
* micro_billing: Receives consumption payloads, processing high-frequency data packets directly from physical meters.
* delinquency_disconnector: Evaluates accounts reaching zero-balance and signs standard atomic disconnection relay instructions.

## 2. Energy Market Dynamics

* yield_auction: A localized double-sided matching engine enabling prosumers with generation surpluses to auction power to neighboring commercial loads.
* peak_load_adjuster: Alters pricing per Watt-hour dynamically based on real-time battery state-of-charge (SoC) metrics to protect battery longevity.
* battery_pool: Holds automated, fractional transaction fee reserves dedicated strictly to the physical maintenance and replacement of grid batteries.
* community_capital: Crowdfunding portal allowing external ecosystem investors to supply expansion capital and claim programmatic dividend yields.

## 3. Hardware & Cryptographic Security

* iot_registrar: An on-chain whitelist mapping unique, factory-fused public keys embedded in tamper-resistant physical smart meters.
* cryptographic_oracle: Ingests and untangles batch-compiled offline telemetry data packets generated during network blackouts.
* firmware_verifier: Verifies that active field hardware exactly matches community-approved, open-source firmware hashes.
* hardware_slash: Punishes malicious actors by seizing the collateral deposits of meters attempting to spoof generation numbers or bypass routers.

------------------------------
## ⚙️ Quick Start & Local Deployment## Prerequisites
Make sure your workstation has the following installed:

* Rust & Wasm32 Target (rustup target add wasm32-unknown-unknown)
* [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools)
* Docker (Optional, for running a local quickstart network node)

## 1. Clone & Build Workspace
Clone your repository, navigate to the directory, and compile the 12-contract Rust workspace:

# Clone the repository
git clone https://github.com
cd solnet-protocol
# Compile all modules down to optimized WASM bytecode
cargo build --manifest-path solnet_contracts/Cargo.toml --target wasm32-unknown-unknown --release

## 2. Run the Test Suite
Every contract ships with an integration suite of 15 tests covering happy paths, every error branch, authorization failures, and accounting invariants:

cargo test --manifest-path solnet_contracts/Cargo.toml --workspace

## 3. Deploy to Standalone Network
We provide an automated automation script to start tracking your protocol locally. Initialize a local network node and execute the deployment flow:

# Make the deployment script executable
chmod +x deploy.sh
# Run the local deployment flow
./deploy.sh

This script adds the standalone network profile to your Stellar configuration, generates an administrative keypair, installs all 12 compiled .wasm binaries onto the ledger, and invokes each module's initialization hook with its own configuration arguments.

------------------------------
## ⚠️ Build Notes
* **Soroban SDK v22, not v20.** The v20 line's `testutils` feature does not compile (an `arbitrary` / `stellar-xdr` derive incompatibility), so no unit or integration tests can be written against it. The workspace pins `soroban-sdk = "22.0.0"`.
* **Lockfile pin required.** `soroban-env-host` declares `ed25519-dalek = ">=2.0.0"`, an unbounded range that resolves to v3, which breaks the build with a `CryptoRng` trait error. `Cargo.lock` pins `ed25519-dalek` to `2.2.0` — keep the lockfile committed, or re-apply with `cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0`.
------------------------------
------------------------------
## 🌟 Why Stellar & Soroban?

* Predictable Microtransactions: Real-time hardware telemetry updates demand hundreds of state transitions. Stellar's low base transaction costs keep real-world operations viable.
* Native Authentication: Leveraging Soroban’s native require_auth patterns guarantees that telemetry payloads cannot be spoofed, directly mirroring hardware identity on-chain.
* State Archival Execution: Utilizing Soroban's unique storage lease mechanics ensures that closed grid instances can be safely archived, preventing state bloat and saving gas.

------------------------------



