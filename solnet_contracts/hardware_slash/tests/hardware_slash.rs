use soroban_sdk::{testutils::Address as _, Address, Env};
use solnet_hardware_slash::{Error, HardwareSlash, HardwareSlashClient, Offence};

fn setup(burn_bps: i128) -> (Env, Address, Address, HardwareSlashClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let operator = Address::generate(&e);
    let id = e.register(HardwareSlash, ());
    let client = HardwareSlashClient::new(&e, &id);
    client.init(&admin, &burn_bps);
    (e, admin, operator, client)
}

#[test]
fn init_stores_burn_bps_and_empty_ledgers() {
    let (_e, admin, _op, client) = setup(2_500);
    assert_eq!(client.burn_bps(), 2_500);
    assert_eq!(client.compensation_pool(), 0);
    assert_eq!(client.total_burned(), 0);
    assert_eq!(client.total_slashed(), 0);
    assert_eq!(client.try_init(&admin, &2_500), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn init_rejects_burn_bps_outside_the_valid_range() {
    let (e, admin, _op, _client) = setup(1_000);
    let high = HardwareSlashClient::new(&e, &e.register(HardwareSlash, ()));
    assert_eq!(high.try_init(&admin, &10_001), Err(Ok(Error::InvalidBurnBps)));

    let low = HardwareSlashClient::new(&e, &e.register(HardwareSlash, ()));
    assert_eq!(low.try_init(&admin, &-1), Err(Ok(Error::InvalidBurnBps)));

    let edge = HardwareSlashClient::new(&e, &e.register(HardwareSlash, ()));
    edge.init(&admin, &10_000);
    assert_eq!(edge.burn_bps(), 10_000);
}

#[test]
fn deposit_accumulates_and_rejects_non_positive() {
    let (_e, _admin, operator, client) = setup(1_000);
    assert_eq!(
        client.try_deposit_collateral(&operator, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_deposit_collateral(&operator, &-500),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(client.collateral_of(&operator), 0);

    client.deposit_collateral(&operator, &1_000);
    client.deposit_collateral(&operator, &250);
    assert_eq!(client.collateral_of(&operator), 1_250);
    assert_eq!(client.total_deposited(), 1_250);
}

#[test]
fn slash_seizes_full_deposit_and_splits_by_burn_bps() {
    let (_e, admin, operator, client) = setup(2_500);
    client.deposit_collateral(&operator, &1_000);

    let r = client.slash(&admin, &operator, &Offence::SpoofedGeneration);
    assert_eq!(r.seized, 1_000);
    assert_eq!(r.burned, 250);
    assert_eq!(r.redistributed, 750);
    assert_eq!(r.offence, Offence::SpoofedGeneration);
    assert_eq!(client.collateral_of(&operator), 0);
    assert_eq!(client.compensation_pool(), 750);
    assert_eq!(client.total_burned(), 250);
}

#[test]
fn slash_with_zero_burn_sends_everything_to_the_pool() {
    let (_e, admin, operator, client) = setup(0);
    client.deposit_collateral(&operator, &777);

    let r = client.slash(&admin, &operator, &Offence::RouterBypass);
    assert_eq!(r.burned, 0);
    assert_eq!(r.redistributed, 777);
    assert_eq!(client.compensation_pool(), 777);
    assert_eq!(client.total_burned(), 0);
}

#[test]
fn slash_with_full_burn_destroys_everything() {
    let (_e, admin, operator, client) = setup(10_000);
    client.deposit_collateral(&operator, &400);

    let r = client.slash(&admin, &operator, &Offence::RouterBypass);
    assert_eq!(r.burned, 400);
    assert_eq!(r.redistributed, 0);
    assert_eq!(client.compensation_pool(), 0);
    assert_eq!(client.total_burned(), 400);
    assert_eq!(client.total_slashed(), 400);
}

#[test]
fn slashing_an_operator_without_collateral_is_rejected() {
    let (e, admin, _op, client) = setup(1_000);
    let stranger = Address::generate(&e);
    assert_eq!(
        client.try_slash(&admin, &stranger, &Offence::SpoofedGeneration),
        Err(Ok(Error::NoCollateral))
    );
    assert_eq!(client.total_slashed(), 0);
    assert!(!client.was_slashed(&stranger, &Offence::SpoofedGeneration));
}

#[test]
fn slashing_the_same_offence_twice_is_rejected() {
    let (_e, admin, operator, client) = setup(5_000);
    client.deposit_collateral(&operator, &1_000);
    client.slash(&admin, &operator, &Offence::SpoofedGeneration);

    assert!(client.was_slashed(&operator, &Offence::SpoofedGeneration));
    assert_eq!(
        client.try_slash(&admin, &operator, &Offence::SpoofedGeneration),
        Err(Ok(Error::AlreadySlashed))
    );
    assert_eq!(client.total_slashed(), 1_000);
}

#[test]
fn double_slash_block_survives_a_fresh_deposit() {
    let (_e, admin, operator, client) = setup(5_000);
    client.deposit_collateral(&operator, &1_000);
    client.slash(&admin, &operator, &Offence::RouterBypass);

    client.deposit_collateral(&operator, &9_000);
    assert_eq!(client.collateral_of(&operator), 9_000);
    assert_eq!(
        client.try_slash(&admin, &operator, &Offence::RouterBypass),
        Err(Ok(Error::AlreadySlashed))
    );
    assert_eq!(client.total_slashed(), 1_000);
    assert_eq!(client.compensation_pool(), 500);
}

#[test]
fn distinct_offences_are_punished_independently() {
    let (_e, admin, operator, client) = setup(1_000);
    client.deposit_collateral(&operator, &1_000);
    client.slash(&admin, &operator, &Offence::SpoofedGeneration);

    client.deposit_collateral(&operator, &2_000);
    let r = client.slash(&admin, &operator, &Offence::RouterBypass);
    assert_eq!(r.seized, 2_000);
    assert!(client.was_slashed(&operator, &Offence::SpoofedGeneration));
    assert!(client.was_slashed(&operator, &Offence::RouterBypass));
    assert_eq!(client.total_slashed(), 3_000);
}

#[test]
fn appeal_window_blocks_slash_until_resolved() {
    let (_e, admin, operator, client) = setup(1_000);
    client.deposit_collateral(&operator, &1_000);
    client.appeal_window(&admin, &operator, &Offence::SpoofedGeneration);

    assert!(client.is_appealed(&operator, &Offence::SpoofedGeneration));
    assert!(!client.is_appealed(&operator, &Offence::RouterBypass));
    assert_eq!(
        client.try_slash(&admin, &operator, &Offence::SpoofedGeneration),
        Err(Ok(Error::UnderAppeal))
    );
    assert_eq!(client.total_slashed(), 0);

    client.resolve_appeal(&admin, &operator, &Offence::SpoofedGeneration);
    assert!(!client.is_appealed(&operator, &Offence::SpoofedGeneration));
    let r = client.slash(&admin, &operator, &Offence::SpoofedGeneration);
    assert_eq!(r.seized, 1_000);
}

#[test]
fn resolving_an_appeal_that_was_never_raised_is_rejected() {
    let (_e, admin, operator, client) = setup(1_000);
    assert_eq!(
        client.try_resolve_appeal(&admin, &operator, &Offence::RouterBypass),
        Err(Ok(Error::NoAppeal))
    );
}

#[test]
fn non_admin_cannot_slash_or_appeal() {
    let (e, _admin, operator, client) = setup(1_000);
    let impostor = Address::generate(&e);
    client.deposit_collateral(&operator, &1_000);

    assert_eq!(
        client.try_slash(&impostor, &operator, &Offence::SpoofedGeneration),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(
        client.try_appeal_window(&impostor, &operator, &Offence::RouterBypass),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(client.collateral_of(&operator), 1_000);
    assert_eq!(client.total_slashed(), 0);
}

#[test]
fn pool_plus_burned_equals_total_seized_across_operators() {
    let (e, admin, _op, client) = setup(3_000);
    let a = Address::generate(&e);
    let b = Address::generate(&e);
    let c = Address::generate(&e);
    let mut seized_total: i128 = 0;

    client.deposit_collateral(&a, &10_000);
    client.deposit_collateral(&b, &5_000);
    client.deposit_collateral(&c, &7_500);
    assert_eq!(client.total_deposited(), 22_500);

    let ra = client.slash(&admin, &a, &Offence::SpoofedGeneration);
    seized_total += ra.seized;
    assert_eq!(client.compensation_pool() + client.total_burned(), seized_total);
    assert_eq!(ra.burned + ra.redistributed, ra.seized);

    let rb = client.slash(&admin, &b, &Offence::RouterBypass);
    seized_total += rb.seized;
    assert_eq!(client.compensation_pool() + client.total_burned(), seized_total);

    let rc = client.slash(&admin, &c, &Offence::SpoofedGeneration);
    seized_total += rc.seized;
    assert_eq!(client.compensation_pool() + client.total_burned(), seized_total);

    assert_eq!(client.total_slashed(), seized_total);
    assert_eq!(client.total_burned(), 6_750);
    assert_eq!(client.compensation_pool(), 15_750);
}

#[test]
fn collateral_never_goes_negative_across_many_slashes() {
    let (e, admin, _op, client) = setup(1_500);
    let owners: [Address; 3] = [
        Address::generate(&e),
        Address::generate(&e),
        Address::generate(&e),
    ];
    let offences = [Offence::SpoofedGeneration, Offence::RouterBypass];

    for (i, owner) in owners.iter().enumerate() {
        client.deposit_collateral(owner, &(1_000 * (i as i128 + 1)));
    }
    for owner in owners.iter() {
        for offence in offences.iter() {
            client.slash(&admin, owner, offence);
            assert!(client.collateral_of(owner) == 0);
            client.deposit_collateral(owner, &500);
            assert!(client.collateral_of(owner) >= 0);
        }
    }

    assert_eq!(client.total_slashed(), 7_500);
    assert_eq!(client.total_burned(), 1_125);
    assert_eq!(client.compensation_pool(), 6_375);
    assert_eq!(client.compensation_pool() + client.total_burned(), 7_500);
    for owner in owners.iter() {
        assert_eq!(client.collateral_of(owner), 500);
    }
}