# SolNet: Autonomous DePIN Microgrid Protocol

SolNet is a decentralized physical infrastructure network (DePIN) built on Stellar's Soroban smart contract engine. It enables autonomous, localized peer-to-peer energy routing, real-time micro-billing, and hardware-enforced access control for shared community solar grids — operating seamlessly even during local telecommunication or internet blackouts.

---

## 🛑 The Core Problem & Friction

In emerging economies and off-grid settlements, over 600 million people rely on local solar microgrids. However, current operations suffer from three major issues:

- **Infrastructure Defaults** — High-frequency resource hoarding occurs when one shop over-consumes power during peak hours, draining shared battery banks and leaving neighbors in the dark.
- **Cloud Dependency** — Traditional smart meters rely on centralized cloud servers. When local cell towers drop offline, payment processing stops, and the grid freezes.
- **Lack of Trust** — Off-grid operators must manually disconnect non-paying users, leading to disputes, human bias, and unstable localized micro-economies.

---

## 💡 The SolNet Solution

SolNet replaces centralized coordination with an interconnected mesh of 12 specialized smart contracts deployed on Stellar.

1. **Asset Stability** — Translates local fiat value into stablecoins (USDC/EURC) via Stellar Anchors to insulate working capital from local inflation.
2. **Offline Resilience** — Allows hardware meters to batch-sign telemetry updates locally and settle them on-chain asynchronously via localized mesh networks when backhaul connectivity resumes.
3. **Automated Enforcement** — Programmatically cuts off access when balances hit zero by transmitting a signed cryptographic disconnect instruction back down to the physical meter relay.

---

## 🏗️ Technical Architecture

SolNet bypasses monolithic design vulnerabilities by dividing responsibilities into a highly modular, decoupled architecture using Soroban Rust SDK v22.

```
                        ┌───────────────────────────┐
                        │  1. Grid Topology Registry │
                        └─────────────┬─────────────┘
                                      │
        ┌─────────────────────────────┼─────────────────────────────┐
        │                             │                             │
┌───────┴────────┐          ┌─────────┴────────┐          ┌─────────┴───────┐
│ Consumption &  │          │  Energy Markets  │          │     Hardware &  │
│   Settlement   │          │                  │          │    Security     │
└───────┬────────┘          └─────────┬────────┘          └─────────┬───────┘
        │                             │                             │
   2. Account Ledger            5. Yield Auction            9. IoT Registrar
   3. Micro-Billing             6. Peak-Load Adjuster      10. Crypto Oracle
   4. Delinquency Disconnector  7. Battery Pool            11. Firmware Verifier
                                8. Community Capital       12. Hardware Slash
```

### Module Reference

| # | Contract | Module | Responsibility |
|---|----------|--------|----------------|
| 1 | `grid_topology` | Foundation | Defines the physical layout of the grid, mapping downstream nodes, generation points, and storage arrays. |
| 2 | `account_ledger` | Consumption & Settlement | Tracks stablecoin balances and converts value into internal, hyper-fractionalized "Watt-credit" accounting. |
| 3 | `micro_billing` | Consumption & Settlement | Receives consumption payloads, processing high-frequency data packets directly from physical meters. |
| 4 | `delinquency_disconnector` | Consumption & Settlement | Evaluates accounts reaching zero-balance and signs standard atomic disconnection relay instructions. |
| 5 | `yield_auction` | Energy Markets | A localized double-sided matching engine enabling prosumers with generation surpluses to auction power to neighboring commercial loads. |
| 6 | `peak_load_adjuster` | Energy Markets | Alters pricing per Watt-hour dynamically based on real-time battery state-of-charge (SoC) metrics to protect battery longevity. |
| 7 | `battery_pool` | Energy Markets | Holds automated, fractional transaction fee reserves dedicated strictly to the physical maintenance and replacement of grid batteries. |
| 8 | `community_capital` | Energy Markets | Crowdfunding portal allowing external ecosystem investors to supply expansion capital and claim programmatic dividend yields. |
| 9 | `iot_registrar` | Hardware & Cryptographic Security | An on-chain whitelist mapping unique, factory-fused public keys embedded in tamper-resistant physical smart meters. |
| 10 | `cryptographic_oracle` | Hardware & Cryptographic Security | Ingests and untangles batch-compiled offline telemetry data packets generated during network blackouts. |
| 11 | `firmware_verifier` | Hardware & Cryptographic Security | Verifies that active field hardware exactly matches community-approved, open-source firmware hashes. |
| 12 | `hardware_slash` | Hardware & Cryptographic Security | Punishes malicious actors by seizing the collateral deposits of meters attempting to spoof generation numbers or bypass routers. |

---

## ⚙️ Quick Start & Local Deployment

### Prerequisites

Make sure your workstation has the following installed:

- Rust & Wasm32 Target — `rustup target add wasm32-unknown-unknown`
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools)
- Docker *(optional, for running a local quickstart network node)*

### 1. Clone & Build Workspace

Clone the repository and compile the 12-contract Rust workspace:

```bash
git clone https://github.com/Chidubemkingsley/SolNet-.git
cd SolNet-

# Compile all modules down to optimized WASM bytecode
cargo build --manifest-path solnet_contracts/Cargo.toml \
    --target wasm32-unknown-unknown --release
```

Artifacts land in `solnet_contracts/target/wasm32-unknown-unknown/release/solnet_<module>.wasm`.

### 2. Run the Test Suite

Every contract ships with an integration suite of **15 tests**, covering happy paths, every error branch, authorization failures, replay protection, and accounting invariants — 180 tests in total.

```bash
cargo test --manifest-path solnet_contracts/Cargo.toml --workspace
```

### 3. Deploy to Standalone Network

The deployment script generates an administrative keypair, installs all 12 compiled `.wasm` binaries onto the ledger, and invokes each module's initialization hook with its own configuration arguments.

```bash
chmod +x deploy.sh
./deploy.sh
```

Configuration values are overridable via environment variables (for example `BASE_PRICE`, `YIELD_BPS`, `GRACE_PERIOD`); see `deploy.sh` for the full table.

---

## ⚠️ Build Notes

Two version constraints are load-bearing. Breaking either one stops the workspace from compiling.

- **Soroban SDK v22, not v20.** The v20 line's `testutils` feature does not compile — an `arbitrary` / `stellar-xdr` derive incompatibility producing ~1800 errors — so no unit or integration tests can be written against it. The workspace pins `soroban-sdk = "22.0.0"`.
- **Keep `Cargo.lock` committed.** `soroban-env-host` declares `ed25519-dalek = ">=2.0.0"`, an unbounded range that resolves to v3 and breaks the build with a `CryptoRng` trait error. The lockfile pins `ed25519-dalek` to `2.2.0`. If it is ever regenerated, re-apply with:

  ```bash
  cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0
  ```

---

## 🌟 Why Stellar & Soroban?

- **Predictable Microtransactions** — Real-time hardware telemetry updates demand hundreds of state transitions. Stellar's low base transaction costs keep real-world operations viable.
- **Native Authentication** — Leveraging Soroban's native `require_auth` patterns guarantees that telemetry payloads cannot be spoofed, directly mirroring hardware identity on-chain.
- **State Archival Execution** — Utilizing Soroban's unique storage lease mechanics ensures that closed grid instances can be safely archived, preventing state bloat and saving gas.