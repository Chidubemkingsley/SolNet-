//! SolNet: Infrastructure Battery Pool Contract
//! Description: Custodies the automated, fractional slice of every settlement that
//! the microgrid routes into a reserve spent strictly on the physical maintenance
//! and eventual replacement of the grid's battery banks. No other module may draw
//! on this reserve, which is what makes the pool auditable.
//!
//! The reserve is split into two non-overlapping buckets:
//!
//! * `total_reserve` — every unit of currency the pool has ever taken in.
//! * `reserved_for_maintenance` — units earmarked against a specific job, keyed by
//!   the 32-byte hash that identifies the work order (the off-chain maintenance
//!   record that authorised it).
//!
//! and the accounting invariant `total_reserve == reserved_for_maintenance + available`
//! holds at every point in the contract's life.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Address, BytesN, Env,
};

/// Fixed-point denominator: 10000 basis points == 100%.
const BPS_DENOM: i128 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    InvalidFeeBps = 4,
    InvalidAmount = 5,
    InvalidSettlement = 6,
    InsufficientReserve = 7,
    AlreadyReserved = 8,
    NotReserved = 9,
    ArithmeticOverflow = 10,
}

#[contract]
pub struct BatteryPool;

#[contractimpl]
impl BatteryPool {
    /// Initializes the pool with the settlement fee, in basis points, that is
    /// automatically routed into the maintenance reserve. It must be within
    /// `0..=10000`; zero disables the levy but keeps the pool usable for
    /// voluntary contributions.
    pub fn init(env: Env, admin: Address, fee_bps: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        if !(0..=BPS_DENOM).contains(&fee_bps) {
            return Err(Error::InvalidFeeBps);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("fee"), &fee_bps);
        env.storage().instance().set(&symbol_short!("tres"), &0i128);
        env.storage().instance().set(&symbol_short!("resvd"), &0i128);
        Ok(())
    }

    /// Returns the administrator address recorded at initialization.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("admin")).unwrap())
    }

    /// Returns the settlement fee rate in basis points.
    pub fn fee_bps(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("fee")).unwrap())
    }

    /// Adds an explicit top-up to the maintenance reserve.
    pub fn contribute(env: Env, account: Address, amount: i128) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        account.require_auth();

        let total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("tres"))
            .unwrap_or(0);
        let updated = total.checked_add(amount).ok_or(Error::ArithmeticOverflow)?;
        env.storage().instance().set(&symbol_short!("tres"), &updated);

        let prior: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("paid"), &account))
            .unwrap_or(0);
        let rolled = prior.checked_add(amount).ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .persistent()
            .set(&(symbol_short!("paid"), &account), &rolled);
        Ok(())
    }

    /// Levies the configured maintenance fee against a settlement and books it into
    /// the reserve, returning the amount collected. A settlement that rounds down to
    /// zero fee is accepted as a no-op.
    pub fn collect_fee(env: Env, settlement_amount: i128) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        if settlement_amount < 0 {
            return Err(Error::InvalidSettlement);
        }
        let fee_bps: i128 = env.storage().instance().get(&symbol_short!("fee")).unwrap();
        let collected = settlement_amount
            .checked_mul(fee_bps)
            .ok_or(Error::ArithmeticOverflow)?
            / BPS_DENOM;

        let total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("tres"))
            .unwrap_or(0);
        let updated = total.checked_add(collected).ok_or(Error::ArithmeticOverflow)?;
        env.storage().instance().set(&symbol_short!("tres"), &updated);
        Ok(collected)
    }

    /// Earmarks `amount` of the currently available reserve against the maintenance
    /// job identified by `reason_hash`. The same hash cannot be earmarked twice and
    /// the pool can never be over-committed.
    pub fn reserve_for_maintenance(
        env: Env,
        admin: Address,
        amount: i128,
        reason_hash: BytesN<32>,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if Self::reservation_of(&env, &reason_hash).is_some() {
            return Err(Error::AlreadyReserved);
        }
        let total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("tres"))
            .unwrap_or(0);
        let reserved: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("resvd"))
            .unwrap_or(0);
        let available = total.checked_sub(reserved).ok_or(Error::ArithmeticOverflow)?;
        if amount > available {
            return Err(Error::InsufficientReserve);
        }

        let updated = reserved.checked_add(amount).ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&symbol_short!("resvd"), &updated);
        env.storage()
            .persistent()
            .set(&(symbol_short!("rsv"), reason_hash), &amount);
        Ok(())
    }

    /// Closes out an earmarked maintenance job once the work is invoiced, releasing
    /// `spent` from both the reserve and the earmark and returning `spent`. Whatever
    /// the job did not consume returns to `available`. Only the administrator may
    /// settle, and `spent` can never exceed the earmark.
    pub fn settle_maintenance(
        env: Env,
        admin: Address,
        reason_hash: BytesN<32>,
        spent: i128,
    ) -> Result<i128, Error> {
        Self::require_admin_auth(&env, &admin)?;
        if spent < 0 {
            return Err(Error::InvalidAmount);
        }
        let earmarked = Self::reservation_of(&env, &reason_hash).ok_or(Error::NotReserved)?;
        if spent > earmarked {
            return Err(Error::InvalidAmount);
        }

        let total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("tres"))
            .unwrap_or(0);
        let reserved: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("resvd"))
            .unwrap_or(0);
        let new_total = total.checked_sub(spent).ok_or(Error::ArithmeticOverflow)?;
        let new_reserved = reserved
            .checked_sub(spent)
            .ok_or(Error::ArithmeticOverflow)?;

        env.storage().instance().set(&symbol_short!("tres"), &new_total);
        env.storage()
            .instance()
            .set(&symbol_short!("resvd"), &new_reserved);
        let leftover = earmarked.checked_sub(spent).ok_or(Error::ArithmeticOverflow)?;
        if leftover == 0 {
            env.storage()
                .persistent()
                .remove(&(symbol_short!("rsv"), &reason_hash));
        } else {
            env.storage()
                .persistent()
                .set(&(symbol_short!("rsv"), &reason_hash), &leftover);
        }
        Ok(spent)
    }

    /// Total currency held by the pool, reserved and unreserved alike.
    pub fn total_reserve(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(Self::read_total(&env))
    }

    /// Currency currently earmarked against specific maintenance jobs.
    pub fn reserved_for_maintenance(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(Self::read_reserved(&env))
    }

    /// Currency the administrator may still earmark for new maintenance work.
    pub fn available(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Self::read_total(&env)
            .checked_sub(Self::read_reserved(&env))
            .ok_or(Error::ArithmeticOverflow)
    }

    /// Reports whether a work-order hash currently holds an earmark.
    pub fn is_reserved(env: Env, reason_hash: BytesN<32>) -> Result<bool, Error> {
        Self::require_initialized(&env)?;
        Ok(Self::reservation_of(&env, &reason_hash).is_some())
    }

    /// Returns the amount earmarked against a work-order hash, zero when absent.
    pub fn reserved_amount(env: Env, reason_hash: BytesN<32>) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(Self::reservation_of(&env, &reason_hash).unwrap_or(0))
    }

    /// Total this account has voluntarily contributed to the reserve.
    pub fn contribution(env: Env, account: Address) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env
            .storage()
            .persistent()
            .get(&(symbol_short!("paid"), account))
            .unwrap_or(0))
    }

    fn reservation_of(env: &Env, reason_hash: &BytesN<32>) -> Option<i128> {
        env.storage()
            .persistent()
            .get(&(symbol_short!("rsv"), reason_hash))
    }

    fn read_total(env: &Env) -> i128 {
        env.storage().instance().get(&symbol_short!("tres")).unwrap_or(0)
    }

    fn read_reserved(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("resvd"))
            .unwrap_or(0)
    }

    fn require_initialized(env: &Env) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            Ok(())
        } else {
            Err(Error::NotInitialized)
        }
    }

    fn require_admin_auth(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotInitialized)?;
        if &stored != admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();
        Ok(())
    }
}