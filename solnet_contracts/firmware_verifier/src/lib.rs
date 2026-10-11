//! SolNet: Firmware Verifier Contract
//! Description: Ensures all active physical meters run the community-approved,
//! unmanipulated open-source firmware hash.
//!
//! A firmware build is identified by the SHA-256 digest of its signed release
//! image. The community approves digests, withdraws approval when a release is
//! found to be compromised, and maintains a monotonically rising minimum
//! version floor. Field meters present their running digest at verification
//! time and receive one of four verdicts.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    InvalidVersion = 3,
    ConflictingApproval = 4,
    NotApproved = 5,
    UnknownDevice = 6,
    InvalidCurrentVersion = 7,
}

/// Outcome of checking a meter against the community firmware registry.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// Digest is approved and at or above the community version floor.
    Approved,
    /// Digest was never submitted to the community for approval.
    Unapproved,
    /// Digest was approved and then withdrawn as compromised.
    Revoked,
    /// Digest is approved but its version sits below the community floor.
    Stale,
}

/// Full attestation result returned to the calling meter relay.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verification {
    /// Factory-fused public key of the meter that was checked.
    pub device_key: BytesN<32>,
    /// Digest the meter claimed to be running.
    pub firmware_hash: BytesN<32>,
    /// Classification of the claimed digest.
    pub verdict: Verdict,
    /// Version recorded for the digest, or zero when never approved.
    pub version: u32,
    /// Version floor that was applied when the verdict was produced.
    pub floor_version: u32,
}

#[contract]
pub struct FirmwareVerifier;

#[contractimpl]
impl FirmwareVerifier {
    /// Initializes the firmware verifier contract state configuration.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("lv"), &0u32);
        env.storage().instance().set(&symbol_short!("flr"), &0u32);
        Self::bump_instance(&env);
        Ok(())
    }

    /// Returns the administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotAdmin)
    }

    /// Marks a firmware build as community-approved and publishes the minimum
    /// version floor that applies to it. Re-approving the same digest with the
    /// same version is idempotent; re-approving it under a different version is
    /// rejected as a conflicting approval because a digest pins one release.
    pub fn approve_firmware(
        env: Env,
        admin: Address,
        firmware_hash: BytesN<32>,
        version: u32,
        min_version: u32,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if version == 0 {
            return Err(Error::InvalidVersion);
        }
        if min_version > version {
            return Err(Error::InvalidVersion);
        }

        if Self::approved_flag(&env, &firmware_hash) {
            let recorded: u32 = env
                .storage()
                .persistent()
                .get(&(symbol_short!("ver"), firmware_hash.clone()))
                .unwrap_or(0);
            if recorded != version {
                return Err(Error::ConflictingApproval);
            }
            return Ok(());
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("apr"), firmware_hash.clone()), &true);
        env.storage()
            .persistent()
            .set(&(symbol_short!("rvk"), firmware_hash.clone()), &false);
        env.storage()
            .persistent()
            .set(&(symbol_short!("ver"), firmware_hash.clone()), &version);
        env.storage()
            .persistent()
            .set(&(symbol_short!("mnv"), firmware_hash.clone()), &min_version);
        env.storage()
            .persistent()
            .set(&(symbol_short!("sc"), firmware_hash.clone()), &0u32);
        Self::bump(&env, &(symbol_short!("apr"), firmware_hash.clone()));
        Self::bump(&env, &(symbol_short!("rvk"), firmware_hash.clone()));
        Self::bump(&env, &(symbol_short!("ver"), firmware_hash.clone()));
        Self::bump(&env, &(symbol_short!("mnv"), firmware_hash.clone()));
        Self::bump(&env, &(symbol_short!("sc"), firmware_hash.clone()));

        let latest: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("lv"))
            .unwrap_or(0);
        if version > latest {
            env.storage().instance().set(&symbol_short!("lv"), &version);
        }

        let floor: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("flr"))
            .unwrap_or(0);
        if min_version > floor {
            env.storage()
                .instance()
                .set(&symbol_short!("flr"), &min_version);
        }
        Self::bump_instance(&env);
        Ok(())
    }

    /// Withdraws community approval from a firmware build, for example after the
    /// release is discovered to be compromised. The build's recorded version and
    /// minimum floor are preserved so the revoke can be audited.
    pub fn revoke_firmware(
        env: Env,
        admin: Address,
        firmware_hash: BytesN<32>,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if !Self::approved_flag(&env, &firmware_hash) {
            return Err(Error::NotApproved);
        }
        env.storage()
            .persistent()
            .set(&(symbol_short!("apr"), firmware_hash.clone()), &false);
        env.storage()
            .persistent()
            .set(&(symbol_short!("rvk"), firmware_hash.clone()), &true);
        Self::bump(&env, &(symbol_short!("apr"), firmware_hash.clone()));
        Self::bump(&env, &(symbol_short!("rvk"), firmware_hash.clone()));
        Ok(())
    }

    /// Core check run by the meter relay before a field device is allowed to
    /// participate in settlement. Returns the verdict for the claimed digest
    /// together with the version and floor that produced it.
    pub fn verify_device(
        env: Env,
        device_key: BytesN<32>,
        firmware_hash: BytesN<32>,
    ) -> Result<Verification, Error> {
        let floor = Self::effective_floor(&env, &firmware_hash);
        let verdict = if !env
            .storage()
            .persistent()
            .has(&(symbol_short!("apr"), firmware_hash.clone()))
            && !env
                .storage()
                .persistent()
                .has(&(symbol_short!("rvk"), firmware_hash.clone()))
        {
            Verdict::Unapproved
        } else if !Self::approved_flag(&env, &firmware_hash) {
            Verdict::Revoked
        } else {
            let version: u32 = env
                .storage()
                .persistent()
                .get(&(symbol_short!("ver"), firmware_hash.clone()))
                .unwrap_or(0);
            if version < floor {
                Verdict::Stale
            } else {
                Verdict::Approved
            }
        };

        let version: u32 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("ver"), firmware_hash.clone()))
            .unwrap_or(0);

        Ok(Verification {
            device_key,
            firmware_hash,
            verdict,
            version,
            floor_version: floor,
        })
    }

    /// Records what firmware a device claims to be running. If the claim sits
    /// below the community version floor the stale report counter for that
    /// digest is incremented, which lets operators spot repeat offenders. A
    /// SHA-256 attestation digest over the tuple is stored for audit.
    pub fn report_device(
        env: Env,
        admin: Address,
        device_key: BytesN<32>,
        firmware_hash: BytesN<32>,
        current_version: u32,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if current_version == 0 {
            return Err(Error::InvalidCurrentVersion);
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("dv"), device_key.clone()), &current_version);
        Self::bump(&env, &(symbol_short!("dv"), device_key.clone()));

        let mut payload = Bytes::new(&env);
        payload.append(&Bytes::from(device_key.clone()));
        payload.append(&Bytes::from(firmware_hash.clone()));
        payload.append(&Bytes::from_array(&env, &current_version.to_be_bytes()));
        let attestation = env.crypto().sha256(&payload).to_bytes();
        env.storage()
            .persistent()
            .set(&(symbol_short!("dig"), device_key.clone()), &attestation);
        Self::bump(&env, &(symbol_short!("dig"), device_key));

        if Self::approved_flag(&env, &firmware_hash) {
            let version: u32 = env
                .storage()
                .persistent()
                .get(&(symbol_short!("ver"), firmware_hash.clone()))
                .unwrap_or(0);
            if version < Self::effective_floor(&env, &firmware_hash)
                || current_version < Self::effective_floor(&env, &firmware_hash)
            {
                let stale: u32 = env
                    .storage()
                    .persistent()
                    .get(&(symbol_short!("sc"), firmware_hash.clone()))
                    .unwrap_or(0);
                env.storage()
                    .persistent()
                    .set(&(symbol_short!("sc"), firmware_hash.clone()), &(stale + 1));
                Self::bump(&env, &(symbol_short!("sc"), firmware_hash));
            }
        }
        Ok(())
    }

    /// Returns true only while a build holds active, unrevoked approval.
    pub fn is_approved(env: Env, firmware_hash: BytesN<32>) -> bool {
        Self::approved_flag(&env, &firmware_hash)
    }

    /// Returns the version recorded against an approved build.
    pub fn approved_version(env: Env, firmware_hash: BytesN<32>) -> Result<u32, Error> {
        if !Self::approved_flag(&env, &firmware_hash) {
            return Err(Error::NotApproved);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&(symbol_short!("ver"), firmware_hash))
            .unwrap_or(0))
    }

    /// Highest firmware version the community has ever approved.
    pub fn latest_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("lv"))
            .unwrap_or(0)
    }

    /// Community-wide minimum version floor, raised monotonically as newer
    /// builds are approved and never lowered by a revoke.
    pub fn floor_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("flr"))
            .unwrap_or(0)
    }

    /// Number of device reports seen for a build that sit below the floor.
    pub fn stale_report_count(env: Env, firmware_hash: BytesN<32>) -> u32 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("sc"), firmware_hash))
            .unwrap_or(0)
    }

    /// Version last reported by a device, or zero when never reported.
    pub fn device_version(env: Env, device_key: BytesN<32>) -> Result<u32, Error> {
        env.storage()
            .persistent()
            .get(&(symbol_short!("dv"), device_key))
            .ok_or(Error::UnknownDevice)
    }

    /// Attestation digest produced by the most recent device report.
    pub fn report_digest(env: Env, device_key: BytesN<32>) -> Result<BytesN<32>, Error> {
        env.storage()
            .persistent()
            .get(&(symbol_short!("dig"), device_key))
            .ok_or(Error::UnknownDevice)
    }

    /// Verdict classifier for a digest without echoing the requesting device.
    pub fn verdict_of(env: Env, firmware_hash: BytesN<32>) -> Verdict {
        match Self::verify_device(env, firmware_hash.clone(), firmware_hash) {
            Ok(v) => v.verdict,
            Err(_) => Verdict::Unapproved,
        }
    }

    fn approved_flag(env: &Env, firmware_hash: &BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("apr"), firmware_hash))
            .unwrap_or(false)
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

    fn effective_floor(env: &Env, firmware_hash: &BytesN<32>) -> u32 {
        let global: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("flr"))
            .unwrap_or(0);
        let local: u32 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("mnv"), firmware_hash))
            .unwrap_or(0);
        if local > global {
            local
        } else {
            global
        }
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