//! SolNet: Hardware Slash Contract
//! Description: Flags fraudulent or bypassed meters and slashes their associated
//! security deposits on-chain.
//!
//! Meter operators post a collateral deposit before joining the mesh. When the
//! cryptographic oracle proves a meter spoofed generation numbers or bypassed
//! the micro-billing router, the administrator slashes the full deposit: a
//! configured fraction in basis points is permanently burned and the remainder
//! is redistributed to the community compensation pool. Each offence may only
//! be punished once, and an offence under appeal is frozen until resolved.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    InvalidBurnBps = 3,
    InvalidAmount = 4,
    NoCollateral = 5,
    AlreadySlashed = 6,
    UnderAppeal = 7,
    NoAppeal = 8,
}

/// Class of hardware misbehaviour proven against a meter operator.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Offence {
    /// Meter reported generation output it did not physically produce.
    SpoofedGeneration,
    /// Meter routed energy around the audited micro-billing router.
    RouterBypass,
}

/// Outcome of a successful slash.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlashResult {
    /// Full amount seized from the operator's deposit.
    pub seized: i128,
    /// Portion permanently removed from supply.
    pub burned: i128,
    /// Portion moved into the community compensation pool.
    pub redistributed: i128,
    /// Offence the seizure was issued for.
    pub offence: Offence,
}

#[contract]
pub struct HardwareSlash;

#[contractimpl]
impl HardwareSlash {
    /// Initializes the hardware slash contract state configuration. `burn_bps`
    /// is the share of every seizure, in basis points, that is permanently
    /// destroyed; the remainder is redistributed to the compensation pool.
    pub fn init(env: Env, admin: Address, burn_bps: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        if !(0..=10_000).contains(&burn_bps) {
            return Err(Error::InvalidBurnBps);
        }
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("bbps"), &burn_bps);
        env.storage().instance().set(&symbol_short!("pool"), &0i128);
        env.storage().instance().set(&symbol_short!("burn"), &0i128);
        env.storage().instance().set(&symbol_short!("slz"), &0i128);
        env.storage().instance().set(&symbol_short!("dep"), &0i128);
        Ok(())
    }

    /// Posts or tops up a security deposit for a meter operator.
    pub fn deposit_collateral(env: Env, owner: Address, amount: i128) -> Result<(), Error> {
        owner.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let existing: i128 = Self::collateral(&env, &owner);
        env.storage()
            .persistent()
            .set(&(symbol_short!("col"), owner.clone()), &(existing + amount));
        let deposited: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("dep"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("dep"), &(deposited + amount));
        Ok(())
    }

    /// Seizes the operator's entire collateral deposit for a proven offence.
    /// Double slashing the same offence is impossible even if the operator has
    /// posted fresh collateral since the first seizure.
    pub fn slash(
        env: Env,
        admin: Address,
        owner: Address,
        offence: Offence,
    ) -> Result<SlashResult, Error> {
        Self::require_admin_auth(&env, &admin)?;
        if Self::appealed(&env, &owner, offence) {
            return Err(Error::UnderAppeal);
        }
        if Self::slashed(&env, &owner, offence) {
            return Err(Error::AlreadySlashed);
        }
        let seized = Self::collateral(&env, &owner);
        if seized <= 0 {
            return Err(Error::NoCollateral);
        }

        let burn_bps: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("bbps"))
            .unwrap_or(0);
        let burned = seized * burn_bps / 10_000;
        let redistributed = seized - burned;

        env.storage()
            .persistent()
            .set(&(symbol_short!("col"), owner.clone()), &0i128);
        env.storage()
            .persistent()
            .set(&(symbol_short!("sl"), owner.clone(), Self::code(offence)), &true);

        let pool: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("pool"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("pool"), &(pool + redistributed));
        let burned_total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("burn"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("burn"), &(burned_total + burned));
        let slashed_total: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("slz"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("slz"), &(slashed_total + seized));

        Ok(SlashResult {
            seized,
            burned,
            redistributed,
            offence,
        })
    }

    /// Freezes an offence while the operator's challenge is being adjudicated.
    pub fn appeal_window(
        env: Env,
        admin: Address,
        owner: Address,
        offence: Offence,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&(symbol_short!("ap"), owner, Self::code(offence)), &true);
        Ok(())
    }

    /// Lifts an appeal freeze so the offence can be adjudicated on the merits.
    pub fn resolve_appeal(
        env: Env,
        admin: Address,
        owner: Address,
        offence: Offence,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        let key = (symbol_short!("ap"), owner, Self::code(offence));
        if !env.storage().persistent().has(&key) {
            return Err(Error::NoAppeal);
        }
        env.storage().persistent().set(&key, &false);
        Ok(())
    }

    /// Security deposit currently backing an operator.
    pub fn collateral_of(env: Env, owner: Address) -> i128 {
        Self::collateral(&env, &owner)
    }

    /// Balance of the community compensation pool fed by redistributions.
    pub fn compensation_pool(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("pool"))
            .unwrap_or(0)
    }

    /// Cumulative amount destroyed by slashes.
    pub fn total_burned(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("burn"))
            .unwrap_or(0)
    }

    /// Cumulative collateral seized across all slashes.
    pub fn total_slashed(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("slz"))
            .unwrap_or(0)
    }

    /// Cumulative collateral ever posted by operators.
    pub fn total_deposited(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("dep"))
            .unwrap_or(0)
    }

    /// Burn share in basis points currently configured by the administrator.
    pub fn burn_bps(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("bbps"))
            .unwrap_or(0)
    }

    /// True once a given offence has been punished for this operator.
    pub fn was_slashed(env: Env, owner: Address, offence: Offence) -> bool {
        Self::slashed(&env, &owner, offence)
    }

    /// True while an offence is frozen pending appeal resolution.
    pub fn is_appealed(env: Env, owner: Address, offence: Offence) -> bool {
        Self::appealed(&env, &owner, offence)
    }

    fn require_admin_auth(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotAdmin)?;
        if &stored != admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();
        Ok(())
    }

    fn collateral(env: &Env, owner: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("col"), owner))
            .unwrap_or(0)
    }

    fn slashed(env: &Env, owner: &Address, offence: Offence) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("sl"), owner, Self::code(offence)))
            .unwrap_or(false)
    }

    fn appealed(env: &Env, owner: &Address, offence: Offence) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("ap"), owner, Self::code(offence)))
            .unwrap_or(false)
    }

    fn code(offence: Offence) -> u32 {
        match offence {
            Offence::SpoofedGeneration => 1,
            Offence::RouterBypass => 2,
        }
    }
}