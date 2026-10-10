use solnet_hardware_slash::{Error, HardwareSlash, HardwareSlashClient, Offence};
use soroban_sdk::{
    symbol_short,
    testutils::{
        storage::{Instance as _, Persistent as _},
        Address as _, Ledger as _,
    },
    Address, Env,
};

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

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect contract storage.
fn setup_tracked(
    burn_bps: i128,
) -> (Env, Address, Address, Address, HardwareSlashClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let operator = Address::generate(&e);
    let id = e.register(HardwareSlash, ());
    let client = HardwareSlashClient::new(&e, &id);
    client.init(&admin, &burn_bps);
    (e, admin, operator, id, client)
}

fn network_max_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().max_ttl())
}

fn collateral_ttl(e: &Env, id: &Address, owner: &Address) -> u32 {
    e.as_contract(id, || {
        e.storage()
            .persistent()
            .get_ttl(&(symbol_short!("col"), owner.clone()))
    })
}

fn appeal_ttl(e: &Env, id: &Address, owner: &Address, offence: Offence) -> u32 {
    let code: u32 = match offence {
        Offence::SpoofedGeneration => 1,
        Offence::RouterBypass => 2,
    };
    e.as_contract(id, || {
        e.storage()
            .persistent()
            .get_ttl(&(symbol_short!("ap"), owner.clone(), code))
    })
}

fn instance_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().instance().get_ttl())
}

/// Writes a collateral balance straight past the contract, with no renewal
/// behind it, so the assertions below can tell a bumped entry apart from one
/// that merely happens to exist.
fn seed_unrenewed_collateral(e: &Env, id: &Address, owner: &Address, value: i128) {
    e.as_contract(id, || {
        e.storage()
            .persistent()
            .set(&(symbol_short!("col"), owner.clone()), &value)
    });
}

/// Same contrast as `seed_unrenewed_collateral`, for the three-part appeal
/// freeze keys, which are keyed by operator and offence code rather than by
/// operator alone.
fn seed_unrenewed_appeal(e: &Env, id: &Address, owner: &Address, offence: Offence) {
    let code: u32 = match offence {
        Offence::SpoofedGeneration => 1,
        Offence::RouterBypass => 2,
    };
    e.as_contract(id, || {
        e.storage()
            .persistent()
            .set(&(symbol_short!("ap"), owner.clone(), code), &true)
    });
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
#[test]
fn depositing_collateral_renews_the_lifetime_of_the_collateral_entry() {
    let (e, _admin, operator, id, client) = setup_tracked(1_000);
    let bystander = Address::generate(&e);
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);
    let floor = e.ledger().get().min_persistent_entry_ttl;

    seed_unrenewed_collateral(&e, &id, &bystander, 9);
    assert!(collateral_ttl(&e, &id, &bystander) <= floor);

    client.deposit_collateral(&operator, &1_000);
    let renewed = collateral_ttl(&e, &id, &operator);
    assert_eq!(renewed, max);
    assert!(renewed > floor);
    assert!(collateral_ttl(&e, &id, &bystander) <= floor);

    client.deposit_collateral(&operator, &500);
    assert_eq!(client.collateral_of(&operator), 1_500);
    assert_eq!(collateral_ttl(&e, &id, &operator), max);
}

#[test]
fn slashing_renews_the_lifetime_of_the_collateral_and_marker_entries() {
    let (e, admin, operator, id, client) = setup_tracked(2_500);
    client.deposit_collateral(&operator, &1_000);
    let max = network_max_ttl(&e, &id);

    client.slash(&admin, &operator, &Offence::SpoofedGeneration);
    assert!(client.was_slashed(&operator, &Offence::SpoofedGeneration));
    let renewed = collateral_ttl(&e, &id, &operator);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);

    let marker = e.as_contract(&id, || {
        e.storage()
            .persistent()
            .get_ttl(&(symbol_short!("sl"), operator.clone(), 1u32))
    });
    assert_eq!(marker, max);
    assert!(marker > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn raising_and_lifting_an_appeal_freeze_renews_the_lifetime_of_the_freeze_entry() {
    let (e, admin, operator, id, client) = setup_tracked(1_000);
    client.deposit_collateral(&operator, &1_000);
    let other = Address::generate(&e);
    let max = network_max_ttl(&e, &id);
    let floor = e.ledger().get().min_persistent_entry_ttl;

    client.appeal_window(&admin, &operator, &Offence::RouterBypass);
    let frozen = appeal_ttl(&e, &id, &operator, Offence::RouterBypass);
    assert_eq!(frozen, max);
    assert!(frozen > floor);

    seed_unrenewed_appeal(&e, &id, &operator, Offence::SpoofedGeneration);
    assert!(appeal_ttl(&e, &id, &operator, Offence::SpoofedGeneration) <= floor);

    client.resolve_appeal(&admin, &operator, &Offence::RouterBypass);
    assert!(!client.is_appealed(&operator, &Offence::RouterBypass));
    let lifted = appeal_ttl(&e, &id, &operator, Offence::RouterBypass);
    assert_eq!(lifted, max);
    assert!(lifted > floor);

    client.appeal_window(&admin, &other, &Offence::RouterBypass);
    assert_eq!(appeal_ttl(&e, &id, &other, Offence::RouterBypass), max);
}

#[test]
fn every_write_path_renews_the_lifetime_of_the_contract_instance() {
    let (e, admin, operator, id, client) = setup_tracked(1_000);
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);
    assert_eq!(instance_ttl(&e, &id), max);

    client.deposit_collateral(&operator, &1_000);
    assert_eq!(instance_ttl(&e, &id), max);

    client.slash(&admin, &operator, &Offence::SpoofedGeneration);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn a_deposit_restores_a_decayed_contract_instance() {
    let (e, _admin, operator, id, client) = setup_tracked(1_000);
    let max = network_max_ttl(&e, &id);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);
    assert_eq!(client.total_deposited(), 0);

    client.deposit_collateral(&operator, &1_000);
    assert_eq!(client.total_deposited(), 1_000);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
}
