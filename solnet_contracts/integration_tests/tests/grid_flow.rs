use soroban_sdk::{testutils::Address as _, Address, Env};
use solnet_account_ledger::{AccountLedger, AccountLedgerClient};
use solnet_delinquency_disconnector::{
    DelinquencyDisconnector, DelinquencyDisconnectorClient, Verdict,
};
use solnet_micro_billing::{MicroBilling, MicroBillingClient};

const RATE: i128 = 1_000_000;
const PRICE_PER_MWH: i128 = 1_000;
const GRACE: u64 = 100;

struct Grid {
    env: Env,
    admin: Address,
    ledger: AccountLedgerClient<'static>,
    ledger_id: Address,
    billing: MicroBillingClient<'static>,
    billing_id: Address,
    disc: DelinquencyDisconnectorClient<'static>,
}

fn build() -> Grid {
    build_with_rate(RATE)
}

fn build_with_rate(rate: i128) -> Grid {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let ledger_id = env.register(AccountLedger, ());
    let ledger = AccountLedgerClient::new(&env, &ledger_id);
    ledger.init(&admin, &rate);

    let billing_id = env.register(MicroBilling, ());
    let billing = MicroBillingClient::new(&env, &billing_id);
    billing.init(&admin, &PRICE_PER_MWH);

    let disc_id = env.register(DelinquencyDisconnector, ());
    let disc = DelinquencyDisconnectorClient::new(&env, &disc_id);
    disc.init(&admin, &GRACE);

    Grid {
        env,
        admin,
        ledger,
        ledger_id,
        billing,
        billing_id,
        disc,
    }
}

fn wire(g: Grid) -> Grid {
    g.ledger.authorize_router(&g.admin, &g.billing_id);
    g.billing.wire(&g.admin, &g.ledger_id);
    g.disc.wire(&g.admin, &g.ledger_id);
    g
}

fn wired() -> Grid {
    wire(build())
}

#[test]
fn reading_debits_the_shared_ledger() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);
    assert_eq!(g.ledger.watt_balance(&account), 100);

    let charge = g.billing.record(&account, &1, &10, &1);
    assert_eq!(charge, 10);
    assert_eq!(g.billing.unsettled(&account), 10);
    assert_eq!(g.ledger.watt_balance(&account), 90);
}

#[test]
fn repeated_readings_accumulate_against_the_same_balance() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    g.billing.record(&account, &1, &10, &1);
    g.billing.record(&account, &1, &20, &2);
    g.billing.record(&account, &1, &15, &3);

    assert_eq!(g.billing.unsettled(&account), 45);
    assert_eq!(g.ledger.watt_balance(&account), 55);
    assert_eq!(g.ledger.stable_balance(&account), 100);
}

#[test]
fn unwired_router_does_not_debit_any_ledger() {
    let g = build();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    g.billing.record(&account, &1, &10, &1);
    assert_eq!(g.billing.unsettled(&account), 10);
    assert_eq!(g.ledger.watt_balance(&account), 100);
}

#[test]
fn wiring_requires_the_router_to_be_authorized_first() {
    let g = build();
    assert_eq!(
        g.billing.try_wire(&g.admin, &g.ledger_id),
        Err(Ok(solnet_micro_billing::Error::RouterNotAuthorized))
    );
    assert_eq!(g.billing.ledger(), None);
}

#[test]
fn revoked_router_stops_billing_the_ledger() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    g.ledger.revoke_router(&g.admin, &g.billing_id);
    assert_eq!(
        g.billing.try_record(&account, &1, &10, &1),
        Err(Ok(solnet_micro_billing::Error::RouterNotAuthorized))
    );
    assert_eq!(g.ledger.watt_balance(&account), 100);
}

#[test]
fn unauthorized_address_cannot_charge_the_ledger() {
    let g = wired();
    let account = Address::generate(&g.env);
    let stranger = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    assert_eq!(
        g.ledger.try_apply_consumption(&stranger, &account, &10),
        Err(Ok(solnet_account_ledger::Error::NotARouter))
    );
    assert_eq!(g.ledger.watt_balance(&account), 100);
}

#[test]
fn consumption_is_debit_only() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    assert_eq!(
        g.ledger.try_apply_consumption(&g.billing_id, &account, &-50),
        Err(Ok(solnet_account_ledger::Error::InvalidAmount))
    );
    assert_eq!(
        g.ledger.try_apply_consumption(&g.billing_id, &account, &0),
        Err(Ok(solnet_account_ledger::Error::InvalidAmount))
    );
    assert_eq!(g.ledger.watt_balance(&account), 100);

    assert_eq!(
        g.ledger.try_apply_consumption(&g.billing_id, &account, &10),
        Ok(Ok(90))
    );
}

#[test]
fn full_disconnect_loop_runs_end_to_end() {
    let g = wired();
    let account = Address::generate(&g.env);

    g.ledger.deposit(&account, &5);
    assert_eq!(g.disc.assess(&account, &0), Verdict::NotDelinquent);

    g.billing.record(&account, &1, &10, &1);
    assert_eq!(g.ledger.watt_balance(&account), -5);

    assert_eq!(g.disc.assess(&account, &0), Verdict::GracePeriod);
    assert_eq!(g.disc.assess(&account, &99), Verdict::GracePeriod);
    assert_eq!(g.disc.assess(&account, &100), Verdict::Disconnect);

    let instruction = g.disc.issue_disconnect(&account);
    assert_eq!(instruction.len(), 32);
}

#[test]
fn healthy_account_never_reaches_disconnect() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);
    g.billing.record(&account, &1, &10, &1);

    for seq in [0u64, 50, 100, 500, 10_000] {
        assert_eq!(g.disc.assess(&account, &seq), Verdict::NotDelinquent);
    }
    assert_eq!(
        g.disc.try_issue_disconnect(&account),
        Err(Ok(solnet_delinquency_disconnector::Error::NotDelinquent))
    );
}

#[test]
fn repaying_clears_delinquency_and_restores_the_grace_window() {
    let g = wired();
    let account = Address::generate(&g.env);

    g.ledger.deposit(&account, &5);
    g.billing.record(&account, &1, &10, &1);
    assert_eq!(g.disc.assess(&account, &0), Verdict::GracePeriod);
    assert_eq!(g.disc.assess(&account, &500), Verdict::Disconnect);

    g.ledger.deposit(&account, &20);
    assert_eq!(g.disc.assess(&account, &500), Verdict::NotDelinquent);

    g.billing.record(&account, &1, &30, &2);
    assert!(g.ledger.watt_balance(&account) < 0);
    assert_eq!(g.disc.assess(&account, &600), Verdict::GracePeriod);
    assert_eq!(g.disc.assess(&account, &(600 + GRACE)), Verdict::Disconnect);
}

#[test]
fn assessed_accounts_are_isolated_from_each_other() {
    let g = wired();
    let good = Address::generate(&g.env);
    let bad = Address::generate(&g.env);

    g.ledger.deposit(&good, &100);
    g.ledger.deposit(&bad, &5);
    g.billing.record(&bad, &2, &10, &1);

    assert_eq!(g.disc.assess(&good, &0), Verdict::NotDelinquent);
    assert_eq!(g.disc.assess(&bad, &0), Verdict::GracePeriod);
    assert_eq!(g.disc.assess(&good, &1_000), Verdict::NotDelinquent);
}

#[test]
fn disconnector_refuses_to_assess_before_wiring() {
    let g = build();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);
    assert_eq!(
        g.disc.try_assess(&account, &0),
        Err(Ok(solnet_delinquency_disconnector::Error::LedgerCallFailed))
    );
}

#[test]
fn replayed_reading_does_not_double_charge_the_ledger() {
    let g = wired();
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);

    g.billing.record(&account, &1, &10, &7);
    assert_eq!(g.ledger.watt_balance(&account), 90);

    assert_eq!(
        g.billing.try_record(&account, &1, &10, &7),
        Err(Ok(solnet_micro_billing::Error::DuplicateNonce))
    );
    assert_eq!(g.ledger.watt_balance(&account), 90);
}

#[test]
fn charge_is_converted_to_watt_credits_at_the_ledger_rate() {
    // A rate of 2_000_000 means two Watt-credits are minted per stablecoin unit,
    // so a charge of 10 currency units must burn 20 Watt-credits. If the router
    // ever charged raw currency units instead of converting, this would show
    // a burn of 10 and fail.
    let g = wire(build_with_rate(2_000_000));
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);
    assert_eq!(g.ledger.watt_balance(&account), 200);

    let charge = g.billing.record(&account, &1, &10, &1);
    assert_eq!(charge, 10, "charge is denominated in currency units");
    assert_eq!(
        g.ledger.watt_balance(&account),
        180,
        "ten currency units of debt must burn twenty Watt-credits"
    );
}

#[test]
fn fractional_rate_scales_the_burn_proportionally() {
    // Half a Watt-credit per currency unit: ten units of charge burn five.
    let g = wire(build_with_rate(500_000));
    let account = Address::generate(&g.env);
    g.ledger.deposit(&account, &100);
    assert_eq!(g.ledger.watt_balance(&account), 50);

    g.billing.record(&account, &1, &10, &1);
    assert_eq!(g.ledger.watt_balance(&account), 45);
}

#[test]
fn two_independent_grids_do_not_share_balances() {
    let a = wired();
    let acct_a = Address::generate(&a.env);
    a.ledger.deposit(&acct_a, &5);
    a.billing.record(&acct_a, &1, &10, &1);

    let b = wired();
    let acct_b = Address::generate(&b.env);
    b.ledger.deposit(&acct_b, &5);

    assert_eq!(a.ledger.watt_balance(&acct_a), -5);
    assert_eq!(b.ledger.watt_balance(&acct_b), 5);
    assert_eq!(a.disc.assess(&acct_a, &0), Verdict::GracePeriod);
    assert_eq!(b.disc.assess(&acct_b, &0), Verdict::NotDelinquent);
}