use soroban_sdk::{testutils::Address as _, Address, Env};
use solnet_community_capital::{CommunityCapital, CommunityCapitalClient, Error};

fn setup() -> (Env, Address, CommunityCapitalClient<'static>) {
    setup_with(500)
}

fn setup_with(yield_bps: i128) -> (Env, Address, CommunityCapitalClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(CommunityCapital, ());
    let client = CommunityCapitalClient::new(&e, &id);
    client.init(&admin, &yield_bps);
    (e, admin, client)
}

#[test]
fn init_stores_config_and_is_not_repeatable() {
    let (e, admin, client) = setup_with(750);
    assert_eq!(client.admin(), admin);
    assert_eq!(client.yield_bps(), 750);
    assert_eq!(client.total_shares(), 0);
    assert_eq!(client.total_capital(), 0);
    assert_eq!(client.undistributed(), 0);
    assert_eq!(client.share_price(), 10_000);
    assert_eq!(client.try_init(&admin, &750), Err(Ok(Error::AlreadyInitialized)));
    let other = Address::generate(&e);
    assert_eq!(client.try_init(&other, &1), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn init_rejects_out_of_range_yield_rate() {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(CommunityCapital, ());
    let client = CommunityCapitalClient::new(&e, &id);

    assert_eq!(
        client.try_init(&admin, &10_001),
        Err(Ok(Error::InvalidYieldBps))
    );
    assert_eq!(
        client.try_init(&admin, &-1),
        Err(Ok(Error::InvalidYieldBps))
    );

    client.init(&admin, &10_000);
    assert_eq!(client.yield_bps(), 10_000);
}

#[test]
fn invest_mints_shares_at_initial_share_price() {
    let (e, _admin, client) = setup();
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);

    assert_eq!(client.invest(&alice, &1_000), 1_000);
    assert_eq!(client.shares_of(&alice), 1_000);
    assert_eq!(client.shares_of(&bob), 0);
    assert_eq!(client.total_shares(), 1_000);
    assert_eq!(client.total_capital(), 1_000);
    assert_eq!(client.share_price(), 10_000);

    assert_eq!(client.invest(&bob, &2_500), 2_500);
    assert_eq!(client.total_shares(), 3_500);
    assert_eq!(client.total_capital(), 3_500);
    assert_eq!(client.share_price(), 10_000);
}

#[test]
fn invest_and_claim_require_investor_auth() {
    let (e, _admin, client) = setup();
    let mallory = Address::generate(&e);
    e.mock_auths(&[]);

    assert!(client.try_invest(&mallory, &1_000).is_err());
    assert!(client.try_claim_dividend(&mallory).is_err());
}

#[test]
fn invest_rejects_non_positive_amount() {
    let (e, _admin, client) = setup();
    let alice = Address::generate(&e);

    assert_eq!(
        client.try_invest(&alice, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_invest(&alice, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.total_shares(), 0);
    assert_eq!(client.total_capital(), 0);
}

#[test]
fn invest_rejects_amount_rounding_to_zero_shares() {
    let (e, admin, client) = setup();
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);

    client.invest(&alice, &100_000);
    client.accrue_yield(&admin);
    assert_eq!(client.share_price(), 10_500);

    assert_eq!(client.try_invest(&bob, &1), Err(Ok(Error::ZeroShares)));
    assert_eq!(client.try_invest(&bob, &-3), Err(Ok(Error::InvalidAmount)));
    assert_eq!(client.invest(&bob, &1_050), 1_000);
    assert_eq!(client.total_shares(), 101_000);
    assert_eq!(client.share_price(), 10_500);
}

#[test]
fn invest_rejects_overflowing_amount() {
    let (e, _admin, client) = setup();
    let alice = Address::generate(&e);

    assert_eq!(
        client.try_invest(&alice, &i128::MAX),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.total_shares(), 0);
}

#[test]
fn accrue_yield_credits_configured_dividend() {
    let (e, admin, client) = setup();
    let alice = Address::generate(&e);
    client.invest(&alice, &1_000);

    assert_eq!(client.accrue_yield(&admin), 50);
    assert_eq!(client.undistributed(), 50);
    assert_eq!(client.total_capital(), 1_050);
    assert_eq!(client.share_price(), 10_500);
    assert_eq!(client.total_shares(), 1_000);
}

#[test]
fn accrue_yield_is_guarded_per_ledger_sequence() {
    use soroban_sdk::testutils::Ledger as _;

    let (e, admin, client) = setup();
    let alice = Address::generate(&e);
    client.invest(&alice, &1_000);

    assert_eq!(client.accrue_yield(&admin), 50);
    assert_eq!(
        client.try_accrue_yield(&admin),
        Err(Ok(Error::DuplicateAccrual))
    );
    assert_eq!(client.undistributed(), 50);

    e.ledger().set_sequence_number(7);
    assert_eq!(client.accrue_yield(&admin), 52);
    assert_eq!(client.undistributed(), 102);

    assert_eq!(
        client.try_accrue_yield(&admin),
        Err(Ok(Error::DuplicateAccrual))
    );
    e.ledger().set_sequence_number(8);
    assert_eq!(client.accrue_yield(&admin), 55);
    assert_eq!(client.undistributed(), 157);
}

#[test]
fn accrue_yield_requires_admin() {
    let (e, _admin, client) = setup();
    let alice = Address::generate(&e);
    let impostor = Address::generate(&e);
    client.invest(&alice, &1_000);

    assert_eq!(
        client.try_accrue_yield(&impostor),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.undistributed(), 0);

    assert_eq!(client.accrue_yield(&_admin), 50);
}

#[test]
fn accrue_yield_requires_shares_outstanding() {
    let (_e, admin, client) = setup();
    assert_eq!(
        client.try_accrue_yield(&admin),
        Err(Ok(Error::NoShares))
    );
    assert_eq!(client.undistributed(), 0);
}

#[test]
fn accrue_yield_rejects_zero_rate_fund() {
    let (e, admin, client) = setup_with(0);
    let alice = Address::generate(&e);
    client.invest(&alice, &5_000);

    assert_eq!(
        client.try_accrue_yield(&admin),
        Err(Ok(Error::NothingAccrued))
    );
    assert_eq!(client.undistributed(), 0);
    assert_eq!(client.total_capital(), 5_000);
}

#[test]
fn claim_dividend_pays_pro_rata_to_each_holder() {
    let (e, admin, client) = setup();
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let carol = Address::generate(&e);

    client.invest(&alice, &1_000);
    client.invest(&bob, &2_000);
    client.invest(&carol, &1_000);
    assert_eq!(client.total_shares(), 4_000);

    assert_eq!(client.accrue_yield(&admin), 200);
    assert_eq!(client.claim_dividend(&alice), 50);
    assert_eq!(client.claim_dividend(&bob), 100);
    assert_eq!(client.claim_dividend(&carol), 50);
    assert_eq!(client.undistributed(), 0);

    assert_eq!(
        client.try_claim_dividend(&alice),
        Err(Ok(Error::NothingAccrued))
    );
}

#[test]
fn claim_dividend_conserves_pool_across_many_holders() {
    let (e, admin, client) = setup_with(705);
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let carol = Address::generate(&e);

    client.invest(&alice, &1_000);
    client.invest(&bob, &1_000);
    client.invest(&carol, &1_000);

    let accrued = client.accrue_yield(&admin);
    assert_eq!(accrued, 211);

    let a_pay = client.claim_dividend(&alice);
    let b_pay = client.claim_dividend(&bob);
    let c_pay = client.claim_dividend(&carol);

    assert_eq!(a_pay, 211 * 1_000 / 3_000);
    assert_eq!(b_pay, 211 * 1_000 / 3_000);
    assert_eq!(c_pay, 211 * 1_000 / 3_000);

    let claimed = a_pay + b_pay + c_pay;
    assert_eq!(client.undistributed() + claimed, accrued);
    assert_eq!(client.undistributed(), accrued - claimed);
    assert!(claimed <= accrued);
}

#[test]
fn claim_dividend_rejects_empty_pool_and_non_holders() {
    let (e, admin, client) = setup();
    let alice = Address::generate(&e);
    let bystander = Address::generate(&e);

    assert_eq!(
        client.try_claim_dividend(&bystander),
        Err(Ok(Error::NoShares))
    );

    client.invest(&alice, &1_000);
    assert_eq!(
        client.try_claim_dividend(&alice),
        Err(Ok(Error::NothingAccrued))
    );
    assert_eq!(
        client.try_claim_dividend(&bystander),
        Err(Ok(Error::NoShares))
    );

    client.accrue_yield(&admin);
    assert_eq!(client.claim_dividend(&alice), 50);
    assert_eq!(
        client.try_claim_dividend(&bystander),
        Err(Ok(Error::NoShares))
    );
}