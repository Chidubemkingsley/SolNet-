use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};
use solnet_firmware_verifier::{Error, FirmwareVerifier, FirmwareVerifierClient, Verdict};

fn setup() -> (Env, Address, FirmwareVerifierClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(FirmwareVerifier, ());
    let client = FirmwareVerifierClient::new(&e, &id);
    client.init(&admin);
    (e, admin, client)
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

#[test]
fn init_stores_admin_and_empty_high_water_marks() {
    let (_e, admin, client) = setup();
    assert_eq!(client.latest_version(), 0);
    assert_eq!(client.floor_version(), 0);
    assert_eq!(
        client.try_init(&admin),
        Err(Ok(Error::AlreadyInitialized))
    );
}

#[test]
fn unknown_hash_is_unapproved_and_unreadable() {
    let (e, _admin, client) = setup();
    let h = hash(&e, 0xAA);
    let device = hash(&e, 0x01);
    assert!(!client.is_approved(&h));
    assert_eq!(client.try_approved_version(&h), Err(Ok(Error::NotApproved)));
    assert_eq!(client.verdict_of(&h), Verdict::Unapproved);
    assert_eq!(
        client.verify_device(&device, &h).verdict,
        Verdict::Unapproved
    );
    assert_eq!(client.stale_report_count(&h), 0);
}

#[test]
fn approve_records_version_and_verdict_is_approved() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x11);
    client.approve_firmware(&admin, &h, &3, &1);

    assert!(client.is_approved(&h));
    assert_eq!(client.approved_version(&h), 3);
    assert_eq!(client.latest_version(), 3);
    assert_eq!(
        client.verify_device(&hash(&e, 0x09), &h).verdict,
        Verdict::Approved
    );
}

#[test]
fn verify_device_echoes_device_key_and_applied_floor() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x22);
    let device = hash(&e, 0x77);
    client.approve_firmware(&admin, &h, &5, &2);

    let v = client.verify_device(&device, &h);
    assert_eq!(v.device_key, device);
    assert_eq!(v.firmware_hash, h);
    assert_eq!(v.version, 5);
    assert_eq!(v.floor_version, 2);
    assert_eq!(v.verdict, Verdict::Approved);
}

#[test]
fn zero_version_is_rejected() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x31);
    assert_eq!(
        client.try_approve_firmware(&admin, &h, &0, &0),
        Err(Ok(Error::InvalidVersion))
    );
    assert!(!client.is_approved(&h));
    assert_eq!(client.latest_version(), 0);
}

#[test]
fn min_version_above_version_is_rejected() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x32);
    assert_eq!(
        client.try_approve_firmware(&admin, &h, &2, &3),
        Err(Ok(Error::InvalidVersion))
    );
    assert!(!client.is_approved(&h));
    assert_eq!(client.floor_version(), 0);
}

#[test]
fn equal_min_and_version_is_the_accepted_boundary() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x33);
    client.approve_firmware(&admin, &h, &4, &4);
    assert_eq!(client.approved_version(&h), 4);
    assert_eq!(
        client.verify_device(&hash(&e, 0x01), &h).verdict,
        Verdict::Approved
    );
}

#[test]
fn conflicting_reapproval_is_rejected_but_identical_one_is_idempotent() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x41);
    client.approve_firmware(&admin, &h, &7, &3);
    client.approve_firmware(&admin, &h, &7, &3);
    assert_eq!(client.approved_version(&h), 7);
    assert_eq!(
        client.try_approve_firmware(&admin, &h, &8, &3),
        Err(Ok(Error::ConflictingApproval))
    );
    assert_eq!(client.approved_version(&h), 7);
}

#[test]
fn revoke_flips_verdict_and_unknown_revoke_fails() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x51);
    assert_eq!(
        client.try_revoke_firmware(&admin, &h),
        Err(Ok(Error::NotApproved))
    );

    client.approve_firmware(&admin, &h, &2, &1);
    client.revoke_firmware(&admin, &h);
    assert!(!client.is_approved(&h));
    assert_eq!(client.verdict_of(&h), Verdict::Revoked);
    assert_eq!(client.try_approved_version(&h), Err(Ok(Error::NotApproved)));
    assert_eq!(
        client.try_revoke_firmware(&admin, &h),
        Err(Ok(Error::NotApproved))
    );
}

#[test]
fn approve_revoke_reapprove_moves_through_all_three_states() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x61);

    client.approve_firmware(&admin, &h, &1, &1);
    assert!(client.is_approved(&h));
    assert_eq!(client.verdict_of(&h), Verdict::Approved);

    client.revoke_firmware(&admin, &h);
    assert!(!client.is_approved(&h));
    assert_eq!(client.verdict_of(&h), Verdict::Revoked);

    client.approve_firmware(&admin, &h, &2, &1);
    assert!(client.is_approved(&h));
    assert_eq!(client.approved_version(&h), 2);
    assert_eq!(client.verdict_of(&h), Verdict::Approved);
}

#[test]
fn raising_the_floor_turns_older_builds_stale() {
    let (e, admin, client) = setup();
    let old = hash(&e, 0x71);
    let new = hash(&e, 0x72);

    client.approve_firmware(&admin, &old, &1, &1);
    assert_eq!(client.verdict_of(&old), Verdict::Approved);

    client.approve_firmware(&admin, &new, &2, &2);
    assert_eq!(client.floor_version(), 2);
    assert_eq!(client.latest_version(), 2);
    assert_eq!(client.verdict_of(&old), Verdict::Stale);
    assert_eq!(client.verdict_of(&new), Verdict::Approved);
    assert_eq!(client.verify_device(&hash(&e, 0x05), &old).version, 1);
}

#[test]
fn latest_version_never_regresses_on_older_approval() {
    let (e, admin, client) = setup();
    let high = hash(&e, 0x81);
    let low = hash(&e, 0x82);
    client.approve_firmware(&admin, &high, &10, &1);
    client.approve_firmware(&admin, &low, &3, &1);
    assert_eq!(client.latest_version(), 10);
    assert_eq!(client.floor_version(), 1);
}

#[test]
fn report_device_stores_version_and_rejects_zero() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0x91);
    let device = hash(&e, 0x05);
    client.approve_firmware(&admin, &h, &3, &1);

    assert_eq!(
        client.try_report_device(&admin, &device, &h, &0),
        Err(Ok(Error::InvalidCurrentVersion))
    );
    assert_eq!(
        client.try_device_version(&device),
        Err(Ok(Error::UnknownDevice))
    );

    client.report_device(&admin, &device, &h, &3);
    assert_eq!(client.device_version(&device), 3);
    let mut payload = Bytes::new(&e);
    payload.append(&Bytes::from(device.clone()));
    payload.append(&Bytes::from(h));
    payload.append(&Bytes::from_array(&e, &3u32.to_be_bytes()));
    assert_eq!(
        client.report_digest(&device),
        e.crypto().sha256(&payload).to_bytes()
    );
}

#[test]
fn stale_reports_are_counted_per_firmware_hash() {
    let (e, admin, client) = setup();
    let stale = hash(&e, 0xA1);
    let fresh = hash(&e, 0xA2);
    client.approve_firmware(&admin, &stale, &1, &1);
    client.approve_firmware(&admin, &fresh, &2, &2);

    client.report_device(&admin, &hash(&e, 0x01), &stale, &1);
    client.report_device(&admin, &hash(&e, 0x02), &stale, &1);
    client.report_device(&admin, &hash(&e, 0x03), &fresh, &2);

    assert_eq!(client.stale_report_count(&stale), 2);
    assert_eq!(client.stale_report_count(&fresh), 0);
    assert_eq!(client.device_version(&hash(&e, 0x02)), 1);
}

#[test]
fn non_admin_cannot_mutate_registry_but_admin_still_can() {
    let (e, admin, client) = setup();
    let h = hash(&e, 0xB1);
    let device = hash(&e, 0x06);
    let impostor = Address::generate(&e);

    assert_eq!(
        client.try_approve_firmware(&impostor, &h, &1, &1),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(
        client.try_revoke_firmware(&impostor, &h),
        Err(Ok(Error::NotAdmin))
    );
    assert_eq!(
        client.try_report_device(&impostor, &device, &h, &1),
        Err(Ok(Error::NotAdmin))
    );

    client.approve_firmware(&admin, &h, &1, &1);
    assert_eq!(client.latest_version(), 1);
}