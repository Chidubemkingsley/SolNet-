//! SolNet: Cryptographic Oracle Contract
//! Description: Ingests and untangles batch-compiled offline telemetry packets
//! that physical meters batch-sign locally during a network blackout, verifies
//! their integrity against a locally recomputable digest, and settles each
//! batch exactly once.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    EmptyBatch = 4,
    BatchReplay = 5,
    DigestMismatch = 6,
    BatchNotFound = 7,
    AlreadySettled = 8,
    InvalidReading = 9,
    ArithmeticOverflow = 10,
}

/// Domain separation tag prefixed to every telemetry digest, so a digest
/// produced here can never collide with any other hash of the same entries.
const DOMAIN_TAG: &[u8] = b"SOLNET-TELEMETRY-BATCH-V1";

/// A single unsigned telemetry sample captured by a meter during an outage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelemetryEntry {
    /// Grid node the sample belongs to.
    pub node_id: u32,
    /// Energy reading in milli-watt-hours, to avoid any floating point.
    pub milli_wh: i128,
    /// Ledger-free timestamp the meter stamped at capture time.
    pub recorded_at: u64,
}

/// Settled-state summary of one accepted telemetry batch.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchRecord {
    /// Number of telemetry samples contained in the batch.
    pub entry_count: u32,
    /// Sum of every entry's reading, in milli-watt-hours.
    pub total_mwh: i128,
    /// Address that submitted the batch and authorized it on chain.
    pub submitter: Address,
    /// Whether the batch has already been settled by the grid operator.
    pub settled: bool,
}

#[contract]
pub struct CryptographicOracle;

#[contractimpl]
impl CryptographicOracle {
    /// Initializes the oracle with the operator allowed to settle batches.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("count"), &0u32);
        Self::bump_instance(&env);
        Ok(())
    }

    /// Returns the oracle administrator address.
    #[allow(clippy::needless_question_mark)]
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_init(&env)?;
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotInitialized)?)
    }

    /// Recomputes the canonical digest of a telemetry batch. A meter signs
    /// this value offline, so it must be derivable by the meter alone.
    pub fn compute_digest(env: Env, entries: Vec<TelemetryEntry>) -> BytesN<32> {
        Self::digest_of(&env, &entries)
    }

    /// Accepts a batch-compiled offline telemetry packet. The batch id may
    /// only ever be accepted once, which is what makes an outage-replayed
    /// packet impossible to double-count.
    pub fn submit_batch(
        env: Env,
        submitter: Address,
        batch_id: BytesN<32>,
        entries: Vec<TelemetryEntry>,
        digest: BytesN<32>,
    ) -> Result<(), Error> {
        Self::require_init(&env)?;
        submitter.require_auth();

        if entries.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if env
            .storage()
            .persistent()
            .has(&(symbol_short!("bat"), batch_id.clone()))
        {
            return Err(Error::BatchReplay);
        }
        if Self::digest_of(&env, &entries) != digest {
            return Err(Error::DigestMismatch);
        }

        let mut total: i128 = 0;
        for entry in entries.iter() {
            if entry.milli_wh < 0 {
                return Err(Error::InvalidReading);
            }
            total = total
                .checked_add(entry.milli_wh)
                .ok_or(Error::ArithmeticOverflow)?;
        }

        let record = BatchRecord {
            entry_count: entries.len(),
            total_mwh: total,
            submitter,
            settled: false,
        };
        env.storage()
            .persistent()
            .set(&(symbol_short!("bat"), batch_id.clone()), &record);
        Self::bump(&env, &(symbol_short!("bat"), batch_id));

        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("count"), &(count + 1));
        Self::bump_instance(&env);
        Ok(())
    }

    /// Marks an accepted batch as settled so it can never be settled twice.
    pub fn settle(env: Env, admin: Address, batch_id: BytesN<32>) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;

        let mut record: BatchRecord = env
            .storage()
            .persistent()
            .get(&(symbol_short!("bat"), batch_id.clone()))
            .ok_or(Error::BatchNotFound)?;
        if record.settled {
            return Err(Error::AlreadySettled);
        }

        record.settled = true;
        env.storage()
            .persistent()
            .set(&(symbol_short!("bat"), batch_id.clone()), &record);
        Self::bump(&env, &(symbol_short!("bat"), batch_id));
        Ok(())
    }

    /// Reports whether a previously submitted batch has been settled.
    pub fn is_settled(env: Env, batch_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("bat"), batch_id))
            .map(|record: BatchRecord| record.settled)
            .unwrap_or(false)
    }

    /// Returns the full record of an accepted batch.
    pub fn batch(env: Env, batch_id: BytesN<32>) -> Result<BatchRecord, Error> {
        Self::require_init(&env)?;
        env.storage()
            .persistent()
            .get(&(symbol_short!("bat"), batch_id))
            .ok_or(Error::BatchNotFound)
    }

    /// Total energy recorded by a batch, in milli-watt-hours.
    pub fn total_mwh(env: Env, batch_id: BytesN<32>) -> i128 {
        env.storage()
            .persistent()
            .get(&(symbol_short!("bat"), batch_id))
            .map(|record: BatchRecord| record.total_mwh)
            .unwrap_or(0)
    }

    /// Number of batches accepted so far.
    pub fn batch_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0)
    }

    /// Reports whether a batch id has ever been submitted.
    pub fn exists(env: Env, batch_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&(symbol_short!("bat"), batch_id))
    }

    fn digest_of(env: &Env, entries: &Vec<TelemetryEntry>) -> BytesN<32> {
        let mut buf = Bytes::new(env);
        for byte in DOMAIN_TAG {
            buf.push_back(*byte);
        }
        for entry in entries.iter() {
            for byte in entry.node_id.to_be_bytes() {
                buf.push_back(byte);
            }
            for byte in entry.milli_wh.to_be_bytes() {
                buf.push_back(byte);
            }
            for byte in entry.recorded_at.to_be_bytes() {
                buf.push_back(byte);
            }
        }
        env.crypto().sha256(&buf).to_bytes()
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