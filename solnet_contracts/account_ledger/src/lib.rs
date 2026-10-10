//! SolNet: Account Ledger Contract
//! Description: Maintains localized user balances, managing stablecoin deposits and internal Watt-credit tallies.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, symbol_short, Address, Env};

/// Number of decimal places the Watt-credit conversion rate is scaled by.
/// Mirrors the sub-unit precision of the fiat-pegged stablecoins used by the
/// Stellar Anchors, so that one stablecoin unit is `RATE_SCALE` micro-units.
const RATE_SCALE: i128 = 1_000_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    InvalidRate = 3,
    InvalidAmount = 4,
    InvalidCredits = 5,
    InsufficientCredits = 6,
    InsufficientFunds = 7,
    ArithmeticOverflow = 8,
}

#[contract]
pub struct AccountLedger;

#[contractimpl]
impl AccountLedger {
    /// Initializes the account ledger with the administrator and the
    /// Watt-credit conversion rate expressed as Watt-credits per stablecoin
    /// unit scaled by 1e6.
    pub fn init(env: Env, admin: Address, watts_per_unit: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        if watts_per_unit <= 0 {
            return Err(Error::InvalidRate);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&symbol_short!("admin"), &admin);
        env.storage()
            .instance()
            .set(&symbol_short!("rate"), &watts_per_unit);
        Ok(())
    }

    /// Returns the administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_admin(&env)
    }

    /// Returns the Watt-credits minted per stablecoin unit, scaled by 1e6.
    pub fn rate(env: Env) -> Result<i128, Error> {
        Self::require_configured(&env)
    }

    /// Credits a stablecoin deposit to an account and mints the equivalent
    /// number of Watt-credits at the configured conversion rate.
    /// Returns the number of Watt-credits minted.
    pub fn deposit(env: Env, account: Address, amount: i128) -> Result<i128, Error> {
        let rate = Self::require_configured(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        account.require_auth();

        let minted = amount.checked_mul(rate).ok_or(Error::ArithmeticOverflow)? / RATE_SCALE;

        let stable = Self::stable_balance_of(&env, &account)
            .checked_add(amount)
            .ok_or(Error::ArithmeticOverflow)?;
        let watts = Self::watt_balance_of(&env, &account)
            .checked_add(minted)
            .ok_or(Error::ArithmeticOverflow)?;

        env.storage()
            .persistent()
            .set(&(symbol_short!("bal"), account.clone()), &stable);
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &watts);
        Ok(minted)
    }

    /// Burns Watt-credits from an account and returns the stablecoin value that
    /// they represent. Rejected when the request would overdraw the account's
    /// Watt-credit balance, when the stablecoin reserve cannot cover the payout,
    /// or when the credit amount is too small to produce any stablecoin value.
    pub fn withdraw(env: Env, account: Address, credits: i128) -> Result<i128, Error> {
        let rate = Self::require_configured(&env)?;
        if credits <= 0 {
            return Err(Error::InvalidCredits);
        }
        account.require_auth();

        let watts = Self::watt_balance_of(&env, &account);
        if credits > watts {
            return Err(Error::InsufficientCredits);
        }

        let amount = credits
            .checked_mul(RATE_SCALE)
            .ok_or(Error::ArithmeticOverflow)?
            / rate;
        if amount <= 0 {
            return Err(Error::InvalidCredits);
        }

        let stable = Self::stable_balance_of(&env, &account);
        if amount > stable {
            return Err(Error::InsufficientFunds);
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("bal"), account.clone()), &(stable - amount));
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &(watts - credits));
        Ok(amount)
    }

    /// Administrator hook that applies consumption-derived Watt-credits to an
    /// account. The delta is signed: negative values burn previously minted
    /// Watt-credits and are permitted to drive the Watt balance below zero so
    /// that accrued supply debt is visible to the delinquency disconnector.
    /// Returns the resulting Watt-credit balance.
    pub fn credit_watts(
        env: Env,
        admin: Address,
        account: Address,
        milli_wh: i128,
    ) -> Result<i128, Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if milli_wh == 0 {
            return Err(Error::InvalidAmount);
        }

        let next = Self::watt_balance_of(&env, &account)
            .checked_add(milli_wh)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &next);
        Ok(next)
    }

    /// Stablecoin balance held by an account, in the smallest currency unit.
    pub fn stable_balance(env: Env, account: Address) -> Result<i128, Error> {
        Self::require_configured(&env)?;
        Ok(Self::stable_balance_of(&env, &account))
    }

    /// Watt-credit balance held by an account. May be negative when the account
    /// has accrued unpaid consumption debt.
    pub fn watt_balance(env: Env, account: Address) -> Result<i128, Error> {
        Self::require_configured(&env)?;
        Ok(Self::watt_balance_of(&env, &account))
    }

    fn require_configured(env: &Env) -> Result<i128, Error> {
        if !env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::NotAdmin);
        }
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("rate"))
            .unwrap_or(0))
    }

    fn require_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotAdmin)
    }

    fn require_admin_auth(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored = Self::require_admin(env)?;
        if &stored != admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();
        Ok(())
    }

    fn stable_balance_of(env: &Env, account: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("bal"), account))
            .unwrap_or(0)
    }

    fn watt_balance_of(env: &Env, account: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("wat"), account))
            .unwrap_or(0)
    }
}
