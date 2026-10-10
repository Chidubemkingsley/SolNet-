use solnet_micro_billing::{Error, MicroBilling, MicroBillingClient};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

fn setup_with_price(price_per_mwh: i128) -> (Env, Address, MicroBillingClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(MicroBilling, ());
    let client = MicroBillingClient::new(&e, &id);
    client.init(&admin, &price_per_mwh);
    (e, admin, client)
}

fn setup() -> (Env, Address, MicroBillingClient<'static>) {
    setup_with_price(250)
}

#[test]
fn init_stores_admin_and_price() {
    let (e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.price(), 250);
    let account = Address::generate(&e);
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 0);
    assert!(!client.seen_nonce(&account, &0));
}

#[test]
fn init_rejects_second_initialization() {
    let (e, admin, client) = setup();
    let other = Address::generate(&e);
    assert_eq!(
        client.try_init(&other, &1_000),
        Err(Ok(Error::AlreadyInitialized))
    );
    assert_eq!(client.price(), 250);
    assert_eq!(client.admin(), admin);
}

#[test]
fn init_rejects_non_positive_price() {
    let e = Env::default();
    e.mock_all_auths();
    let id = e.register(MicroBilling, ());
    let client = MicroBillingClient::new(&e, &id);
    let admin = Address::generate(&e);

    assert_eq!(client.try_init(&admin, &0), Err(Ok(Error::InvalidPrice)));
    assert_eq!(client.try_init(&admin, &-7), Err(Ok(Error::InvalidPrice)));
    assert_eq!(client.try_price(), Err(Ok(Error::NotAdmin)));

    client.init(&admin, &1);
    assert_eq!(client.price(), 1);
}

#[test]
fn record_charges_scaled_price_and_returns_fee() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(client.record(&account, &7, &2_000, &1), 500);
    assert_eq!(client.unsettled(&account), 500);
    assert_eq!(client.reading_count(&account), 1);
    assert_eq!(client.bound_node(&account), 7);
    assert!(client.seen_nonce(&account, &1));
}

#[test]
fn record_accumulates_unsettled_balance() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    client.record(&account, &1, &1_000, &1);
    client.record(&account, &1, &1_500, &2);
    client.record(&account, &1, &250, &3);
    assert_eq!(client.unsettled(&account), 687);
    assert_eq!(client.reading_count(&account), 3);
}

#[test]
fn record_rounds_sub_billable_readings_down_to_zero() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(client.record(&account, &1, &3, &1), 0);
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 1);
    assert!(client.seen_nonce(&account, &1));
    assert_eq!(client.try_settle(&account), Err(Ok(Error::NothingToSettle)));
}

#[test]
fn record_rejects_zero_and_negative_milli_wh() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_record(&account, &1, &0, &1),
        Err(Ok(Error::InvalidReading))
    );
    assert_eq!(
        client.try_record(&account, &1, &-2_000, &1),
        Err(Ok(Error::InvalidReading))
    );
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 0);
    assert!(!client.seen_nonce(&account, &1));
}

#[test]
fn record_rejects_readings_that_would_overflow() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_record(&account, &1, &i128::MAX, &1),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 0);
    assert!(!client.seen_nonce(&account, &1));

    assert_eq!(
        client.record(&account, &1, &1_000_000_000_000, &1),
        250_000_000_000
    );
    assert_eq!(client.unsettled(&account), 250_000_000_000);
}

#[test]
fn record_isolates_balances_and_counts_per_account() {
    let (e, _admin, client) = setup();
    let first = Address::generate(&e);
    let second = Address::generate(&e);
    client.record(&first, &1, &2_000, &1);
    client.record(&first, &1, &2_000, &2);
    client.record(&second, &9, &4_000, &1);

    assert_eq!(client.unsettled(&first), 1_000);
    assert_eq!(client.reading_count(&first), 2);
    assert_eq!(client.bound_node(&first), 1);
    assert_eq!(client.unsettled(&second), 1_000);
    assert_eq!(client.reading_count(&second), 1);
    assert_eq!(client.bound_node(&second), 9);
    assert!(client.seen_nonce(&first, &1));
    assert!(!client.seen_nonce(&first, &3));
    assert!(client.seen_nonce(&second, &1));

    let untouched = vec![&e, Address::generate(&e)];
    for account in untouched.iter() {
        assert_eq!(client.unsettled(&account), 0);
        assert_eq!(client.reading_count(&account), 0);
        assert_eq!(
            client.try_bound_node(&account),
            Err(Ok(Error::InvalidReading))
        );
    }
}

#[test]
fn replayed_nonce_is_rejected_without_double_charge() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    let other = Address::generate(&e);
    client.record(&account, &1, &2_000, &1);

    assert_eq!(
        client.try_record(&account, &1, &2_000, &1),
        Err(Ok(Error::DuplicateNonce))
    );
    assert_eq!(client.unsettled(&account), 500);
    assert_eq!(client.reading_count(&account), 1);

    client.record(&other, &2, &1_000, &1);
    assert_eq!(client.unsettled(&other), 250);
    assert_eq!(client.unsettled(&account), 500);
}

#[test]
fn first_reading_binds_account_to_a_meter_node() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_bound_node(&account),
        Err(Ok(Error::InvalidReading))
    );
    client.record(&account, &42, &1_000, &1);
    assert_eq!(client.bound_node(&account), 42);
}

#[test]
fn readings_from_a_foreign_node_are_rejected() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    client.record(&account, &42, &1_000, &1);

    assert_eq!(
        client.try_record(&account, &43, &1_000, &2),
        Err(Ok(Error::NodeMismatch))
    );
    assert_eq!(client.unsettled(&account), 250);
    assert_eq!(client.reading_count(&account), 1);
    assert!(!client.seen_nonce(&account, &2));
    assert_eq!(client.bound_node(&account), 42);

    assert_eq!(client.record(&account, &42, &1_000, &2), 250);
    assert_eq!(client.unsettled(&account), 500);
}

#[test]
fn settle_closes_out_and_resets_unsettled_balance() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    client.record(&account, &1, &2_000, &1);
    client.record(&account, &1, &1_000, &2);

    assert_eq!(client.settle(&account), 750);
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 2);
    assert_eq!(client.bound_node(&account), 1);

    client.record(&account, &1, &1_000, &3);
    assert_eq!(client.unsettled(&account), 250);
    assert_eq!(client.settle(&account), 250);
    assert_eq!(client.reading_count(&account), 3);
    assert_eq!(client.bound_node(&account), 1);
}

#[test]
fn settle_rejects_account_with_nothing_unsettled() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(client.try_settle(&account), Err(Ok(Error::NothingToSettle)));

    client.record(&account, &1, &1, &1);
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.try_settle(&account), Err(Ok(Error::NothingToSettle)));

    client.record(&account, &1, &2_000, &2);
    assert_eq!(client.settle(&account), 500);
    assert_eq!(client.try_settle(&account), Err(Ok(Error::NothingToSettle)));
}

#[test]
fn record_requires_account_authorization() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);

    e.set_auths(&[]);
    assert!(client.try_record(&account, &1, &2_000, &1).is_err());

    e.mock_all_auths();
    assert_eq!(client.unsettled(&account), 0);
    assert_eq!(client.reading_count(&account), 0);
    assert!(!client.seen_nonce(&account, &1));
    assert_eq!(client.record(&account, &1, &2_000, &1), 500);
}

#[test]
fn ledger_is_none_before_wiring() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);

    assert_eq!(client.ledger(), None);
    client.record(&account, &1, &2_000, &1);
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
    let id = e.register(MicroBilling, ());
    let client = MicroBillingClient::new(&e, &id);

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
    let target = e.register(MicroBilling, ());

    assert_eq!(
        client.try_wire(&admin, &target),
        Err(Ok(Error::LedgerCallFailed))
    );
    assert_eq!(client.ledger(), None);
}

#[test]
fn a_failed_wire_leaves_the_router_unwired() {
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

    assert_eq!(client.record(&account, &1, &2_000, &1), 500);
    assert_eq!(client.unsettled(&account), 500);
    assert_eq!(client.reading_count(&account), 1);
    assert_eq!(client.bound_node(&account), 1);
    assert_eq!(client.settle(&account), 500);
}

#[test]
fn record_accrues_charges_while_no_ledger_is_wired() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    let untouched = vec![&e, Address::generate(&e)];

    client.record(&account, &1, &2_000, &1);
    client.record(&account, &1, &1_000, &2);

    assert_eq!(client.unsettled(&account), 750);
    assert_eq!(client.reading_count(&account), 2);
    for account in untouched.iter() {
        assert_eq!(client.unsettled(&account), 0);
        assert_eq!(client.reading_count(&account), 0);
        assert_eq!(
            client.try_bound_node(&account),
            Err(Ok(Error::InvalidReading))
        );
    }
}
