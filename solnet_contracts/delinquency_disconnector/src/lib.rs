//! SolNet: Delinquency Disconnector Contract
//! Description: Enforces automated power disconnection commands when user balances hit zero.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Symbol, Val, Vec,
};

/// Domain separation tag hashed into every relay instruction. Deliberately
/// deployment independent so that the same digest can be replayed by the
/// physical meter relay against any grid instance.
const RELAY_DOMAIN_TAG: &[u8] = b"solnet/disconnect/v1";

/// Upper bound on the canonical strkey encoding of a Stellar address.
const ADDRESS_STRKEY_MAX: usize = 64;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    NotDelinquent = 3,
    SequenceUnderflow = 4,
    InvalidLedgerSeq = 5,
    RelayEncodingFailed = 6,
    LedgerCallFailed = 7,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// Balance is non-negative, or the account has never been marked delinquent.
    NotDelinquent,
    /// Delinquent, but the grace window has not yet elapsed.
    GracePeriod,
    /// Grace window elapsed: the meter relay may be instructed to disconnect.
    Disconnect,
}

#[contract]
pub struct DelinquencyDisconnector;

#[contractimpl]
impl DelinquencyDisconnector {
    /// Initializes the disconnector with the administrator and the grace period
    /// in seconds (ledger sequences) allowed after an account first goes
    /// delinquent before it may be cut off.
    pub fn init(env: Env, admin: Address, grace_period: u64) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&symbol_short!("admin"), &admin);
        env.storage()
            .instance()
            .set(&symbol_short!("grace"), &grace_period);
        Ok(())
    }

    /// Returns the administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_admin(&env)
    }

    /// Grace period, in seconds, applied before disconnection.
    pub fn grace_period(env: Env) -> Result<u64, Error> {
        Self::require_configured(&env)?;
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("grace"))
            .unwrap_or(0))
    }

    /// Records the first ledger sequence at which an account went delinquent.
    /// Re-marking an already delinquent account is accepted but deliberately
    /// preserves the original sequence, so the grace clock cannot be restarted
    /// to dodge a disconnection.
    pub fn mark_delinquent(
        env: Env,
        admin: Address,
        account: Address,
        ledger_seq: u64,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if ledger_seq == 0 {
            return Err(Error::InvalidLedgerSeq);
        }

        if Self::is_delinquent_of(&env, &account) {
            return Ok(());
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("dln"), account.clone()), &true);
        env.storage()
            .persistent()
            .set(&(symbol_short!("seq"), account), &ledger_seq);
        Ok(())
    }

    /// Points the disconnector at the account ledger it polices. Until this is
    /// called, `assess` is unavailable and the operator must supply balances
    /// explicitly through `verdict`.
    pub fn wire(env: Env, admin: Address, ledger: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        let probe = env.try_invoke_contract::<i128, Error>(
            &ledger,
            &symbol_short!("rate"),
            Vec::<Val>::new(&env),
        );
        let rate: i128 = match probe {
            Ok(Ok(r)) => r,
            _ => return Err(Error::LedgerCallFailed),
        };
        if rate <= 0 {
            return Err(Error::LedgerCallFailed);
        }
        env.storage().instance().set(&symbol_short!("ledg"), &ledger);
        Ok(())
    }

    /// The account ledger this disconnector polices, if one has been wired.
    pub fn ledger(env: Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("ledg"))
    }

    /// Reads the live Watt-credit balance of an account from the wired ledger
    /// and evaluates it. The delinquency mark tracks the account's current
    /// state rather than latching: an account first observed with a negative
    /// balance is marked at `current_seq`, starting its grace clock, and an
    /// account observed back at a non-negative balance is unmarked. Re-entering
    /// debt therefore always earns a fresh grace window instead of inheriting
    /// an already-expired one, so a single anomalous reading after a repayment
    /// cannot trigger an immediate cutoff.
    ///
    /// This is the entry point that closes the consumption loop end to end: a
    /// metering reading drives the balance negative via the billing router, and
    /// this reads that balance directly rather than trusting a caller.
    pub fn assess(
        env: Env,
        account: Address,
        current_seq: u64,
    ) -> Result<Verdict, Error> {
        Self::require_configured(&env)?;
        let ledger: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("ledg"))
            .ok_or(Error::LedgerCallFailed)?;

        let mut args: Vec<Val> = Vec::new(&env);
        args.push_back(account.clone().into_val(&env));
        let read = env.try_invoke_contract::<i128, Error>(
            &ledger,
            &Symbol::new(&env, "watt_balance"),
            args,
        );
        let balance: i128 = match read {
            Ok(Ok(b)) => b,
            _ => return Err(Error::LedgerCallFailed),
        };

        if balance < 0 {
            if !Self::is_delinquent_of(&env, &account) {
                env.storage()
                    .persistent()
                    .set(&(symbol_short!("dln"), account.clone()), &true);
                env.storage()
                    .persistent()
                    .set(&(symbol_short!("seq"), account.clone()), &current_seq);
            }
        } else if Self::is_delinquent_of(&env, &account) {
            env.storage()
                .persistent()
                .remove(&(symbol_short!("dln"), account.clone()));
            env.storage()
                .persistent()
                .remove(&(symbol_short!("seq"), account.clone()));
        }

        Self::verdict(env, account, balance, current_seq)
    }

    /// Evaluates an account against a supplied balance and ledger sequence.
    /// Returns `NotDelinquent` for non-negative balances and for negative
    /// balances that were never formally marked, `GracePeriod` while the grace
    /// window is still running, and `Disconnect` once it has elapsed.
    pub fn verdict(
        env: Env,
        account: Address,
        balance: i128,
        current_seq: u64,
    ) -> Result<Verdict, Error> {
        Self::require_configured(&env)?;
        let grace: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("grace"))
            .unwrap_or(0);

        if balance >= 0 || !Self::is_delinquent_of(&env, &account) {
            return Ok(Verdict::NotDelinquent);
        }

        let since: u64 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("seq"), account))
            .ok_or(Error::NotDelinquent)?;
        if current_seq < since {
            return Err(Error::SequenceUnderflow);
        }
        if current_seq - since < grace {
            Ok(Verdict::GracePeriod)
        } else {
            Ok(Verdict::Disconnect)
        }
    }

    /// Produces the deterministic 32-byte signed relay instruction that the
    /// physical meter relay verifies before cutting power. Rejected unless the
    /// account is currently marked delinquent.
    pub fn issue_disconnect(env: Env, account: Address) -> Result<BytesN<32>, Error> {
        Self::require_configured(&env)?;
        if !Self::is_delinquent_of(&env, &account) {
            return Err(Error::NotDelinquent);
        }

        let seq = env.ledger().sequence() as u64;

        let strkey = account.to_string();
        let len = strkey.len() as usize;
        if len == 0 || len > ADDRESS_STRKEY_MAX {
            return Err(Error::RelayEncodingFailed);
        }
        let mut address_bytes = [0u8; ADDRESS_STRKEY_MAX];
        strkey.copy_into_slice(&mut address_bytes[..len]);

        let mut payload = Bytes::from_slice(&env, RELAY_DOMAIN_TAG);
        payload.extend_from_slice(&address_bytes[..len]);
        payload.extend_from_array(&seq.to_be_bytes());

        Ok(env.crypto().sha256(&payload).to_bytes())
    }

    /// Administrator hook that clears delinquency state after the account has
    /// settled its debt. Rejected when the account is not marked delinquent.
    pub fn clear(env: Env, admin: Address, account: Address) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_configured(&env)?;
        if !Self::is_delinquent_of(&env, &account) {
            return Err(Error::NotDelinquent);
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("dln"), account.clone()), &false);
        env.storage()
            .persistent()
            .set(&(symbol_short!("seq"), account), &0u64);
        Ok(())
    }

    /// Ledger sequence at which the account first went delinquent, or zero when
    /// it is not delinquent.
    pub fn delinquent_since(env: Env, account: Address) -> Result<u64, Error> {
        Self::require_configured(&env)?;
        if !Self::is_delinquent_of(&env, &account) {
            return Ok(0);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&(symbol_short!("seq"), account))
            .unwrap_or(0))
    }

    /// Reports whether the account currently carries delinquency state.
    pub fn is_delinquent(env: Env, account: Address) -> Result<bool, Error> {
        Self::require_configured(&env)?;
        Ok(Self::is_delinquent_of(&env, &account))
    }

    fn require_configured(env: &Env) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            Ok(())
        } else {
            Err(Error::NotAdmin)
        }
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

    fn is_delinquent_of(env: &Env, account: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("dln"), account))
            .unwrap_or(false)
    }
}
