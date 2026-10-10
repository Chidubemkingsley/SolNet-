//! SolNet: Micro Billing Contract
//! Description: Processes high-frequency data packets to calculate real-time consumption and bill users.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, vec, Address, Env, IntoVal, Symbol, Val,
    Vec,
};

/// Readings are reported in milli-watt-hours while the tariff is quoted per
/// watt-hour, so the charge is divided by 1000.
const CHARGE_SCALE: i128 = 1_000;

/// Mirrors the scale factor the account ledger uses to express its
/// Watt-credit conversion rate. Must stay in lockstep with that contract.
const RATE_SCALE: i128 = 1_000_000;

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
    LedgerCallFailed = 9,
    RouterNotAuthorized = 10,
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
        Self::bump_instance(&env);
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

    /// Points this router at the shared account ledger it bills against. Until
    /// this is called the router operates standalone and accumulates unsettled
    /// charges without debiting any account, which is the correct behaviour for
    /// a grid that has not yet settled its accounts with a settlement rail.
    ///
    /// The ledger's Watt-credit conversion rate is captured here rather than
    /// re-read on every reading. That is safe because the ledger fixes its rate
    /// at initialization and exposes no setter, so the cached value cannot go
    /// stale. It also keeps telemetry ingestion to a single cross-contract call
    /// per reading, which matters for the high-frequency path.
    pub fn wire(env: Env, admin: Address, ledger: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        let self_addr = env.current_contract_address();
        let ok = env
            .try_invoke_contract::<i128, Error>(&ledger, &symbol_short!("rate"), vec![&env]);
        let rate: i128 = match ok {
            Ok(Ok(r)) => r,
            _ => return Err(Error::LedgerCallFailed),
        };
        if rate <= 0 {
            return Err(Error::LedgerCallFailed);
        }
        let authorized = env.try_invoke_contract::<bool, Error>(
            &ledger,
            &symbol_short!("is_router"),
            vec![&env, self_addr.into_val(&env)],
        );
        let authorized: bool = match authorized {
            Ok(Ok(b)) => b,
            _ => return Err(Error::LedgerCallFailed),
        };
        if !authorized {
            return Err(Error::RouterNotAuthorized);
        }
        env.storage().instance().set(&symbol_short!("ledg"), &ledger);
        env.storage().instance().set(&symbol_short!("rat"), &rate);
        Self::bump_instance(&env);
        Ok(())
    }

    /// The account ledger this router bills against, if one has been wired.
    pub fn ledger(env: Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("ledg"))
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
            None => {
                env.storage()
                    .persistent()
                    .set(&(symbol_short!("node"), account.clone()), &node_id);
                Self::bump(&env, &(symbol_short!("node"), account.clone()));
            }
        }

        let charge = milli_wh
            .checked_mul(price)
            .ok_or(Error::ArithmeticOverflow)?
            / CHARGE_SCALE;

        Self::charge_ledger(&env, &account, charge)?;

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
        Self::bump(&env, &(symbol_short!("bal"), account.clone()));
        Self::bump(&env, &(symbol_short!("cnt"), account.clone()));
        Self::bump_nonce(&env, &(symbol_short!("nnc"), account, nonce));
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
            .set(&(symbol_short!("bal"), account.clone()), &0i128);
        Self::bump(&env, &(symbol_short!("bal"), account));
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

    /// Same renewal as `bump` for the three-part nonce replay-guard keys,
    /// which are keyed by account and nonce rather than by account alone.
    fn bump_nonce(env: &Env, key: &(Symbol, Address, u64)) {
        let max = env.storage().max_ttl();
        if max == 0 {
            return;
        }
        env.storage().persistent().extend_ttl(key, max / 2, max);
    }

    fn charge_ledger(env: &Env, account: &Address, charge: i128) -> Result<(), Error> {
        let ledger: Option<Address> = env.storage().instance().get(&symbol_short!("ledg"));
        let (ledger, rate) = match ledger {
            Some(l) => {
                let r: i128 = env
                    .storage()
                    .instance()
                    .get(&symbol_short!("rat"))
                    .unwrap_or(0);
                (l, r)
            }
            None => return Ok(()),
        };

        let credits = charge
            .checked_mul(rate)
            .ok_or(Error::ArithmeticOverflow)?
            / RATE_SCALE;

        if credits == 0 {
            return Ok(());
        }

        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(env.current_contract_address().into_val(env));
        args.push_back(account.clone().into_val(env));
        args.push_back(credits.into_val(env));
        let fn_name = Symbol::new(env, "apply_consumption");
        let res = env.try_invoke_contract::<i128, Error>(&ledger, &fn_name, args);
        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::RouterNotAuthorized),
        }
    }

    fn require_admin_auth(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored = Self::require_admin(env)?;
        if &stored != admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();
        Ok(())
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
