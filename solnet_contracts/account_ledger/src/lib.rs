//! SolNet: Account Ledger Contract
//! Description: Maintains localized user balances, managing stablecoin deposits and internal Watt-credit tallies.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Address, Env, IntoVal, Symbol, Val, Vec,
};

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
    NotARouter = 9,
    AlreadyARouter = 10,
    TokenNotSet = 11,
    TokenTransferFailed = 12,
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
        Self::bump_instance(&env);
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
        Self::bump(&env, &(symbol_short!("bal"), account.clone()));
        Self::bump(&env, &(symbol_short!("wat"), account.clone()));
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
        Self::bump(&env, &(symbol_short!("bal"), account.clone()));
        Self::bump(&env, &(symbol_short!("wat"), account.clone()));
        Ok(amount)
    }

    /// Administrator hook that applies consumption-derived Watt-credits to an
    /// account. The delta is signed: negative values burn previously minted
    /// Watt-credits and are permitted to drive the Watt balance below zero so
    /// that accrued supply debt is visible to the delinquency disconnector.
    /// The delta is in Watt-credits (already converted from milli-watt-hours
    /// at the ledger's configured rate). Returns the resulting Watt-credit balance.
    pub fn credit_watts(
        env: Env,
        admin: Address,
        account: Address,
        watts_delta: i128,
    ) -> Result<i128, Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if watts_delta == 0 {
            return Err(Error::InvalidAmount);
        }

        let next = Self::watt_balance_of(&env, &account)
            .checked_add(watts_delta)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &next);
        Self::bump(&env, &(symbol_short!("wat"), account.clone()));
        Ok(next)
    }

    /// Grants a billing router the capability to charge consumption against
    /// accounts on this ledger. Only an authorized router may call
    /// `apply_consumption`. This is what lets the micro-billing router debit a
    /// shared ledger without being handed the ledger administrator key.
    pub fn authorize_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if Self::is_router_of(&env, &router) {
            return Err(Error::AlreadyARouter);
        }
        env.storage()
            .persistent()
            .set(&(symbol_short!("rtr"), router.clone()), &true);
        Self::bump(&env, &(symbol_short!("rtr"), router));
        Ok(())
    }

    /// Revokes a previously granted billing router capability.
    pub fn revoke_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if !Self::is_router_of(&env, &router) {
            return Err(Error::NotARouter);
        }
        env.storage()
            .persistent()
            .remove(&(symbol_short!("rtr"), router));
        Ok(())
    }

    /// Reports whether an address currently holds the billing router capability.
    pub fn is_router(env: Env, router: Address) -> bool {
        Self::is_router_of(&env, &router)
    }

    /// Burns Watt-credits against an account on behalf of an authorized billing
    /// router. The balance is permitted to fall below zero, which is how accrued
    /// supply debt becomes visible to the delinquency disconnector.
    ///
    /// Debit-only by construction: `credits` must be strictly positive, so a
    /// router can charge an account but can never credit it. Returns the
    /// resulting Watt-credit balance.
    pub fn apply_consumption(
        env: Env,
        router: Address,
        account: Address,
        credits: i128,
    ) -> Result<i128, Error> {
        Self::require_configured(&env)?;
        if credits <= 0 {
            return Err(Error::InvalidAmount);
        }
        if !Self::is_router_of(&env, &router) {
            return Err(Error::NotARouter);
        }
        router.require_auth();

        let next = Self::watt_balance_of(&env, &account)
            .checked_sub(credits)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &next);
        Self::bump(&env, &(symbol_short!("wat"), account));
        Ok(next)
    }

    fn is_router_of(env: &Env, router: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("rtr"), router))
            .unwrap_or(false)
    }

    /// Points this ledger at the Stellar Asset Contract whose tokens back the
    /// secured deposit/withdraw paths. The asset is deliberately generic: on
    /// testnet this is typically the native XLM SAC, on mainnet a USDC SAC.
    /// Until this is set, `deposit_secured` and `withdraw_secured` are
    /// unavailable and fail with `TokenNotSet`. The plain `deposit`/`withdraw`
    /// pair is intentionally left untouched so existing integrations keep
    /// working — but note the asymmetry: credits minted by plain `deposit`
    /// have no backing tokens behind them.
    pub fn set_token(env: Env, admin: Address, token: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        env.storage().instance().set(&symbol_short!("token"), &token);
        Self::bump_instance(&env);
        Ok(())
    }

    /// The backing token, if one has been configured.
    pub fn token(env: Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("token"))
    }

    /// Deposit backed 1:1 by real tokens. Behaves exactly like
    /// `deposit`, except `amount` of the configured token is pulled from the
    /// caller into this contract via the token's `transfer` before any state
    /// is written, so a failed transfer leaves no partial state behind.
    ///
    /// Dust is rejected up front: if `amount` is too small to mint at least
    /// one Watt-credit, the call fails rather than taking tokens for nothing.
    /// Returns the number of Watt-credits minted.
    pub fn deposit_secured(
        env: Env,
        account: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        let rate = Self::require_configured(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        account.require_auth();
        let token: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("token"))
            .ok_or(Error::TokenNotSet)?;

        let minted = amount.checked_mul(rate).ok_or(Error::ArithmeticOverflow)? / RATE_SCALE;
        if minted <= 0 {
            return Err(Error::InvalidAmount);
        }

        let this = env.current_contract_address();
        Self::token_transfer(&env, &token, &account, &this, &amount)?;

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
        Self::bump(&env, &(symbol_short!("bal"), account.clone()));
        Self::bump(&env, &(symbol_short!("wat"), account.clone()));
        Ok(minted)
    }

    /// Withdraw backed 1:1 by real tokens. Behaves exactly like
    /// `withdraw`, except the stablecoin payout is pushed from this contract
    /// to the caller via the token's `transfer` before any state is written.
    /// Returns the stablecoin amount paid out.
    pub fn withdraw_secured(
        env: Env,
        account: Address,
        credits: i128,
    ) -> Result<i128, Error> {
        let rate = Self::require_configured(&env)?;
        if credits <= 0 {
            return Err(Error::InvalidCredits);
        }
        account.require_auth();
        let token: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("token"))
            .ok_or(Error::TokenNotSet)?;

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

        let this = env.current_contract_address();
        Self::token_transfer(&env, &token, &this, &account, &amount)?;

        env.storage()
            .persistent()
            .set(&(symbol_short!("bal"), account.clone()), &(stable - amount));
        env.storage()
            .persistent()
            .set(&(symbol_short!("wat"), account.clone()), &(watts - credits));
        Self::bump(&env, &(symbol_short!("bal"), account.clone()));
        Self::bump(&env, &(symbol_short!("wat"), account.clone()));
        Ok(amount)
    }

    fn token_transfer(
        env: &Env,
        token: &Address,
        from: &Address,
        to: &Address,
        amount: &i128,
    ) -> Result<(), Error> {
        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(from.clone().into_val(env));
        args.push_back(to.clone().into_val(env));
        args.push_back(amount.clone().into_val(env));
        let res =
            env.try_invoke_contract::<Val, Error>(token, &Symbol::new(env, "transfer"), args);
        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::TokenTransferFailed),
        }
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

    /// Extends the ledger lifetime of a persistent entry that has just been
    /// written. Deriving both bounds from the network maximum keeps the
    /// renewal policy adaptive: an entry is renewed once less than half of its
    /// life remains and is restored to the full maximum, and nothing has to be
    /// configured at initialization.
    fn bump(env: &Env, key: &(Symbol, Address)) {
        let max = env.storage().max_ttl();
        if max == 0 {
            return;
        }
        env.storage().persistent().extend_ttl(key, max / 2, max);
    }

    /// Extends the ledger lifetime of the contract instance that has just been
    /// written. Deriving both bounds from the network maximum keeps the
    /// renewal policy adaptive: the configuration is renewed once less than half
    /// of its life remains and is restored to the full maximum, and nothing has
    /// to be configured at initialization. Instance storage expires as a single
    /// ledger entry, so one renewal covers every key written by the caller.
    fn bump_instance(env: &Env) {
        let max = env.storage().max_ttl();
        if max == 0 {
            return;
        }
        env.storage().instance().extend_ttl(max / 2, max);
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
