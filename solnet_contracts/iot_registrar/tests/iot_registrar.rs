use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String};
use solnet_iot_registrar::{Device, Error, IotRegistrar, IotRegistrarClient};

fn setup() -> (Env, Address, IotRegistrarClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(IotRegistrar, ());
    let client = IotRegistrarClient::new(&e, &id);
    client.init(&admin);
    (e, admin, client)
}

fn key(e: &Env, seed: u32) -> BytesN<32> {
    BytesN::from_array(e, &[seed as u8; 32])
}

fn model(e: &Env, text: &str) -> String {
    String::from_str(e, text)
}

fn device(e: &Env, text: &str, at: u64) -> Device {
    Device {
        model: model(e, text),
        registered_at: at,
    }
}

#[test]
fn init_stores_admin_and_is_not_repeatable() {
    let (e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.registered_count(), 0);
    assert_eq!(client.try_init(&admin), Err(Ok(Error::AlreadyInitialized)));
    let other = Address::generate(&e);
    assert_eq!(client.try_init(&other), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn register_device_stores_model_and_sequence() {
    use soroban_sdk::testutils::Ledger as _;

    let (e, _admin, client) = setup();
    e.ledger().set_sequence_number(41);

    client.register_device(&key(&e, 1), &model(&e, "SunVault-X1"));
    assert_eq!(client.device(&key(&e, 1)), device(&e, "SunVault-X1", 41));
    assert!(client.is_registered(&key(&e, 1)));
    assert_eq!(client.registered_count(), 1);
}

#[test]
fn register_device_rejects_empty_model() {
    let (e, _admin, client) = setup();
    assert_eq!(
        client.try_register_device(&key(&e, 1), &model(&e, "")),
        Err(Ok(Error::EmptyModel))
    );
    assert!(!client.is_registered(&key(&e, 1)));
    assert_eq!(client.registered_count(), 0);
}

#[test]
fn register_device_rejects_duplicate_key() {
    let (e, _admin, client) = setup();
    client.register_device(&key(&e, 7), &model(&e, "GridNode-A"));

    assert_eq!(
        client.try_register_device(&key(&e, 7), &model(&e, "GridNode-B")),
        Err(Ok(Error::AlreadyRegistered))
    );
    assert_eq!(client.device(&key(&e, 7)).model, model(&e, "GridNode-A"));
    assert_eq!(client.version(&key(&e, 7)), 1);
    assert_eq!(client.registered_count(), 1);
}

#[test]
fn register_device_requires_admin_auth() {
    let (e, _admin, client) = setup();
    e.mock_auths(&[]);
    assert!(client
        .try_register_device(&key(&e, 3), &model(&e, "Meter"))
        .is_err());
    assert_eq!(client.registered_count(), 0);
}

#[test]
fn register_device_tracks_independent_keys() {
    let (e, _admin, client) = setup();
    for seed in 1..=4u32 {
        client.register_device(&key(&e, seed), &model(&e, "Meter"));
    }
    assert_eq!(client.registered_count(), 4);
    for seed in 1..=4u32 {
        assert!(client.is_registered(&key(&e, seed)));
        assert_eq!(client.version(&key(&e, seed)), 1);
    }
    assert!(!client.is_registered(&key(&e, 5)));
    assert_eq!(client.version(&key(&e, 5)), 0);
}

#[test]
fn unknown_key_lookup_fails() {
    let (e, _admin, client) = setup();
    assert!(!client.is_registered(&key(&e, 9)));
    assert_eq!(
        client.try_device(&key(&e, 9)),
        Err(Ok(Error::DeviceNotFound))
    );
    assert_eq!(client.version(&key(&e, 9)), 0);
    assert_eq!(
        client.try_revocation_reason(&key(&e, 9)),
        Err(Ok(Error::NoRevocation))
    );
}

#[test]
fn revoke_clears_registration_and_count() {
    let (e, admin, client) = setup();
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    client.register_device(&key(&e, 2), &model(&e, "Meter-B"));
    assert_eq!(client.registered_count(), 2);

    client.revoke(&admin, &key(&e, 1), &model(&e, "tamper"));
    assert!(!client.is_registered(&key(&e, 1)));
    assert_eq!(
        client.try_device(&key(&e, 1)),
        Err(Ok(Error::DeviceNotFound))
    );
    assert_eq!(client.registered_count(), 1);
    assert!(client.is_registered(&key(&e, 2)));
}

#[test]
fn revoke_rejects_unknown_key() {
    let (e, admin, client) = setup();
    assert_eq!(
        client.try_revoke(&admin, &key(&e, 4), &model(&e, "ghost")),
        Err(Ok(Error::DeviceNotFound))
    );
    client.register_device(&key(&e, 4), &model(&e, "Meter-C"));
    client.revoke(&admin, &key(&e, 4), &model(&e, "tamper"));
    assert_eq!(
        client.try_revoke(&admin, &key(&e, 4), &model(&e, "again")),
        Err(Ok(Error::DeviceNotFound))
    );
    assert_eq!(client.registered_count(), 0);
}

#[test]
fn revoke_requires_admin_auth() {
    let (e, admin, client) = setup();
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_revoke(&impostor, &key(&e, 1), &model(&e, "nope")),
        Err(Ok(Error::NotAdmin))
    );
    assert!(client.is_registered(&key(&e, 1)));

    e.mock_auths(&[]);
    assert!(client
        .try_revoke(&admin, &key(&e, 1), &model(&e, "nope"))
        .is_err());
}

#[test]
fn revoke_records_reason_for_audit() {
    let (e, admin, client) = setup();
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    assert_eq!(
        client.try_revocation_reason(&key(&e, 1)),
        Err(Ok(Error::NoRevocation))
    );

    client.revoke(&admin, &key(&e, 1), &model(&e, "relay-tamper"));
    assert_eq!(
        client.revocation_reason(&key(&e, 1)),
        model(&e, "relay-tamper")
    );
}

#[test]
fn version_increments_on_re_registration() {
    let (e, admin, client) = setup();
    assert_eq!(client.version(&key(&e, 1)), 0);

    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    assert_eq!(client.version(&key(&e, 1)), 1);

    client.revoke(&admin, &key(&e, 1), &model(&e, "warranty"));
    assert_eq!(client.version(&key(&e, 1)), 1);

    client.register_device(&key(&e, 1), &model(&e, "Meter-A2"));
    assert_eq!(client.version(&key(&e, 1)), 2);
    assert!(client.is_registered(&key(&e, 1)));
    assert_eq!(client.registered_count(), 1);

    client.revoke(&admin, &key(&e, 1), &model(&e, "warranty"));
    client.register_device(&key(&e, 1), &model(&e, "Meter-A3"));
    assert_eq!(client.version(&key(&e, 1)), 3);
}

#[test]
fn re_registered_key_gets_fresh_registration_sequence() {
    use soroban_sdk::testutils::Ledger as _;

    let (e, admin, client) = setup();
    e.ledger().set_sequence_number(10);
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    client.revoke(&admin, &key(&e, 1), &model(&e, "swap"));

    e.ledger().set_sequence_number(90);
    client.register_device(&key(&e, 1), &model(&e, "Meter-A2"));
    assert_eq!(client.device(&key(&e, 1)), device(&e, "Meter-A2", 90));
    assert_eq!(client.version(&key(&e, 1)), 2);
}

#[test]
fn transfer_moves_registration_and_bumps_version() {
    use soroban_sdk::testutils::Ledger as _;

    let (e, admin, client) = setup();
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    client.register_device(&key(&e, 2), &model(&e, "Meter-B"));
    client.revoke(&admin, &key(&e, 2), &model(&e, "recalled"));
    e.ledger().set_sequence_number(55);

    client.transfer(&admin, &key(&e, 1), &key(&e, 2));
    assert!(!client.is_registered(&key(&e, 1)));
    assert!(client.is_registered(&key(&e, 2)));
    assert_eq!(client.device(&key(&e, 2)), device(&e, "Meter-A", 55));
    assert_eq!(client.version(&key(&e, 2)), 2);
    assert_eq!(client.version(&key(&e, 1)), 1);
    assert_eq!(client.registered_count(), 1);
}

#[test]
fn transfer_rejects_invalid_endpoints_and_self_transfer() {
    let (e, admin, client) = setup();
    client.register_device(&key(&e, 1), &model(&e, "Meter-A"));
    client.register_device(&key(&e, 2), &model(&e, "Meter-B"));

    assert_eq!(
        client.try_transfer(&admin, &key(&e, 1), &key(&e, 1)),
        Err(Ok(Error::SelfTransfer))
    );
    assert_eq!(
        client.try_transfer(&admin, &key(&e, 1), &key(&e, 2)),
        Err(Ok(Error::AlreadyRegistered))
    );
    assert_eq!(
        client.try_transfer(&admin, &key(&e, 3), &key(&e, 1)),
        Err(Ok(Error::DeviceNotFound))
    );

    let impostor = Address::generate(&e);
    assert_eq!(
        client.try_transfer(&impostor, &key(&e, 1), &key(&e, 3)),
        Err(Ok(Error::NotAdmin))
    );

    e.mock_auths(&[]);
    assert!(client
        .try_transfer(&admin, &key(&e, 1), &key(&e, 3))
        .is_err());
    assert!(client.is_registered(&key(&e, 1)));
}