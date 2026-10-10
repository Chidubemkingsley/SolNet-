//! SolNet: Community Capital Fund Contract
//! Description: Crowdfunding portal that lets external ecosystem investors
//! supply expansion capital for physical grid builds and claim programmatic
//! pro-rata dividend yields on the shares they receive.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Address, Env, IntoVal, Val,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    InvalidYieldBps = 4,
    InvalidAmount = 5,
    ZeroShares = 6,
    NoShares = 7,
    NothingAccrued = 8,
    DuplicateAccrual = 9,
    ArithmeticOverflow = 10,
}

/// Basis-point denominator: 10000 bps == 100%.
const BPS_SCALE: i128 = 10_000;
/// Highest accepted annual rate: one accrual period may never yield > 100%.
const MAX_YIELD_BPS: i128 = 10_000;
/// Fixed-point scale of the share price; 10000 means one whole capital unit.
const PRICE_SCALE: i128 = 10_000;
/// Fixed-point scale of the cumulative dividend-per-share index.
const INDEX_SCALE: i128 = 1_000_000_000_000_000;
/// Tombstone written before the first accrual so ledger sequence 0 can accrue.
const NEVER_ACCRUED: u32 = u32::MAX;

#[contract]
pub struct CommunityCapital;

#[contractimpl]
impl CommunityCapital {
    /// Initializes the community capital fund with its administrator and the
    /// annual dividend rate expressed in basis points (10000 bps == 100%).
    pub fn init(env: Env, admin: Address, yield_bps: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        if !(0..=MAX_YIELD_BPS).contains(&yield_bps) {
            return Err(Error::InvalidYieldBps);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("ybps"), &yield_bps);
        env.storage().instance().set(&symbol_short!("tshr"), &0u64);
        env.storage().instance().set(&symbol_short!("capl"), &0i128);
        env.storage().instance().set(&symbol_short!("unds"), &0i128);
        env.storage().instance().set(&symbol_short!("indx"), &0i128);
        env.storage()
            .instance()
            .set(&symbol_short!("lastq"), &NEVER_ACCRUED);
        Self::bump_instance(&env);
        Ok(())
    }

    /// Returns the administrator address of the fund.
    #[allow(clippy::needless_question_mark)]
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_init(&env)?;
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotInitialized)?)
    }

    /// Supplies expansion capital and mints shares to `investor` at the
    /// current share price. Returns the number of shares issued.
    pub fn invest(env: Env, investor: Address, amount: i128) -> Result<u64, Error> {
        Self::require_init(&env)?;
        investor.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let price = Self::read_share_price(&env);
        let scaled = amount
            .checked_mul(PRICE_SCALE)
            .ok_or(Error::ArithmeticOverflow)?;
        let shares_raw = scaled.checked_div(price).ok_or(Error::ArithmeticOverflow)?;
        if shares_raw <= 0 {
            return Err(Error::ZeroShares);
        }
        let shares = u64::try_from(shares_raw).map_err(|_| Error::ArithmeticOverflow)?;

        let held = Self::load_shares(&env, &investor);
        let next_held = held.checked_add(shares).ok_or(Error::ArithmeticOverflow)?;
        let total_shares: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("tshr"))
            .unwrap_or(0);
        let next_total = total_shares.checked_add(shares).ok_or(Error::ArithmeticOverflow)?;
        let capital: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("capl"))
            .unwrap_or(0);
        let next_capital = capital.checked_add(amount).ok_or(Error::ArithmeticOverflow)?;

        env.storage()
            .persistent()
            .set(&(symbol_short!("sh"), investor.clone()), &next_held);
        Self::bump(&env, &(symbol_short!("sh"), investor.clone()));
        if held == 0 {
            env.storage()
                .persistent()
                .set(&(symbol_short!("clmd"), investor.clone()), &Self::read_index(&env));
            Self::bump(&env, &(symbol_short!("clmd"), investor));
        }
        env.storage()
            .instance()
            .set(&symbol_short!("tshr"), &next_total);
        env.storage()
            .instance()
            .set(&symbol_short!("capl"), &next_capital);
        Self::bump_instance(&env);
        Ok(shares)
    }

    /// Books one accrual period of dividends into the undistributed pool and
    /// returns the amount booked. Callable at most once per ledger sequence.
    pub fn accrue_yield(env: Env, admin: Address) -> Result<i128, Error> {
        Self::require_admin_auth(&env, &admin)?;

        let total_shares: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("tshr"))
            .unwrap_or(0);
        if total_shares == 0 {
            return Err(Error::NoShares);
        }

        let sequence = env.ledger().sequence();
        let last: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("lastq"))
            .unwrap_or(NEVER_ACCRUED);
        if sequence == last {
            return Err(Error::DuplicateAccrual);
        }

        let yield_bps: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("ybps"))
            .unwrap_or(0);
        let capital: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("capl"))
            .unwrap_or(0);
        let gross = capital
            .checked_mul(yield_bps)
            .ok_or(Error::ArithmeticOverflow)?;
        let dividend = gross.checked_div(BPS_SCALE).ok_or(Error::ArithmeticOverflow)?;
        if dividend <= 0 {
            return Err(Error::NothingAccrued);
        }

        let delta = dividend
            .checked_mul(INDEX_SCALE)
            .ok_or(Error::ArithmeticOverflow)?
            .checked_div(total_shares as i128)
            .ok_or(Error::ArithmeticOverflow)?;
        if delta <= 0 {
            return Err(Error::NothingAccrued);
        }

        let undistributed: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("unds"))
            .unwrap_or(0);
        let next_undistributed = undistributed
            .checked_add(dividend)
            .ok_or(Error::ArithmeticOverflow)?;
        let next_capital = capital
            .checked_add(dividend)
            .ok_or(Error::ArithmeticOverflow)?;
        let index = Self::read_index(&env);
        let next_index = index.checked_add(delta).ok_or(Error::ArithmeticOverflow)?;

        env.storage().instance().set(&symbol_short!("lastq"), &sequence);
        env.storage()
            .instance()
            .set(&symbol_short!("unds"), &next_undistributed);
        env.storage()
            .instance()
            .set(&symbol_short!("capl"), &next_capital);
        env.storage()
            .instance()
            .set(&symbol_short!("indx"), &next_index);
        Self::bump_instance(&env);
        Ok(dividend)
    }

    /// Pays the caller the dividends their shares earned and reduces the
    /// undistributed pool by exactly the amount paid out. Entitlement is
    /// derived from the cumulative dividend-per-share index, so the result
    /// never depends on the order in which holders claim. Any integer
    /// division remainder deterministically stays in the undistributed pool
    /// and rolls forward into the next accrual, which keeps
    /// `undistributed + total claimed == total accrued` exact.
    pub fn claim_dividend(env: Env, investor: Address) -> Result<i128, Error> {
        Self::require_init(&env)?;
        investor.require_auth();

        let shares = Self::load_shares(&env, &investor);
        if shares == 0 {
            return Err(Error::NoShares);
        }
        let undistributed: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("unds"))
            .unwrap_or(0);
        if undistributed <= 0 {
            return Err(Error::NothingAccrued);
        }
        let index = Self::read_index(&env);
        let entitled = index
            .checked_mul(shares as i128)
            .ok_or(Error::ArithmeticOverflow)?
            .checked_div(INDEX_SCALE)
            .ok_or(Error::ArithmeticOverflow)?;
        let claimed: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("clmd"), investor.clone()))
            .unwrap_or(0);
        let payout = entitled.checked_sub(claimed).ok_or(Error::ArithmeticOverflow)?;
        if payout <= 0 {
            return Err(Error::NothingAccrued);
        }

        env.storage()
            .instance()
            .set(&symbol_short!("unds"), &(undistributed - payout));
        env.storage()
            .persistent()
            .set(&(symbol_short!("clmd"), investor.clone()), &entitled);
        Self::bump(&env, &(symbol_short!("clmd"), investor));
        Self::bump_instance(&env);
        Ok(payout)
    }

    /// Shares currently held by `investor`.
    pub fn shares_of(env: Env, investor: Address) -> u64 {
        Self::load_shares(&env, &investor)
    }

    /// Shares outstanding across every investor.
    pub fn total_shares(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&symbol_short!("tshr"))
            .unwrap_or(0)
    }

    /// Dividends booked but not yet claimed by any investor.
    pub fn undistributed(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("unds"))
            .unwrap_or(0)
    }

    /// Configured annual dividend rate in basis points.
    pub fn yield_bps(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("ybps"))
            .unwrap_or(0)
    }

    /// Capital backing one share, scaled by 10000. Rises as yield accrues.
    pub fn share_price(env: Env) -> i128 {
        Self::read_share_price(&env)
    }

    /// Total net asset value backing all outstanding shares.
    pub fn total_capital(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("capl"))
            .unwrap_or(0)
    }

    /// Cumulative dividend per share, scaled by 1000000000000000.
    pub fn dividend_index(env: Env) -> i128 {
        Self::read_index(&env)
    }

    fn read_index(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("indx"))
            .unwrap_or(0)
    }

    fn load_shares(env: &Env, investor: &Address) -> u64 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("sh"), investor.clone()))
            .unwrap_or(0)
    }

    fn read_share_price(env: &Env) -> i128 {
        let total_shares: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("tshr"))
            .unwrap_or(0);
        if total_shares == 0 {
            return PRICE_SCALE;
        }
        let capital: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("capl"))
            .unwrap_or(0);
        let price = capital
            .checked_mul(PRICE_SCALE)
            .and_then(|scaled| scaled.checked_div(total_shares as i128))
            .unwrap_or(0);
        if price > 0 {
            price
        } else {
            PRICE_SCALE
        }
    }

    fn require_init(env: &Env) -> Result<(), Error> {
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

    /// Extends the ledger lifetime of a persistent entry that has just been
    /// written. Deriving both bounds from the network maximum keeps the
    /// renewal policy adaptive: an entry is renewed once less than half of its
    /// life remains and is restored to the full maximum, and nothing has to be
    /// configured at initialization.
    fn bump<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
        let max = env.storage().max_ttl();
        if max > 0 {
            env.storage().persistent().extend_ttl(key, max / 2, max);
        }
    }

    /// Extends the ledger lifetime of the contract instance that has just been
    /// written. Deriving both bounds from the network maximum keeps the
    /// renewal policy adaptive: the configuration is renewed once less than half
    /// of its life remains and is restored to the full maximum, and nothing has
    /// to be configured at initialization. Instance storage expires as a single
    /// ledger entry, so one renewal covers every key written by the caller.
    fn bump_instance(env: &Env) {
        let max = env.storage().max_ttl();
        if max > 0 {
            env.storage().instance().extend_ttl(max / 2, max);
        }
    }
}