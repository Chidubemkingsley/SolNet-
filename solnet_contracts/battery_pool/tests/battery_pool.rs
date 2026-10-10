use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use solnet_battery_pool::{BatteryPool, BatteryPoolClient, Error};

fn setup_with(fee_bps: i128) -> (Env, Address, BatteryPoolClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(BatteryPool, ());
    let client = BatteryPoolClient::new(&e, &id);
    client.init(&admin, &fee_bps);
    (e, admin, client)
}

fn setup() -> (Env, Address, BatteryPoolClient<'static>) {
    setup_with(250)
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn assert_invariant(client: &BatteryPoolClient<'_>) {
    assert_eq!(
        client.total_reserve(),
        client.reserved_for_maintenance() + client.available()
    );
}

#[test]
fn init_stores_fee_and_blocks_reinit() {
    let (e, admin, client) = setup();
    let impostor = Address::generate(&e);

    assert_eq!(client.admin(), admin);
    assert_eq!(client.fee_bps(), 250);
    assert_eq!(client.total_reserve(), 0);
    assert_eq!(client.reserved_for_maintenance(), 0);
    assert_eq!(client.available(), 0);

    assert_eq!(client.try_init(&impostor, &1), Err(Ok(Error::AlreadyInitialized)));
    assert_eq!(client.fee_bps(), 250);
    assert_invariant(&client);
}

#[test]
fn init_rejects_fee_outside_the_valid_range() {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(BatteryPool, ());
    let client = BatteryPoolClient::new(&e, &id);

    assert_eq!(client.try_init(&admin, &-1), Err(Ok(Error::InvalidFeeBps)));
    assert_eq!(
        client.try_init(&admin, &10_001),
        Err(Ok(Error::InvalidFeeBps))
    );
    assert_eq!(client.try_admin(), Err(Ok(Error::NotInitialized)));

    client.init(&admin, &10_000);
    assert_eq!(client.fee_bps(), 10_000);

    let (_e2, _admin2, zero) = setup_with(0);
    assert_eq!(zero.fee_bps(), 0);
    assert_eq!(zero.collect_fee(&1_000_000), 0);
}

#[test]
fn operations_before_init_are_rejected() {
    let e = Env::default();
    e.mock_all_auths();
    let id = e.register(BatteryPool, ());
    let client = BatteryPoolClient::new(&e, &id);
    let admin = Address::generate(&e);
    let payer = Address::generate(&e);

    assert_eq!(client.try_admin(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_fee_bps(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_total_reserve(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_reserved_for_maintenance(),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(client.try_available(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_is_reserved(&hash(&e, 1)),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_contribute(&payer, &100),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_collect_fee(&100),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &100, &hash(&e, 1)),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
fn contribute_rejects_non_positive_amounts() {
    let (e, _admin, client) = setup();
    let payer = Address::generate(&e);

    assert_eq!(
        client.try_contribute(&payer, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_contribute(&payer, &-100),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.total_reserve(), 0);
    assert_eq!(client.contribution(&payer), 0);
}

#[test]
fn contribute_accumulates_reserve_and_records_the_account() {
    let (e, _admin, client) = setup();
    let a = Address::generate(&e);
    let b = Address::generate(&e);

    client.contribute(&a, &500);
    assert_eq!(client.total_reserve(), 500);
    assert_eq!(client.contribution(&a), 500);

    client.contribute(&a, &250);
    client.contribute(&b, &1);
    assert_eq!(client.total_reserve(), 751);
    assert_eq!(client.contribution(&a), 750);
    assert_eq!(client.contribution(&b), 1);
    assert_eq!(client.available(), 751);
    assert_invariant(&client);
}

#[test]
fn collect_fee_routes_the_exact_basis_point_share() {
    let (_e, _admin, client) = setup_with(250);

    assert_eq!(client.collect_fee(&10_000), 250);
    assert_eq!(client.total_reserve(), 250);

    assert_eq!(client.collect_fee(&1_000), 25);
    assert_eq!(client.total_reserve(), 275);

    assert_eq!(client.collect_fee(&0), 0);
    assert_eq!(client.total_reserve(), 275);
    assert_invariant(&client);
}

#[test]
fn collect_fee_rounds_down_for_tiny_settlements() {
    let (_e, _admin, client) = setup_with(250);

    assert_eq!(client.collect_fee(&1), 0);
    assert_eq!(client.collect_fee(&3), 0);
    assert_eq!(client.collect_fee(&39), 0);
    assert_eq!(client.total_reserve(), 0);

    assert_eq!(client.collect_fee(&40), 1);
    assert_eq!(client.total_reserve(), 1);
    assert_invariant(&client);

    let (_e2, _admin2, full) = setup_with(10_000);
    assert_eq!(full.collect_fee(&7), 7);
    assert_eq!(full.total_reserve(), 7);
}

#[test]
fn collect_fee_rejects_negative_settlements() {
    let (e, _admin, client) = setup();
    let payer = Address::generate(&e);
    client.contribute(&payer, &42);

    assert_eq!(
        client.try_collect_fee(&-1),
        Err(Ok(Error::InvalidSettlement))
    );
    assert_eq!(
        client.try_collect_fee(&i128::MIN),
        Err(Ok(Error::InvalidSettlement))
    );
    assert_eq!(client.total_reserve(), 42);
    assert_invariant(&client);

}

#[test]
fn reserve_for_maintenance_requires_the_admin() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);
    let impostor = Address::generate(&e);
    client.contribute(&payer, &1_000);

    assert_eq!(
        client.try_reserve_for_maintenance(&impostor, &400, &hash(&e, 7)),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.reserved_for_maintenance(), 0);
    assert!(!client.is_reserved(&hash(&e, 7)));

    client.reserve_for_maintenance(&admin, &400, &hash(&e, 7));
    assert_eq!(client.reserved_for_maintenance(), 400);
    assert_invariant(&client);
}

#[test]
fn reserve_for_maintenance_rejects_overspend_and_duplicate_hashes() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);
    client.contribute(&payer, &500);

    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &0, &hash(&e, 1)),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &-10, &hash(&e, 1)),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &501, &hash(&e, 1)),
        Err(Ok(Error::InsufficientReserve))
    );

    client.reserve_for_maintenance(&admin, &200, &hash(&e, 1));
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &100, &hash(&e, 1)),
        Err(Ok(Error::AlreadyReserved))
    );
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &301, &hash(&e, 2)),
        Err(Ok(Error::InsufficientReserve))
    );
    assert_eq!(client.reserved_amount(&hash(&e, 1)), 200);
    assert_eq!(client.reserved_amount(&hash(&e, 2)), 0);
    assert_eq!(client.available(), 300);
    assert_invariant(&client);
}

#[test]
fn reserve_invariant_holds_across_mixed_operations() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);

    client.contribute(&payer, &1_000);
    assert_invariant(&client);

    assert_eq!(client.collect_fee(&200_000), 5_000);
    assert_eq!(client.total_reserve(), 6_000);
    assert_invariant(&client);

    client.reserve_for_maintenance(&admin, &2_500, &hash(&e, 3));
    assert_eq!(client.reserved_for_maintenance(), 2_500);
    assert_eq!(client.available(), 3_500);
    assert_invariant(&client);

    assert_eq!(client.collect_fee(&40_000), 1_000);
    assert_eq!(client.total_reserve(), 7_000);
    assert_eq!(client.available(), 4_500);
    assert_invariant(&client);

    client.reserve_for_maintenance(&admin, &4_500, &hash(&e, 4));
    assert_eq!(client.reserved_for_maintenance(), 7_000);
    assert_eq!(client.available(), 0);
    assert_invariant(&client);
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &1, &hash(&e, 5)),
        Err(Ok(Error::InsufficientReserve))
    );

    assert_eq!(client.settle_maintenance(&admin, &hash(&e, 3), &1_000), 1_000);
    assert_eq!(client.total_reserve(), 6_000);
    assert_eq!(client.reserved_for_maintenance(), 6_000);
    assert_eq!(client.available(), 0);
    assert_invariant(&client);
}

#[test]
fn is_reserved_tracks_the_earmark_lifecycle() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);
    client.contribute(&payer, &900);

    assert!(!client.is_reserved(&hash(&e, 9)));
    client.reserve_for_maintenance(&admin, &400, &hash(&e, 9));
    assert!(client.is_reserved(&hash(&e, 9)));
    assert_eq!(client.reserved_amount(&hash(&e, 9)), 400);

    client.settle_maintenance(&admin, &hash(&e, 9), &400);
    assert!(!client.is_reserved(&hash(&e, 9)));
    assert_eq!(client.reserved_amount(&hash(&e, 9)), 0);
    assert_eq!(client.reserved_for_maintenance(), 0);
    assert_eq!(client.available(), 500);
    assert_invariant(&client);
}

#[test]
fn settle_maintenance_returns_unspent_earmark_to_available() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);
    client.contribute(&payer, &1_000);

    client.reserve_for_maintenance(&admin, &400, &hash(&e, 11));
    assert_eq!(client.available(), 600);

    assert_eq!(client.settle_maintenance(&admin, &hash(&e, 11), &250), 250);
    assert_eq!(client.total_reserve(), 750);
    assert_eq!(client.reserved_for_maintenance(), 150);
    assert_eq!(client.available(), 600);

    assert_eq!(client.settle_maintenance(&admin, &hash(&e, 11), &0), 0);
    assert_eq!(client.total_reserve(), 750);
    assert_eq!(client.reserved_for_maintenance(), 150);

    client.settle_maintenance(&admin, &hash(&e, 11), &150);
    assert_eq!(client.total_reserve(), 600);
    assert_eq!(client.reserved_for_maintenance(), 0);
    assert_eq!(client.available(), 600);
    assert_invariant(&client);
}

#[test]
fn settle_maintenance_rejects_unknown_hashes_and_overspend() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);
    let impostor = Address::generate(&e);
    client.contribute(&payer, &500);
    client.reserve_for_maintenance(&admin, &100, &hash(&e, 2));

    assert_eq!(
        client.try_settle_maintenance(&admin, &hash(&e, 8), &50),
        Err(Ok(Error::NotReserved))
    );
    assert_eq!(
        client.try_settle_maintenance(&admin, &hash(&e, 2), &101),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_settle_maintenance(&admin, &hash(&e, 2), &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_settle_maintenance(&impostor, &hash(&e, 2), &10),
        Err(Ok(Error::NotAdmin))
    );

    assert_eq!(client.total_reserve(), 500);
    assert_eq!(client.reserved_for_maintenance(), 100);
    assert!(client.is_reserved(&hash(&e, 2)));
    assert_invariant(&client);
}

#[test]
fn reserve_totals_guard_against_i128_overflow() {
    let (e, admin, client) = setup();
    let payer = Address::generate(&e);

    client.contribute(&payer, &i128::MAX);
    assert_eq!(client.total_reserve(), i128::MAX);

    assert_eq!(
        client.try_contribute(&payer, &1),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.total_reserve(), i128::MAX);
    assert_invariant(&client);

    assert_eq!(
        client.try_collect_fee(&i128::MAX),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.total_reserve(), i128::MAX);

    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &i128::MAX, &hash(&e, 1)),
        Ok(Ok(()))
    );
    assert_eq!(client.reserved_for_maintenance(), i128::MAX);
    assert_eq!(client.available(), 0);
    assert_invariant(&client);
    assert_eq!(
        client.try_reserve_for_maintenance(&admin, &1, &hash(&e, 2)),
        Err(Ok(Error::InsufficientReserve))
    );

    let (_e2, _admin2, big_fee) = setup_with(10_000);
    assert_eq!(big_fee.collect_fee(&(i128::MAX / 10_000)), i128::MAX / 10_000);
    assert_eq!(
        big_fee.try_collect_fee(&i128::MAX),
        Err(Ok(Error::ArithmeticOverflow))
    );
}