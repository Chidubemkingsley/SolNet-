//! SolNet: IoT Device Registrar Contract
//! Description: An on-chain whitelist mapping unique, factory-fused public keys
//! embedded in tamper-resistant physical smart meters onto their hardware
//! identity, with a generation counter that lets firmware and oracle logic
//! detect stale or revoked registrations.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    EmptyModel = 4,
    AlreadyRegistered = 5,
    DeviceNotFound = 6,
    SelfTransfer = 7,
    NoRevocation = 8,
}

/// Whitelisted hardware identity of a single physical smart meter.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    /// Manufacturer model string fused into the meter at the factory.
    pub model: String,
    /// Ledger sequence at which the current registration was recorded.
    pub registered_at: u64,
}

#[contract]
pub struct IotRegistrar;

#[contractimpl]
impl IotRegistrar {
    /// Initializes the registrar with the authority allowed to mutate the
    /// hardware whitelist.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("count"), &0u32);
        Ok(())
    }

    /// Returns the registrar administrator address.
    #[allow(clippy::needless_question_mark)]
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_init(&env)?;
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotInitialized)?)
    }

    /// Whitelists a factory-fused meter public key. The registration
    /// generation of the key is incremented on every successful registration.
    pub fn register_device(
        env: Env,
        public_key: BytesN<32>,
        model: String,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if model.is_empty() {
            return Err(Error::EmptyModel);
        }
        if Self::exists(&env, &public_key) {
            return Err(Error::AlreadyRegistered);
        }

        let device = Device {
            model,
            registered_at: u64::from(env.ledger().sequence()),
        };
        env.storage()
            .persistent()
            .set(&(symbol_short!("dev"), public_key.clone()), &device);

        let version = Self::bump_version(&env, &public_key);
        env.storage()
            .persistent()
            .set(&(symbol_short!("ver"), public_key), &version);

        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("count"), &(count + 1));
        Ok(())
    }

    /// Removes a key from the whitelist, retaining the reason for audit and
    /// preserving the registration generation so stale firmware is detectable.
    pub fn revoke(
        env: Env,
        admin: Address,
        public_key: BytesN<32>,
        reason: String,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if !Self::exists(&env, &public_key) {
            return Err(Error::DeviceNotFound);
        }

        env.storage()
            .persistent()
            .remove(&(symbol_short!("dev"), public_key.clone()));
        env.storage()
            .persistent()
            .set(&(symbol_short!("rsn"), public_key), &reason);

        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("count"), &count.saturating_sub(1));
        Ok(())
    }

    /// Reports whether a key currently holds a live whitelist registration.
    pub fn is_registered(env: Env, public_key: BytesN<32>) -> bool {
        Self::exists(&env, &public_key)
    }

    /// Returns the whitelisted device record for a key.
    pub fn device(env: Env, public_key: BytesN<32>) -> Result<Device, Error> {
        Self::require_init(&env)?;
        env.storage()
            .persistent()
            .get(&(symbol_short!("dev"), public_key))
            .ok_or(Error::DeviceNotFound)
    }

    /// Number of keys currently holding a live registration.
    pub fn registered_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0)
    }

    /// Registration generation of a key. Starts at 0 and increases on every
    /// registration, so a key that was revoked and re-registered reports a
    /// strictly higher generation than firmware minted against the old one.
    pub fn version(env: Env, public_key: BytesN<32>) -> u32 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("ver"), public_key))
            .unwrap_or(0)
    }

    /// Moves a live registration from one key to another, bumping the
    /// destination key's registration generation.
    pub fn transfer(
        env: Env,
        admin: Address,
        from_key: BytesN<32>,
        to_key: BytesN<32>,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if from_key == to_key {
            return Err(Error::SelfTransfer);
        }
        if !Self::exists(&env, &from_key) {
            return Err(Error::DeviceNotFound);
        }
        if Self::exists(&env, &to_key) {
            return Err(Error::AlreadyRegistered);
        }

        let device: Device = env
            .storage()
            .persistent()
            .get(&(symbol_short!("dev"), from_key.clone()))
            .ok_or(Error::DeviceNotFound)?;
        let moved = Device {
            model: device.model,
            registered_at: u64::from(env.ledger().sequence()),
        };

        env.storage()
            .persistent()
            .remove(&(symbol_short!("dev"), from_key));
        env.storage()
            .persistent()
            .set(&(symbol_short!("dev"), to_key.clone()), &moved);

        let version = Self::bump_version(&env, &to_key);
        env.storage()
            .persistent()
            .set(&(symbol_short!("ver"), to_key), &version);
        Ok(())
    }

    /// Reason recorded by the most recent revocation of a key.
    pub fn revocation_reason(env: Env, public_key: BytesN<32>) -> Result<String, Error> {
        Self::require_init(&env)?;
        env.storage()
            .persistent()
            .get(&(symbol_short!("rsn"), public_key))
            .ok_or(Error::NoRevocation)
    }

    fn exists(env: &Env, public_key: &BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&(symbol_short!("dev"), public_key.clone()))
    }

    fn bump_version(env: &Env, public_key: &BytesN<32>) -> u32 {
        let current: u32 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("ver"), public_key.clone()))
            .unwrap_or(0);
        current.saturating_add(1)
    }

    fn require_init(env: &Env) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            Ok(())
        } else {
            Err(Error::NotInitialized)
        }
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotInitialized)?;
        stored.require_auth();
        Ok(())
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