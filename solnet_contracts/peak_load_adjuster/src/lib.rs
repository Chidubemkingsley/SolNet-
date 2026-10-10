//! SolNet: Peak-Load Pricing Adjuster Contract
//! Description: Dynamically re-prices every milli-watt-hour shipped across the
//! microgrid from the live battery state-of-charge, so that a depleted community
//! battery bank is drained more slowly and its cycle life is protected.
//!
//! ## The scarcity curve
//!
//! State of charge is expressed in basis points, `soc_bps`, over `0..=10000`.
//! The scarcity multiplier implied by that reading is
//!
//! ```text
//! multiplier_bps = 10000 + (10000 - soc_bps) * SCARCITY_SCALAR_BPS / 10000
//! ```
//!
//! with `SCARCITY_SCALAR_BPS = 5000`. A full battery therefore prices at exactly
//! `1.00x` the base price, a half-charged battery at `1.25x`, and an empty battery
//! at `1.50x`. The raw price is then
//!
//! ```text
//! raw_price = base_price * multiplier_bps / 10000
//! price     = clamp(raw_price, floor_price, max_price)
//! ```
//!
//! `floor_price` is the minimum tariff the grid will ever charge (it may be set
//! above `base_price` to guarantee a floor revenue floor for battery upkeep), and
//! `max_price` is the hard ceiling protecting consumers from scarcity spikes. The
//! ceiling defaults to the natural top of the curve, `base_price * 3 / 2`, and the
//! administrator may reconfigure it later with `set_max_price` as long as it never
//! drops below `floor_price`.
//!
//! All arithmetic is integer-only. The raw price is clamped *before* any
//! multiplication so an extreme `base_price` can never overflow.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, symbol_short, Address, Env};

/// Fixed-point denominator: 10000 basis points == 100%.
const BPS_DENOM: i128 = 10_000;

/// Extra scarcity multiplier, in basis points, contributed at 0% state of charge.
const SCARCITY_SCALAR_BPS: i128 = 5_000;

/// Top of the scarcity curve: 10000 + SCARCITY_SCALAR_BPS == 15000 bps == 1.50x.
const MAX_MULTIPLIER_BPS: i128 = 10_000 + SCARCITY_SCALAR_BPS;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    InvalidBasePrice = 4,
    InvalidFloorPrice = 5,
    InvalidMaxPrice = 6,
    InvalidSoc = 7,
    ArithmeticOverflow = 8,
}

#[contract]
pub struct PeakLoadAdjuster;

#[contractimpl]
impl PeakLoadAdjuster {
    /// Initializes the peak-load adjuster with the base price per milli-watt-hour
    /// and the minimum price the grid will ever charge. The price ceiling starts at
    /// the natural top of the scarcity curve, `base_price * 3 / 2`, raised to
    /// `floor_price` when the floor sits above it.
    pub fn init(env: Env, admin: Address, base_price: i128, floor_price: i128) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        if base_price <= 0 {
            return Err(Error::InvalidBasePrice);
        }
        if floor_price <= 0 {
            return Err(Error::InvalidFloorPrice);
        }
        admin.require_auth();

        let curve_ceiling = Self::scaled_ceiling(base_price);
        let max_price = if curve_ceiling < floor_price {
            floor_price
        } else {
            curve_ceiling
        };

        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("base"), &base_price);
        env.storage().instance().set(&symbol_short!("floor"), &floor_price);
        env.storage().instance().set(&symbol_short!("maxpx"), &max_price);
        env.storage().instance().set(&symbol_short!("soc"), &BPS_DENOM);
        Ok(())
    }

    /// Returns the administrator address recorded at initialization.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("admin")).unwrap())
    }

    /// Returns the configured base price per milli-watt-hour.
    pub fn base_price(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("base")).unwrap())
    }

    /// Returns the configured minimum price per milli-watt-hour.
    pub fn floor_price(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("floor")).unwrap())
    }

    /// Returns the configured maximum price per milli-watt-hour.
    pub fn max_price(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("maxpx")).unwrap())
    }

    /// Publishes a fresh battery state-of-charge reading in basis points over
    /// `0..=10000`. Only the administrator may move the reading.
    pub fn update_soc(env: Env, admin: Address, soc_bps: i128) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if soc_bps < 0 || soc_bps > BPS_DENOM {
            return Err(Error::InvalidSoc);
        }
        env.storage().instance().set(&symbol_short!("soc"), &soc_bps);
        Ok(())
    }

    /// Returns the latest battery state of charge in basis points.
    pub fn soc(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("soc")).unwrap())
    }

    /// Returns the scarcity multiplier, in basis points, implied by the current
    /// state of charge, independent of the configured prices.
    pub fn price_multiplier_bps(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        let soc: i128 = env.storage().instance().get(&symbol_short!("soc")).unwrap();
        Ok(Self::multiplier_for(soc))
    }

    /// Returns the effective price per milli-watt-hour for the current battery
    /// state of charge, clamped to `[floor_price, max_price]`.
    pub fn current_price(env: Env) -> Result<i128, Error> {
        Self::require_initialized(&env)?;
        let base: i128 = env.storage().instance().get(&symbol_short!("base")).unwrap();
        let floor: i128 = env.storage().instance().get(&symbol_short!("floor")).unwrap();
        let ceiling: i128 = env.storage().instance().get(&symbol_short!("maxpx")).unwrap();
        let soc: i128 = env.storage().instance().get(&symbol_short!("soc")).unwrap();
        let multiplier = Self::multiplier_for(soc);

        let safe_base = i128::MAX / multiplier;
        let raw = if base > safe_base {
            ceiling
        } else {
            base.checked_mul(multiplier)
                .ok_or(Error::ArithmeticOverflow)?
                / BPS_DENOM
        };

        let bounded = if raw > ceiling { ceiling } else { raw };
        Ok(if bounded < floor { floor } else { bounded })
    }

    /// Reconfigures the hard price ceiling. The ceiling may never fall below
    /// `floor_price`, otherwise the two clamps could contradict each other.
    pub fn set_max_price(env: Env, admin: Address, max_price: i128) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if max_price <= 0 {
            return Err(Error::InvalidMaxPrice);
        }
        let floor: i128 = env.storage().instance().get(&symbol_short!("floor")).unwrap();
        if max_price < floor {
            return Err(Error::InvalidMaxPrice);
        }
        env.storage().instance().set(&symbol_short!("maxpx"), &max_price);
        Ok(())
    }

    fn multiplier_for(soc_bps: i128) -> i128 {
        let deficit = BPS_DENOM - soc_bps;
        BPS_DENOM + deficit * SCARCITY_SCALAR_BPS / BPS_DENOM
    }

    fn scaled_ceiling(base_price: i128) -> i128 {
        let safe_base = i128::MAX / MAX_MULTIPLIER_BPS;
        if base_price > safe_base {
            i128::MAX
        } else {
            base_price * MAX_MULTIPLIER_BPS / BPS_DENOM
        }
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