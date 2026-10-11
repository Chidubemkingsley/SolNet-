use solnet_account_ledger::{AccountLedger, AccountLedgerClient, Error};
use soroban_sdk::{
    symbol_short,
    testutils::{storage::Instance as _, storage::Persistent as _, Address as _, Ledger as _},
    vec, Address, Env, Symbol,
};

const SCALE: i128 = 1_000_000;

fn setup_with_rate(rate: i128) -> (Env, Address, AccountLedgerClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(AccountLedger, ());
    let client = AccountLedgerClient::new(&e, &id);
    client.init(&admin, &rate);
    (e, admin, client)
}

fn setup() -> (Env, Address, AccountLedgerClient<'static>) {
    setup_with_rate(SCALE)
}

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect contract storage.
fn setup_tracked() -> (Env, Address, Address, AccountLedgerClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(AccountLedger, ());
    let client = AccountLedgerClient::new(&e, &id);
    client.init(&admin, &SCALE);
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
fn init_stores_admin_and_rate() {
    let (e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.rate(), SCALE);
    let account = Address::generate(&e);
    assert_eq!(client.stable_balance(&account), 0);
    assert_eq!(client.watt_balance(&account), 0);
}

#[test]
fn init_rejects_second_initialization() {
    let (e, admin, client) = setup();
    let other = Address::generate(&e);
    assert_eq!(
        client.try_init(&other, &2_000_000),
        Err(Ok(Error::AlreadyInitialized))
    );
    assert_eq!(client.rate(), SCALE);
    assert_eq!(client.admin(), admin);
}

#[test]
fn init_rejects_non_positive_rate() {
    let e = Env::default();
    e.mock_all_auths();
    let id = e.register(AccountLedger, ());
    let client = AccountLedgerClient::new(&e, &id);
    let admin = Address::generate(&e);

    assert_eq!(client.try_init(&admin, &0), Err(Ok(Error::InvalidRate)));
    assert_eq!(client.try_init(&admin, &-1), Err(Ok(Error::InvalidRate)));
    assert_eq!(client.try_admin(), Err(Ok(Error::NotAdmin)));

    client.init(&admin, &1);
    assert_eq!(client.rate(), 1);
}

#[test]
fn deposit_credits_stable_and_watt_balances() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(client.deposit(&account, &250), 250);
    assert_eq!(client.stable_balance(&account), 250);
    assert_eq!(client.watt_balance(&account), 250);
}

#[test]
fn deposit_uses_fixed_point_rate_and_rounds_down() {
    let (e, _admin, client) = setup_with_rate(2_500_000);
    let account = Address::generate(&e);
    assert_eq!(client.deposit(&account, &3), 7);
    assert_eq!(client.watt_balance(&account), 7);
    assert_eq!(client.stable_balance(&account), 3);
}

#[test]
fn deposit_rejects_zero_and_negative_amounts() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_deposit(&account, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_deposit(&account, &-100),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.stable_balance(&account), 0);
    assert_eq!(client.watt_balance(&account), 0);
}

#[test]
fn deposit_is_cumulative_and_isolated_per_account() {
    let (e, _admin, client) = setup();
    let first = Address::generate(&e);
    let second = Address::generate(&e);
    client.deposit(&first, &100);
    client.deposit(&first, &50);
    client.deposit(&second, &10);

    assert_eq!(client.stable_balance(&first), 150);
    assert_eq!(client.watt_balance(&first), 150);
    assert_eq!(client.stable_balance(&second), 10);
    assert_eq!(client.watt_balance(&second), 10);

    let untouched = vec![&e, Address::generate(&e), Address::generate(&e)];
    for account in untouched.iter() {
        assert_eq!(client.stable_balance(&account), 0);
        assert_eq!(client.watt_balance(&account), 0);
    }
}

#[test]
fn withdraw_burns_credits_and_returns_stable_amount() {
    let (e, _admin, client) = setup_with_rate(500_000);
    let account = Address::generate(&e);
    client.deposit(&account, &20);
    assert_eq!(client.watt_balance(&account), 10);
    assert_eq!(client.stable_balance(&account), 20);

    assert_eq!(client.withdraw(&account, &4), 8);
    assert_eq!(client.watt_balance(&account), 6);
    assert_eq!(client.stable_balance(&account), 12);

    assert_eq!(client.withdraw(&account, &6), 12);
    assert_eq!(client.watt_balance(&account), 0);
    assert_eq!(client.stable_balance(&account), 0);

    assert_eq!(
        client.try_withdraw(&account, &1),
        Err(Ok(Error::InsufficientCredits))
    );
}

#[test]
fn withdraw_rejects_zero_and_negative_credits() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &100);
    assert_eq!(
        client.try_withdraw(&account, &0),
        Err(Ok(Error::InvalidCredits))
    );
    assert_eq!(
        client.try_withdraw(&account, &-25),
        Err(Ok(Error::InvalidCredits))
    );
    assert_eq!(client.watt_balance(&account), 100);
    assert_eq!(client.stable_balance(&account), 100);
}

#[test]
fn withdraw_rejects_overdrawn_watt_balance() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &10);
    assert_eq!(client.watt_balance(&account), 10);

    assert_eq!(
        client.try_withdraw(&account, &11),
        Err(Ok(Error::InsufficientCredits))
    );

    client.credit_watts(&admin, &account, &-100);
    assert_eq!(client.watt_balance(&account), -90);
    assert_eq!(
        client.try_withdraw(&account, &1),
        Err(Ok(Error::InsufficientCredits))
    );
    assert_eq!(client.stable_balance(&account), 10);
}

#[test]
fn withdraw_rejects_insufficient_stable_reserve() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &5);
    client.credit_watts(&admin, &account, &10);
    assert_eq!(client.watt_balance(&account), 15);
    assert_eq!(client.stable_balance(&account), 5);

    assert_eq!(
        client.try_withdraw(&account, &15),
        Err(Ok(Error::InsufficientFunds))
    );
    assert_eq!(client.withdraw(&account, &5), 5);
    assert_eq!(client.stable_balance(&account), 0);
    assert_eq!(client.watt_balance(&account), 10);
}

#[test]
fn withdraw_rejects_dust_below_payout_floor() {
    let (e, _admin, client) = setup_with_rate(2_000_000);
    let account = Address::generate(&e);
    client.deposit(&account, &1);
    assert_eq!(client.watt_balance(&account), 2);

    assert_eq!(
        client.try_withdraw(&account, &1),
        Err(Ok(Error::InvalidCredits))
    );
    assert_eq!(client.watt_balance(&account), 2);
    assert_eq!(client.stable_balance(&account), 1);

    assert_eq!(client.withdraw(&account, &2), 1);
    assert_eq!(client.watt_balance(&account), 0);
    assert_eq!(client.stable_balance(&account), 0);
}

#[test]
fn credit_watts_requires_admin_authority() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_credit_watts(&impostor, &account, &50),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.watt_balance(&account), 0);

    assert_eq!(client.credit_watts(&admin, &account, &50), 50);
    assert_eq!(client.watt_balance(&account), 50);
}

#[test]
fn credit_watts_can_drive_watt_balance_negative() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &100);
    assert_eq!(client.watt_balance(&account), 100);

    assert_eq!(client.credit_watts(&admin, &account, &-250), -150);
    assert_eq!(client.watt_balance(&account), -150);
    assert_eq!(client.stable_balance(&account), 100);
    assert_eq!(
        client.try_withdraw(&account, &1),
        Err(Ok(Error::InsufficientCredits))
    );

    assert_eq!(client.credit_watts(&admin, &account, &200), 50);
    assert_eq!(client.watt_balance(&account), 50);
}

#[test]
fn zero_credit_and_arithmetic_overflow_are_rejected() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);

    assert_eq!(
        client.try_credit_watts(&admin, &account, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.watt_balance(&account), 0);

    assert_eq!(
        client.try_deposit(&account, &i128::MAX),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.stable_balance(&account), 0);

    assert_eq!(client.credit_watts(&admin, &account, &i128::MIN), i128::MIN);
    assert_eq!(
        client.try_credit_watts(&admin, &account, &i128::MIN),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.watt_balance(&account), i128::MIN);
}

#[test]
fn authorize_router_grants_the_billing_capability() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    assert!(!client.is_router(&router));

    client.authorize_router(&admin, &router);
    assert!(client.is_router(&router));
}

#[test]
fn authorize_router_rejects_a_duplicate_grant() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let second = Address::generate(&e);
    client.authorize_router(&admin, &router);

    assert_eq!(
        client.try_authorize_router(&admin, &router),
        Err(Ok(Error::AlreadyARouter))
    );
    assert!(client.is_router(&router));
    assert!(!client.is_router(&second));

    client.authorize_router(&admin, &second);
    assert!(client.is_router(&router));
    assert!(client.is_router(&second));
}

#[test]
fn authorize_router_requires_admin_authority() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_authorize_router(&impostor, &router),
        Err(Ok(Error::NotAdmin))
    );
    assert!(!client.is_router(&router));

    client.authorize_router(&admin, &router);
    assert!(client.is_router(&router));
}

#[test]
fn is_router_is_scoped_to_the_authorized_addresses() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let others = vec![&e, Address::generate(&e), Address::generate(&e)];
    for other in others.iter() {
        assert!(!client.is_router(&other));
    }

    client.authorize_router(&admin, &router);
    assert!(client.is_router(&router));
    for other in others.iter() {
        assert!(!client.is_router(&other));
    }
}

#[test]
fn revoke_router_removes_the_billing_capability() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let other = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.authorize_router(&admin, &other);

    client.revoke_router(&admin, &router);
    assert!(!client.is_router(&router));
    assert!(client.is_router(&other));

    client.authorize_router(&admin, &router);
    assert!(client.is_router(&router));
}

#[test]
fn revoke_router_rejects_an_unknown_router() {
    let (e, admin, client) = setup();
    let stranger = Address::generate(&e);

    assert_eq!(
        client.try_revoke_router(&admin, &stranger),
        Err(Ok(Error::NotARouter))
    );

    let router = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.revoke_router(&admin, &router);
    assert_eq!(
        client.try_revoke_router(&admin, &router),
        Err(Ok(Error::NotARouter))
    );
}

#[test]
fn revoke_router_requires_admin_authority() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let impostor = Address::generate(&e);
    client.authorize_router(&admin, &router);

    assert_eq!(
        client.try_revoke_router(&impostor, &router),
        Err(Ok(Error::NotAdmin))
    );
    assert!(client.is_router(&router));

    client.revoke_router(&admin, &router);
    assert!(!client.is_router(&router));
}

#[test]
fn apply_consumption_debits_watt_credits_and_returns_the_new_balance() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.deposit(&account, &100);

    assert_eq!(client.apply_consumption(&router, &account, &30), 70);
    assert_eq!(client.watt_balance(&account), 70);
    assert_eq!(client.stable_balance(&account), 100);

    assert_eq!(client.apply_consumption(&router, &account, &20), 50);
    assert_eq!(client.watt_balance(&account), 50);
    assert_eq!(client.stable_balance(&account), 100);
}

#[test]
fn apply_consumption_rejects_a_non_router() {
    let (e, admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &100);
    let stranger = Address::generate(&e);

    assert_eq!(
        client.try_apply_consumption(&stranger, &account, &10),
        Err(Ok(Error::NotARouter))
    );
    assert_eq!(client.watt_balance(&account), 100);

    client.authorize_router(&admin, &stranger);
    assert_eq!(
        client.try_apply_consumption(&admin, &account, &10),
        Err(Ok(Error::NotARouter))
    );
    assert_eq!(client.watt_balance(&account), 100);
}

#[test]
fn apply_consumption_rejects_zero_and_negative_credits() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.deposit(&account, &100);

    assert_eq!(
        client.try_apply_consumption(&router, &account, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_apply_consumption(&router, &account, &-50),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.watt_balance(&account), 100);
    assert_eq!(client.stable_balance(&account), 100);
}

#[test]
fn apply_consumption_validates_the_amount_before_the_router_capability() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    let stranger = Address::generate(&e);

    assert_eq!(
        client.try_apply_consumption(&stranger, &account, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_apply_consumption(&stranger, &account, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.watt_balance(&account), 0);
}

#[test]
fn apply_consumption_can_drive_watt_balance_negative() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.deposit(&account, &5);

    assert_eq!(client.apply_consumption(&router, &account, &10), -5);
    assert_eq!(client.watt_balance(&account), -5);
    assert_eq!(client.stable_balance(&account), 5);
    assert_eq!(
        client.try_withdraw(&account, &1),
        Err(Ok(Error::InsufficientCredits))
    );

    assert_eq!(client.apply_consumption(&router, &account, &5), -10);
    assert_eq!(client.watt_balance(&account), -10);
}

#[test]
fn apply_consumption_guards_against_arithmetic_overflow() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.credit_watts(&admin, &account, &i128::MIN);
    assert_eq!(client.watt_balance(&account), i128::MIN);

    assert_eq!(
        client.try_apply_consumption(&router, &account, &1),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.watt_balance(&account), i128::MIN);

    assert_eq!(
        client.try_apply_consumption(&router, &account, &i128::MAX),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.watt_balance(&account), i128::MIN);

    let healthy = Address::generate(&e);
    assert_eq!(client.apply_consumption(&router, &healthy, &1), -1);
    assert_eq!(client.watt_balance(&healthy), -1);
}

#[test]
fn apply_consumption_requires_router_authorization() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.deposit(&account, &100);

    e.set_auths(&[]);
    assert!(client
        .try_apply_consumption(&router, &account, &10)
        .is_err());

    e.mock_all_auths();
    assert_eq!(client.watt_balance(&account), 100);
    assert_eq!(client.apply_consumption(&router, &account, &10), 90);
}

#[test]
fn revoked_router_can_no_longer_charge() {
    let (e, admin, client) = setup();
    let router = Address::generate(&e);
    let account = Address::generate(&e);
    client.authorize_router(&admin, &router);
    client.deposit(&account, &100);
    assert_eq!(client.apply_consumption(&router, &account, &10), 90);

    client.revoke_router(&admin, &router);
    assert!(!client.is_router(&router));
    assert_eq!(
        client.try_apply_consumption(&router, &account, &10),
        Err(Ok(Error::NotARouter))
    );
    assert_eq!(client.watt_balance(&account), 90);

    client.authorize_router(&admin, &router);
    assert_eq!(client.apply_consumption(&router, &account, &10), 80);
}

#[test]
fn balance_write_paths_renew_the_lifetime_of_both_balance_entries() {
    let (e, admin, id, client) = setup_tracked();
    let account = Address::generate(&e);
    let router = Address::generate(&e);
    client.authorize_router(&admin, &router);
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);

    client.deposit(&account, &100);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("bal"), account.clone())), max);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("wat"), account.clone())), max);

    client.withdraw(&account, &20);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("bal"), account.clone())), max);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("wat"), account.clone())), max);

    client.credit_watts(&admin, &account, &40);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("wat"), account.clone())), max);

    client.apply_consumption(&router, &account, &10);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("wat"), account.clone())), max);
}

#[test]
fn router_grant_renews_the_lifetime_of_the_capability_entry() {
    let (e, admin, id, client) = setup_tracked();
    let router = Address::generate(&e);
    let max = network_max_ttl(&e, &id);

    client.authorize_router(&admin, &router);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("rtr"), router.clone())), max);

    client.revoke_router(&admin, &router);
    client.authorize_router(&admin, &router);
    assert_eq!(entry_ttl(&e, &id, &(symbol_short!("rtr"), router)), max);
}

#[test]
fn renewal_outlives_the_minimum_entry_ttl_and_leaves_other_accounts_alone() {
    let (e, admin, id, client) = setup_tracked();
    let touched = Address::generate(&e);
    let untouched = Address::generate(&e);
    client.deposit(&touched, &100);
    client.credit_watts(&admin, &touched, &5);
    let max = network_max_ttl(&e, &id);

    let renewed = entry_ttl(&e, &id, &(symbol_short!("wat"), touched));
    assert_eq!(renewed, max);

    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert!(!e.as_contract(&id, || {
        e.storage()
            .persistent()
            .has(&(symbol_short!("bal"), untouched.clone()))
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
    assert_eq!(client.rate(), SCALE);
}

#[test]
fn an_idle_ledger_no_longer_bricks_the_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    let account = Address::generate(&e);
    client.deposit(&account, &100);

    let idle = e.ledger().get().min_persistent_entry_ttl + 1;
    e.ledger().with_mut(|l| l.sequence_number += idle);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);

    assert_eq!(client.admin(), admin);
    assert_eq!(client.rate(), SCALE);
    assert_eq!(client.deposit(&account, &50), 50);
}

#[test]
fn set_token_stores_and_token_getter_reflects_it() {
    let (e, admin, client) = setup();
    assert_eq!(client.token(), None);
    let token = Address::generate(&e);
    client.set_token(&admin, &token);
    assert_eq!(client.token(), Some(token));
}

#[test]
fn set_token_requires_admin_authority() {
    let (e, _admin, client) = setup();
    let impostor = Address::generate(&e);
    let token = Address::generate(&e);
    assert_eq!(
        client.try_set_token(&impostor, &token),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.token(), None);
}

#[test]
fn deposit_secured_without_token_is_rejected() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    assert_eq!(
        client.try_deposit_secured(&account, &100),
        Err(Ok(Error::TokenNotSet))
    );
    assert_eq!(client.watt_balance(&account), 0);
    assert_eq!(client.stable_balance(&account), 0);
}

#[test]
fn withdraw_secured_without_token_is_rejected() {
    let (e, _admin, client) = setup();
    let account = Address::generate(&e);
    client.deposit(&account, &100);
    assert_eq!(
        client.try_withdraw_secured(&account, &50),
        Err(Ok(Error::TokenNotSet))
    );
    assert_eq!(client.watt_balance(&account), 100);
}

#[test]
fn deposit_secured_rejects_non_positive_amounts() {
    let (e, admin, client) = setup();
    let token = Address::generate(&e);
    client.set_token(&admin, &token);
    let account = Address::generate(&e);
    assert_eq!(
        client.try_deposit_secured(&account, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_deposit_secured(&account, &-10),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn withdraw_secured_rejects_non_positive_credits() {
    let (e, admin, client) = setup();
    let token = Address::generate(&e);
    client.set_token(&admin, &token);
    let account = Address::generate(&e);
    assert_eq!(
        client.try_withdraw_secured(&account, &0),
        Err(Ok(Error::InvalidCredits))
    );
    assert_eq!(
        client.try_withdraw_secured(&account, &-10),
        Err(Ok(Error::InvalidCredits))
    );
}

#[test]
fn deposit_secured_rejects_dust_below_one_credit() {
    let (e, admin, client) = setup_with_rate(1);
    let token = Address::generate(&e);
    client.set_token(&admin, &token);
    let account = Address::generate(&e);
    // rate 1: amount 1 mints 1*1/1_000_000 = 0 credits -> must fail
    // BEFORE any token movement, not after.
    assert_eq!(
        client.try_deposit_secured(&account, &1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.watt_balance(&account), 0);
    assert_eq!(client.stable_balance(&account), 0);
}
