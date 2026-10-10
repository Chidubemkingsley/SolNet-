use soroban_sdk::{
    testutils::{storage::Instance as _, Address as _, Ledger as _},
    Address, Env,
};
use solnet_peak_load_adjuster::{Error, PeakLoadAdjuster, PeakLoadAdjusterClient};

fn setup_with(base_price: i128, floor_price: i128) -> (Env, Address, PeakLoadAdjusterClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(PeakLoadAdjuster, ());
    let client = PeakLoadAdjusterClient::new(&e, &id);
    client.init(&admin, &base_price, &floor_price);
    (e, admin, client)
}

fn setup() -> (Env, Address, PeakLoadAdjusterClient<'static>) {
    setup_with(100, 10)
}

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect the contract instance.
fn setup_tracked() -> (Env, Address, Address, PeakLoadAdjusterClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(PeakLoadAdjuster, ());
    let client = PeakLoadAdjusterClient::new(&e, &id);
    client.init(&admin, &100, &10);
    (e, admin, id, client)
}

fn network_max_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().max_ttl())
}

fn instance_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().instance().get_ttl())
}

#[test]
fn init_stores_configuration_and_blocks_reinit() {
    let (e, admin, client) = setup();

    assert_eq!(client.admin(), admin);
    assert_eq!(client.base_price(), 100);
    assert_eq!(client.floor_price(), 10);
    assert_eq!(client.max_price(), 150);
    assert_eq!(client.soc(), 10_000);
    assert_eq!(client.price_multiplier_bps(), 10_000);
    assert_eq!(client.current_price(), 100);

    let impostor = Address::generate(&e);
    assert_eq!(client.try_init(&impostor, &1, &1), Err(Ok(Error::AlreadyInitialized)));
    assert_eq!(client.base_price(), 100);
}

#[test]
fn operations_before_init_are_rejected() {
    let e = Env::default();
    e.mock_all_auths();
    let id = e.register(PeakLoadAdjuster, ());
    let client = PeakLoadAdjusterClient::new(&e, &id);
    let admin = Address::generate(&e);

    assert_eq!(client.try_admin(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_soc(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_base_price(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_floor_price(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_max_price(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_current_price(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_price_multiplier_bps(),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_update_soc(&admin, &5_000),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_set_max_price(&admin, &200),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
fn init_rejects_non_positive_prices() {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(PeakLoadAdjuster, ());
    let client = PeakLoadAdjusterClient::new(&e, &id);

    assert_eq!(
        client.try_init(&admin, &0, &10),
        Err(Ok(Error::InvalidBasePrice))
    );
    assert_eq!(
        client.try_init(&admin, &-100, &10),
        Err(Ok(Error::InvalidBasePrice))
    );
    assert_eq!(
        client.try_init(&admin, &100, &0),
        Err(Ok(Error::InvalidFloorPrice))
    );
    assert_eq!(
        client.try_init(&admin, &100, &-1),
        Err(Ok(Error::InvalidFloorPrice))
    );
    assert_eq!(client.try_admin(), Err(Ok(Error::NotInitialized)));

    client.init(&admin, &100, &10);
    assert_eq!(client.base_price(), 100);
}

#[test]
fn update_soc_requires_the_admin() {
    let (e, admin, client) = setup();
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_update_soc(&impostor, &5_000),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.soc(), 10_000);

    assert_eq!(
        client.try_set_max_price(&impostor, &200),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.max_price(), 150);

    client.update_soc(&admin, &5_000);
    assert_eq!(client.soc(), 5_000);
}

#[test]
fn update_soc_rejects_out_of_range_readings() {
    let (_e, admin, client) = setup();

    assert_eq!(
        client.try_update_soc(&admin, &-1),
        Err(Ok(Error::InvalidSoc))
    );
    assert_eq!(
        client.try_update_soc(&admin, &10_001),
        Err(Ok(Error::InvalidSoc))
    );
    assert_eq!(client.soc(), 10_000);

    client.update_soc(&admin, &0);
    assert_eq!(client.soc(), 0);
    client.update_soc(&admin, &10_000);
    assert_eq!(client.soc(), 10_000);
}

#[test]
fn price_multiplier_matches_the_documented_curve() {
    let (_e, admin, client) = setup_with(1_000, 1);

    for (soc_bps, expected) in [
        (10_000i128, 10_000i128),
        (9_000, 10_500),
        (7_500, 11_250),
        (5_000, 12_500),
        (2_500, 13_750),
        (1, 14_999),
        (0, 15_000),
    ] {
        client.update_soc(&admin, &soc_bps);
        assert_eq!(client.price_multiplier_bps(), expected, "soc {}", soc_bps);
    }
}

#[test]
fn current_price_equals_base_price_at_full_charge() {
    let (_e, _admin, client) = setup_with(37, 1);
    assert_eq!(client.soc(), 10_000);
    assert_eq!(client.price_multiplier_bps(), 10_000);
    assert_eq!(client.current_price(), 37);

    let (_e2, _admin2, tiny) = setup_with(1, 1);
    assert_eq!(tiny.max_price(), 1);
    assert_eq!(tiny.current_price(), 1);
}

#[test]
fn current_price_scales_with_base_price_across_soc() {
    let (_e, admin, client) = setup_with(100, 10);

    for (soc_bps, expected) in
        [(10_000i128, 100i128), (7_500, 112), (5_000, 125), (2_500, 137), (0, 150)]
    {
        client.update_soc(&admin, &soc_bps);
        assert_eq!(client.current_price(), expected, "soc {}", soc_bps);
    }

    let (_e2, admin2, odd) = setup_with(7, 1);
    odd.update_soc(&admin2, &5_000);
    assert_eq!(odd.price_multiplier_bps(), 12_500);
    assert_eq!(odd.current_price(), 8);
}

#[test]
fn current_price_is_clamped_up_to_the_floor() {
    let (_e, admin, client) = setup_with(7, 10);
    assert_eq!(client.max_price(), 10);

    assert_eq!(client.current_price(), 10);
    client.update_soc(&admin, &5_000);
    assert_eq!(client.current_price(), 10);
    client.update_soc(&admin, &0);
    assert_eq!(client.current_price(), 10);
    client.update_soc(&admin, &10_000);
    assert_eq!(client.price_multiplier_bps(), 10_000);
    assert_eq!(client.current_price(), 10);
}

#[test]
fn set_max_price_rejects_a_ceiling_below_the_floor() {
    let (e, admin, client) = setup();
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_set_max_price(&admin, &9),
        Err(Ok(Error::InvalidMaxPrice))
    );
    assert_eq!(
        client.try_set_max_price(&admin, &0),
        Err(Ok(Error::InvalidMaxPrice))
    );
    assert_eq!(
        client.try_set_max_price(&admin, &-5),
        Err(Ok(Error::InvalidMaxPrice))
    );
    assert_eq!(client.max_price(), 150);

    client.set_max_price(&admin, &10);
    assert_eq!(client.max_price(), 10);

    assert_eq!(
        client.try_set_max_price(&impostor, &11),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.max_price(), 10);
}

#[test]
fn set_max_price_raises_and_lowers_the_ceiling() {
    let (_e, admin, client) = setup_with(100, 10);

    client.set_max_price(&admin, &200);
    client.update_soc(&admin, &0);
    assert_eq!(client.current_price(), 150);

    client.set_max_price(&admin, &120);
    assert_eq!(client.current_price(), 120);

    client.set_max_price(&admin, &10);
    client.update_soc(&admin, &10_000);
    assert_eq!(client.current_price(), 10);
    client.update_soc(&admin, &5_000);
    assert_eq!(client.current_price(), 10);
}

#[test]
fn set_max_price_reconfigures_the_ceiling_without_moving_the_curve() {
    let (_e, admin, client) = setup_with(100, 10);

    for soc_bps in [10_000i128, 6_000, 0] {
        client.update_soc(&admin, &soc_bps);
        let before = client.price_multiplier_bps();
        client.set_max_price(&admin, &111);
        assert_eq!(client.price_multiplier_bps(), before);
        assert_eq!(client.soc(), soc_bps);
        assert_eq!(client.max_price(), 111);
    }
}

#[test]
fn current_price_is_overflow_safe_for_an_extreme_base_price() {
    let (_e, admin, client) = setup_with(i128::MAX, 1);
    assert_eq!(client.max_price(), i128::MAX);

    assert_eq!(client.current_price(), i128::MAX);

    client.update_soc(&admin, &0);
    assert_eq!(client.price_multiplier_bps(), 15_000);
    assert_eq!(client.current_price(), i128::MAX);

    client.update_soc(&admin, &5_000);
    assert_eq!(client.current_price(), i128::MAX);

    client.set_max_price(&admin, &(i128::MAX - 1));
    client.update_soc(&admin, &0);
    assert_eq!(client.current_price(), i128::MAX - 1);
}

#[test]
fn repeated_soc_updates_move_the_price_both_directions() {
    let (_e, admin, client) = setup_with(100, 10);

    client.update_soc(&admin, &5_000);
    assert_eq!(client.current_price(), 125);
    client.update_soc(&admin, &2_000);
    assert_eq!(client.price_multiplier_bps(), 14_000);
    assert_eq!(client.current_price(), 140);
    client.update_soc(&admin, &9_000);
    assert_eq!(client.current_price(), 105);
    client.update_soc(&admin, &10_000);
    assert_eq!(client.current_price(), 100);

    for step in 0..=20u32 {
        let soc_bps = (step as i128) * 500;
        client.update_soc(&admin, &soc_bps);
        let price = client.current_price();
        assert!(price >= client.floor_price());
        assert!(price <= client.max_price());
    }
}

#[test]
fn floor_and_ceiling_never_intersect_across_reconfiguration() {
    let (e, admin, client) = setup_with(100, 30);
    let other = Address::generate(&e);

    assert_eq!(client.max_price(), 150);
    client.set_max_price(&admin, &80);

    for soc_bps in (0..=10_000i128).step_by(250) {
        client.update_soc(&admin, &soc_bps);
        let price = client.current_price();
        assert!(price >= 30, "floor breached at soc {}", soc_bps);
        assert!(price <= 80, "ceiling breached at soc {}", soc_bps);
    }

    assert_eq!(
        client.try_set_max_price(&other, &90),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.max_price(), 80);
}

#[test]
fn configuration_writes_renew_the_lifetime_of_the_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);
    assert_eq!(instance_ttl(&e, &id), max);

    client.update_soc(&admin, &5_000);
    assert_eq!(instance_ttl(&e, &id), max);

    client.set_max_price(&admin, &200);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn a_configuration_write_restores_a_decayed_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);
    assert_eq!(client.soc(), 10_000);

    client.update_soc(&admin, &5_000);
    assert_eq!(client.soc(), 5_000);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}