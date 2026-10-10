use solnet_delinquency_disconnector::{
    DelinquencyDisconnector, DelinquencyDisconnectorClient, Error, Verdict,
};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{storage::Instance as _, storage::Persistent as _, Address as _, Ledger as _},
    vec, Address, Env, Symbol,
};

/// Stand-in for a wired account ledger, so the tests can drive `assess` down
/// both of its write branches without depending on another contract crate.
#[contract]
pub struct StubLedger;

#[contractimpl]
impl StubLedger {
    pub fn set_balance(env: Env, account: Address, balance: i128) {
        env.storage().instance().set(&account, &balance);
    }

    pub fn rate(_env: Env) -> i128 {
        1_000_000
    }

    pub fn watt_balance(env: Env, account: Address) -> i128 {
        env.storage().instance().get(&account).unwrap_or(0)
    }
}

fn setup_with_grace(grace_period: u64) -> (Env, Address, DelinquencyDisconnectorClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(DelinquencyDisconnector, ());
    let client = DelinquencyDisconnectorClient::new(&e, &id);
    client.init(&admin, &grace_period);
    (e, admin, client)
}

fn setup() -> (Env, Address, DelinquencyDisconnectorClient<'static>) {
    setup_with_grace(100)
}

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect contract storage.
fn setup_tracked() -> (Env, Address, Address, DelinquencyDisconnectorClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(DelinquencyDisconnector, ());
    let client = DelinquencyDisconnectorClient::new(&e, &id);
    client.init(&admin, &100);
    (e, admin, id, client)
}

fn network_max_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().max_ttl())
}

fn entry_ttl(e: &Env, id: &Address, key: &(Symbol, Address)) -> u32 {
    e.as_contract(id, || e.storage().persistent().get_ttl(key))
}

fn instance_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().instance().get_ttl())
}

#[test]
fn init_stores_admin_and_grace_period() {
    let (e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.grace_period(), 100);
    let account = Address::generate(&e);
    assert!(!client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 0);
}

#[test]
fn init_rejects_second_initialization() {
    let (e, admin, client) = setup();
    let other = Address::generate(&e);
    assert_eq!(
        client.try_init(&other, &5),
        Err(Ok(Error::AlreadyInitialized))
    );
    assert_eq!(client.grace_period(), 100);
    assert_eq!(client.admin(), admin);
}

#[test]
fn mark_delinquent_requires_admin_authority() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_mark_delinquent(&impostor, &account, &10),
        Err(Ok(Error::NotAdmin))
    );
    assert!(!client.is_delinquent(&account));

    client.mark_delinquent(&admin, &account, &10);
    assert!(client.is_delinquent(&account));

    assert_eq!(
        client.try_clear(&impostor, &account),
        Err(Ok(Error::NotAdmin))
    );
    assert!(client.is_delinquent(&account));
}

#[test]
fn mark_delinquent_records_first_ledger_sequence() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &500);
    assert!(client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 500);
}

#[test]
fn re_marking_preserves_the_original_grace_clock() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);
    client.mark_delinquent(&admin, &account, &900);

    assert_eq!(client.delinquent_since(&account), 100);
    assert_eq!(client.verdict(&account, &-500, &150), Verdict::GracePeriod);
    assert_eq!(client.verdict(&account, &-500, &200), Verdict::Disconnect);
}

#[test]
fn mark_delinquent_rejects_zero_ledger_sequence() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_mark_delinquent(&admin, &account, &0),
        Err(Ok(Error::InvalidLedgerSeq))
    );
    assert!(!client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 0);

    client.mark_delinquent(&admin, &account, &1);
    assert_eq!(client.delinquent_since(&account), 1);
}

#[test]
fn verdict_is_not_delinquent_for_non_negative_balance() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    let healthy = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    assert_eq!(client.verdict(&account, &0, &500), Verdict::NotDelinquent);
    assert_eq!(
        client.verdict(&account, &1_000_000, &500),
        Verdict::NotDelinquent
    );
    assert_eq!(
        client.verdict(&healthy, &1_000_000, &500),
        Verdict::NotDelinquent
    );
    assert!(client.is_delinquent(&account));
}

#[test]
fn verdict_is_not_delinquent_before_the_account_is_marked() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.verdict(&account, &-1_000, &10_000),
        Verdict::NotDelinquent
    );
    assert_eq!(
        client.try_issue_disconnect(&account),
        Err(Ok(Error::NotDelinquent))
    );
}

#[test]
fn verdict_reports_grace_period_before_window_elapses() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    assert_eq!(client.verdict(&account, &-1, &100), Verdict::GracePeriod);
    assert_eq!(
        client.verdict(&account, &-999_999, &150),
        Verdict::GracePeriod
    );
    assert_eq!(client.verdict(&account, &-1, &199), Verdict::GracePeriod);
    assert_eq!(client.verdict(&account, &5, &199), Verdict::NotDelinquent);
}

#[test]
fn verdict_boundary_is_exact_at_the_grace_period() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    assert_eq!(client.verdict(&account, &-1, &199), Verdict::GracePeriod);
    assert_eq!(client.verdict(&account, &-1, &200), Verdict::Disconnect);
    assert_eq!(
        client.verdict(&account, &-1, &1_000_000),
        Verdict::Disconnect
    );

    let (e2, admin2, strict) = setup_with_grace(0);
    let instant = Address::generate(&e2);
    strict.mark_delinquent(&admin2, &instant, &100);
    assert_eq!(strict.grace_period(), 0);
    assert_eq!(strict.verdict(&instant, &-1, &100), Verdict::Disconnect);
}

#[test]
fn verdict_rejects_sequence_going_backwards() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &500);

    assert_eq!(
        client.try_verdict(&account, &-1, &499),
        Err(Ok(Error::SequenceUnderflow))
    );
    assert_eq!(client.verdict(&account, &-1, &500), Verdict::GracePeriod);
    assert_eq!(client.verdict(&account, &0, &1), Verdict::NotDelinquent);
}

#[test]
fn issue_disconnect_rejects_healthy_accounts() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);

    assert_eq!(
        client.try_issue_disconnect(&account),
        Err(Ok(Error::NotDelinquent))
    );

    client.mark_delinquent(&admin, &account, &100);
    client.clear(&admin, &account);
    assert_eq!(
        client.try_issue_disconnect(&account),
        Err(Ok(Error::NotDelinquent))
    );
}

#[test]
fn issue_disconnect_is_deterministic_and_account_bound() {
    let (e, admin, client) = setup();
    let first = Address::generate(&e);
    let second = Address::generate(&e);
    client.mark_delinquent(&admin, &first, &100);
    client.mark_delinquent(&admin, &second, &100);

    let a = client.issue_disconnect(&first);
    let b = client.issue_disconnect(&first);
    assert_eq!(a, b);
    assert_ne!(a, client.issue_disconnect(&second));

    let others = vec![&e, Address::generate(&e), Address::generate(&e)];
    for account in others.iter() {
        client.mark_delinquent(&admin, &account, &100);
        assert!(a != client.issue_disconnect(&account));
    }
}

#[test]
fn issue_disconnect_tracks_the_current_ledger_sequence() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    e.ledger().set_sequence_number(64);
    let at_64 = client.issue_disconnect(&account);
    assert_eq!(at_64, client.issue_disconnect(&account));

    e.ledger().set_sequence_number(65);
    let at_65 = client.issue_disconnect(&account);
    assert_ne!(at_64, at_65);

    e.ledger().set_sequence_number(64);
    assert_eq!(client.issue_disconnect(&account), at_64);
}

#[test]
fn clear_resets_delinquency_state() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    let never = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    assert_eq!(client.verdict(&account, &-500, &1_000), Verdict::Disconnect);
    client.clear(&admin, &account);
    assert!(!client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 0);
    assert_eq!(
        client.verdict(&account, &-500, &1_000),
        Verdict::NotDelinquent
    );
    assert_eq!(
        client.try_clear(&admin, &account),
        Err(Ok(Error::NotDelinquent))
    );

    client.mark_delinquent(&admin, &account, &2_000);
    assert_eq!(client.delinquent_since(&account), 2_000);
    assert_eq!(
        client.try_clear(&admin, &never),
        Err(Ok(Error::NotDelinquent))
    );
    assert!(!client.is_delinquent(&never));
}

#[test]
fn ledger_is_none_before_wiring() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);

    assert_eq!(client.ledger(), None);
    client.mark_delinquent(&admin, &account, &100);
    assert_eq!(client.ledger(), None);
}

#[test]
fn wire_requires_admin_authority() {
    let (e, _admin, client) = setup();
    let target = Address::generate(&e);
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_wire(&impostor, &target),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.ledger(), None);
}

#[test]
fn wire_rejects_before_initialization() {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let target = Address::generate(&e);
    let id = e.register(DelinquencyDisconnector, ());
    let client = DelinquencyDisconnectorClient::new(&e, &id);

    assert_eq!(client.try_wire(&admin, &target), Err(Ok(Error::NotAdmin)));
    assert_eq!(client.ledger(), None);
}

#[test]
fn wire_rejects_a_target_that_is_not_an_account_ledger() {
    let (e, admin, client) = setup();
    let target = Address::generate(&e);

    assert_eq!(
        client.try_wire(&admin, &target),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert_eq!(client.ledger(), None);
}

#[test]
fn wire_rejects_a_target_contract_that_does_not_expose_a_rate() {
    let (e, admin, client) = setup();
    let target = e.register(DelinquencyDisconnector, ());

    assert_eq!(
        client.try_wire(&admin, &target),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert_eq!(client.ledger(), None);
}

#[test]
fn a_failed_wire_leaves_the_disconnector_unwired() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    let target = Address::generate(&e);

    for _ in 0..3 {
        assert_eq!(
            client.try_wire(&admin, &target),
            Err(Ok(Error::LedgerCallFailed))
        );
    }
    assert_eq!(client.ledger(), None);
    assert_eq!(
        client.try_assess(&account, &0),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert!(!client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 0);
}

#[test]
fn assess_requires_a_wired_ledger() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);

    assert_eq!(
        client.try_assess(&account, &0),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert_eq!(
        client.try_assess(&account, &1_000_000),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert!(!client.is_delinquent(&account));
    assert_eq!(client.delinquent_since(&account), 0);
    assert_eq!(
        client.try_issue_disconnect(&account),
        Err(Ok(Error::NotDelinquent))
    );
}

#[test]
fn assess_rejects_before_initialization() {
    let e = Env::default();
    e.mock_all_auths();
    let account = Address::generate(&e);
    let id = e.register(DelinquencyDisconnector, ());
    let client = DelinquencyDisconnectorClient::new(&e, &id);

    assert_eq!(client.try_assess(&account, &0), Err(Ok(Error::NotAdmin)));
    assert_eq!(client.ledger(), None);
}

#[test]
fn verdict_remains_available_before_wiring() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);

    assert_eq!(client.ledger(), None);
    assert_eq!(client.verdict(&account, &-500, &150), Verdict::GracePeriod);
    assert_eq!(client.verdict(&account, &-500, &200), Verdict::Disconnect);
    assert_eq!(client.issue_disconnect(&account).len(), 32);
}

#[test]
fn marking_delinquent_renews_the_lifetime_of_both_delinquency_entries() {
    let (e, admin, id, client) = setup_tracked();
    let account = Address::generate(&e);
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);

    client.mark_delinquent(&admin, &account, &100);
    let acc1 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("dln"), acc1)),
        max
    );
    let acc2 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("seq"), acc2)),
        max
    );
}

#[test]
fn clearing_renews_the_lifetime_of_both_delinquency_entries() {
    let (e, admin, id, client) = setup_tracked();
    let account = Address::generate(&e);
    client.mark_delinquent(&admin, &account, &100);
    let max = network_max_ttl(&e, &id);

    client.clear(&admin, &account);
    let acc1 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("dln"), acc1)),
        max
    );
    let acc2 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("seq"), acc2)),
        max
    );
}

#[test]
fn assess_renews_the_lifetime_of_the_entries_it_marks() {
    let (e, admin, id, client) = setup_tracked();
    let ledger_id = e.register(StubLedger, ());
    let ledger = StubLedgerClient::new(&e, &ledger_id);
    let account = Address::generate(&e);
    client.wire(&admin, &ledger_id);
    ledger.set_balance(&account, &-500);
    let max = network_max_ttl(&e, &id);

    // First assess: negative balance -> GracePeriod, marks delinquent
    assert_eq!(client.assess(&account, &1_000), Verdict::GracePeriod);
    let acc1 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("dln"), acc1)),
        max
    );
    let acc2 = account.clone();
    assert_eq!(
        entry_ttl(&e, &id, &(symbol_short!("seq"), acc2)),
        max
    );

    // Second assess with positive balance: should be NotDelinquent but mark persists
    ledger.set_balance(&account, &500);
    assert_eq!(client.assess(&account, &1_100), Verdict::NotDelinquent);
    // The delinquency mark persists until explicitly cleared by admin
    let acc1 = account.clone();
    assert!(e.as_contract(&id, || {
        e.storage()
            .persistent()
            .has(&(symbol_short!("dln"), acc1))
    }));
    let acc2 = account.clone();
    assert!(e.as_contract(&id, || {
        e.storage()
            .persistent()
            .has(&(symbol_short!("seq"), acc2))
    }));
    assert_eq!(client.assess(&account, &1_100), Verdict::NotDelinquent);
}

#[test]
fn assess_leaves_the_entries_of_an_untouched_account_unrenewed() {
    let (e, admin, id, client) = setup_tracked();
    let ledger_id = e.register(StubLedger, ());
    let ledger = StubLedgerClient::new(&e, &ledger_id);
    let marked = Address::generate(&e);
    let other = Address::generate(&e);
    client.wire(&admin, &ledger_id);
    ledger.set_balance(&marked, &-500);
    ledger.set_balance(&other, &-500);

    client.assess(&marked, &1_000);
    let renewed = entry_ttl(&e, &id, &(symbol_short!("dln"), marked.clone()));
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert!(!e.as_contract(&id, || {
        e.storage()
            .persistent()
            .has(&(symbol_short!("dln"), other.clone()))
    }));
}

#[test]
fn init_renews_the_lifetime_of_the_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);

    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert_eq!(client.admin(), admin);
    assert_eq!(client.grace_period(), 100);
}

#[test]
fn wiring_renews_the_lifetime_of_the_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let ledger_id = e.register(StubLedger, ());
    let max = network_max_ttl(&e, &id);

    client.wire(&admin, &ledger_id);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert_eq!(client.ledger(), Some(ledger_id));
}

#[test]
fn a_wiring_write_restores_a_decayed_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);
    assert_eq!(client.grace_period(), 100);

    let ledger_id = e.register(StubLedger, ());
    client.wire(&admin, &ledger_id);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}
