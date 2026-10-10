//! SolNet: Micro Billing Contract
//! Description: Processes high-frequency data packets to calculate real-time consumption and bill users.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, symbol_short, Address, Env};

/// Readings are reported in milli-watt-hours while the tariff is quoted per
/// watt-hour, so the charge is divided by 1000.
const CHARGE_SCALE: i128 = 1_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    InvalidPrice = 3,
    InvalidReading = 4,
    DuplicateNonce = 5,
    NodeMismatch = 6,
    NothingToSettle = 7,
    ArithmeticOverflow = 8,
}

#[contract]
pub struct MicroBilling;

#[contractimpl]
impl MicroBilling {
    /// Initializes the micro-billing contract with the administrator and the
    /// tariff expressed as the price of one watt-hour in the smallest currency
    /// unit.
    pub fn init(env: Env, admin: Address, price_per_mwh: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        if price_per_mwh <= 0 {
            return Err(Error::InvalidPrice);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&symbol_short!("admin"), &admin);
        env.storage()
            .instance()
            .set(&symbol_short!("price"), &price_per_mwh);
        Ok(())
    }

    /// Returns the administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_admin(&env)
    }

    /// Returns the configured price of one watt-hour in the smallest currency unit.
    pub fn price(env: Env) -> Result<i128, Error> {
        Self::require_configured(&env)
    }

    /// Ingests a consumption payload emitted by a physical meter and charges the
    /// account `milli_wh * price_per_mwh / 1000`. Idempotent on the
    /// `(account, nonce)` pair: a replayed nonce is rejected before any charge
    /// is applied. The first reading for an account binds that account to its
    /// meter node, and later readings from a different node are refused.
    /// Returns the amount charged for this reading.
    pub fn record(
        env: Env,
        account: Address,
        node_id: u32,
        milli_wh: i128,
        nonce: u64,
    ) -> Result<i128, Error> {
        let price = Self::require_configured(&env)?;
        if milli_wh <= 0 {
            return Err(Error::InvalidReading);
        }
        account.require_auth();

        if Self::seen_nonce_of(&env, &account, nonce) {
            return Err(Error::DuplicateNonce);
        }

        match env
            .storage()
            .persistent()
            .get::<_, u32>(&(symbol_short!("node"), account.clone()))
        {
            Some(bound) => {
                if bound != node_id {
                    return Err(Error::NodeMismatch);
                }
            }
            None => env
                .storage()
                .persistent()
                .set(&(symbol_short!("node"), account.clone()), &node_id),
        }

        let charge = milli_wh
            .checked_mul(price)
            .ok_or(Error::ArithmeticOverflow)?
            / CHARGE_SCALE;

        let unsettled = Self::unsettled_of(&env, &account)
            .checked_add(charge)
            .ok_or(Error::ArithmeticOverflow)?;
        let count = Self::reading_count_of(&env, &account)
            .checked_add(1)
            .ok_or(Error::ArithmeticOverflow)?;

        env.storage()
            .persistent()
            .set(&(symbol_short!("bal"), account.clone()), &unsettled);
        env.storage()
            .persistent()
            .set(&(symbol_short!("cnt"), account.clone()), &count);
        env.storage()
            .persistent()
            .set(&(symbol_short!("nnc"), account.clone(), nonce), &true);
        Ok(charge)
    }

    /// Closes out an account's unsettled balance, resetting it to zero.
    /// Cumulative reading counts and the bound meter node are preserved across
    /// billing cycles. Returns the settled amount.
    pub fn settle(env: Env, account: Address) -> Result<i128, Error> {
        Self::require_configured(&env)?;
        account.require_auth();

        let unsettled = Self::unsettled_of(&env, &account);
        if unsettled <= 0 {
            return Err(Error::NothingToSettle);
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("bal"), account), &0i128);
        Ok(unsettled)
    }

    /// Currently unsettled balance owed by an account.
    pub fn unsettled(env: Env, account: Address) -> Result<i128, Error> {
        Self::require_configured(&env)?;
        Ok(Self::unsettled_of(&env, &account))
    }

    /// Total number of readings ever accepted from an account's meter.
    pub fn reading_count(env: Env, account: Address) -> Result<u32, Error> {
        Self::require_configured(&env)?;
        Ok(Self::reading_count_of(&env, &account))
    }

    /// Reports whether a nonce has already been consumed by an account.
    pub fn seen_nonce(env: Env, account: Address, nonce: u64) -> Result<bool, Error> {
        Self::require_configured(&env)?;
        Ok(Self::seen_nonce_of(&env, &account, nonce))
    }

    /// Meter node an account is permanently bound to. Rejected while the
    /// account has no recorded readings.
    pub fn bound_node(env: Env, account: Address) -> Result<u32, Error> {
        Self::require_configured(&env)?;
        env.storage()
            .persistent()
            .get(&(symbol_short!("node"), account))
            .ok_or(Error::InvalidReading)
    }

    fn require_configured(env: &Env) -> Result<i128, Error> {
        if !env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::NotAdmin);
        }
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("price"))
            .unwrap_or(0))
    }

    fn require_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotAdmin)
    }

    fn unsettled_of(env: &Env, account: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("bal"), account))
            .unwrap_or(0)
    }

    fn reading_count_of(env: &Env, account: &Address) -> u32 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("cnt"), account))
            .unwrap_or(0)
    }

    fn seen_nonce_of(env: &Env, account: &Address, nonce: u64) -> bool {
        env.storage()
            .persistent()
            .has(&(symbol_short!("nnc"), account, nonce))
    }
}
