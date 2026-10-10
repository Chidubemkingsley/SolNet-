use solnet_cryptographic_oracle::{
    BatchRecord, CryptographicOracle, CryptographicOracleClient, Error, TelemetryEntry,
};
use soroban_sdk::{
    symbol_short,
    testutils::{
        storage::{Instance as _, Persistent as _},
        Address as _, Ledger as _,
    },
    vec, Address, BytesN, Env, Symbol, Vec,
};

fn setup() -> (Env, Address, CryptographicOracleClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(CryptographicOracle, ());
    let client = CryptographicOracleClient::new(&e, &id);
    client.init(&admin);
    (e, admin, client)
}

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect contract storage.
fn setup_tracked() -> (Env, Address, Address, CryptographicOracleClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(CryptographicOracle, ());
    let client = CryptographicOracleClient::new(&e, &id);
    client.init(&admin);
    (e, admin, id, client)
}

fn network_max_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().max_ttl())
}

fn entry_ttl(e: &Env, id: &Address, key: &(Symbol, BytesN<32>)) -> u32 {
    e.as_contract(id, || e.storage().persistent().get_ttl(key))
}

fn instance_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().instance().get_ttl())
}

/// Writes a batch record straight past the contract, with no renewal behind
/// it, so the assertions below can tell a bumped entry apart from one that
/// merely happens to exist.
fn seed_unrenewed(e: &Env, id: &Address, key: &(Symbol, BytesN<32>), value: &BatchRecord) {
    e.as_contract(id, || e.storage().persistent().set(key, value));
}

fn key(e: &Env, seed: u32) -> BytesN<32> {
    BytesN::from_array(e, &[seed as u8; 32])
}

fn entries(e: &Env) -> Vec<TelemetryEntry> {
    vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 1_500,
            recorded_at: 100,
        },
        TelemetryEntry {
            node_id: 1,
            milli_wh: 2_500,
            recorded_at: 200,
        },
        TelemetryEntry {
            node_id: 2,
            milli_wh: 3_000,
            recorded_at: 300,
        },
    ]
}

fn record(submitter: &Address, count: u32, total: i128, settled: bool) -> BatchRecord {
    BatchRecord {
        entry_count: count,
        total_mwh: total,
        submitter: submitter.clone(),
        settled,
    }
}

#[test]
fn init_stores_admin_and_is_not_repeatable() {
    let (e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.batch_count(), 0);
    assert_eq!(client.try_init(&admin), Err(Ok(Error::AlreadyInitialized)));
    let other = Address::generate(&e);
    assert_eq!(client.try_init(&other), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn submit_batch_stores_record_and_counts() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);

    client.submit_batch(&meter, &batch, &samples, &digest);

    assert_eq!(client.batch_count(), 1);
    assert_eq!(client.batch(&batch), record(&meter, 3, 7_000, false));
    assert!(client.exists(&batch));
    assert!(!client.is_settled(&batch));
}

#[test]
fn submit_batch_rejects_empty_entries() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let none = Vec::new(&e);
    let digest = client.compute_digest(&none);

    assert_eq!(
        client.try_submit_batch(&meter, &batch, &none, &digest),
        Err(Ok(Error::EmptyBatch))
    );
    assert_eq!(client.batch_count(), 0);
    assert!(!client.exists(&batch));
}

#[test]
fn submit_batch_rejects_replayed_batch_id() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);

    client.submit_batch(&meter, &batch, &samples, &digest);
    assert_eq!(
        client.try_submit_batch(&meter, &batch, &samples, &digest),
        Err(Ok(Error::BatchReplay))
    );
    assert_eq!(client.batch_count(), 1);

    let attacker = Address::generate(&e);
    let inflated = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 9_999_999,
            recorded_at: 100,
        },
    ];
    let forged = client.compute_digest(&inflated);
    assert_eq!(
        client.try_submit_batch(&attacker, &batch, &inflated, &forged),
        Err(Ok(Error::BatchReplay))
    );
    assert_eq!(client.batch(&batch).total_mwh, 7_000);
}

#[test]
fn submit_batch_rejects_digest_mismatch() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);

    assert_eq!(
        client.try_submit_batch(&meter, &batch, &samples, &key(&e, 99)),
        Err(Ok(Error::DigestMismatch))
    );
    assert_eq!(client.batch_count(), 0);
    assert!(!client.exists(&batch));
}

#[test]
fn submit_batch_detects_tampered_entry() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);

    let tampered = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 1_500,
            recorded_at: 100,
        },
        TelemetryEntry {
            node_id: 1,
            milli_wh: 2_500,
            recorded_at: 200,
        },
        TelemetryEntry {
            node_id: 2,
            milli_wh: 999_999,
            recorded_at: 300,
        },
    ];

    assert_eq!(
        client.try_submit_batch(&meter, &key(&e, 1), &tampered, &digest),
        Err(Ok(Error::DigestMismatch))
    );
    assert_eq!(
        client.try_submit_batch(&meter, &key(&e, 2), &tampered, &digest),
        Err(Ok(Error::DigestMismatch))
    );
    assert_eq!(client.batch_count(), 0);
}

#[test]
fn submit_batch_rejects_negative_reading() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let samples = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 500,
            recorded_at: 100,
        },
        TelemetryEntry {
            node_id: 1,
            milli_wh: -1,
            recorded_at: 200,
        },
    ];
    let digest = client.compute_digest(&samples);

    assert_eq!(
        client.try_submit_batch(&meter, &key(&e, 1), &samples, &digest),
        Err(Ok(Error::InvalidReading))
    );
    assert_eq!(client.batch_count(), 0);
}

#[test]
fn submit_batch_requires_submitter_auth() {
    let (e, _admin, client) = setup();
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);
    let impostor = Address::generate(&e);
    e.mock_auths(&[]);

    assert!(client
        .try_submit_batch(&impostor, &key(&e, 1), &samples, &digest)
        .is_err());
    assert_eq!(client.batch_count(), 0);
}

#[test]
fn total_mwh_sums_entries() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);
    client.submit_batch(&meter, &batch, &samples, &digest);

    assert_eq!(client.total_mwh(&batch), 7_000);
    assert_eq!(client.total_mwh(&key(&e, 77)), 0);

    let zeroed = vec![
        &e,
        TelemetryEntry {
            node_id: 9,
            milli_wh: 0,
            recorded_at: 1,
        },
    ];
    let zero_digest = client.compute_digest(&zeroed);
    client.submit_batch(&meter, &key(&e, 2), &zeroed, &zero_digest);
    assert_eq!(client.total_mwh(&key(&e, 2)), 0);
    assert_eq!(client.batch(&key(&e, 2)).entry_count, 1);
}

#[test]
fn total_mwh_rejects_overflowing_batch() {
    let (e, _admin, client) = setup();
    let meter = Address::generate(&e);
    let samples = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: i128::MAX,
            recorded_at: 100,
        },
        TelemetryEntry {
            node_id: 1,
            milli_wh: 1,
            recorded_at: 200,
        },
    ];
    let digest = client.compute_digest(&samples);

    assert_eq!(
        client.try_submit_batch(&meter, &key(&e, 1), &samples, &digest),
        Err(Ok(Error::ArithmeticOverflow))
    );
    assert_eq!(client.batch_count(), 0);
}

#[test]
fn settle_marks_batch_settled() {
    let (e, admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);
    client.submit_batch(&meter, &batch, &samples, &digest);

    assert!(!client.is_settled(&batch));
    client.settle(&admin, &batch);
    assert!(client.is_settled(&batch));
    assert_eq!(client.batch(&batch), record(&meter, 3, 7_000, true));
}

#[test]
fn settle_rejects_unknown_batch() {
    let (e, admin, client) = setup();
    assert!(!client.is_settled(&key(&e, 5)));
    assert_eq!(client.total_mwh(&key(&e, 5)), 0);
    assert!(!client.exists(&key(&e, 5)));
    assert_eq!(
        client.try_batch(&key(&e, 5)),
        Err(Ok(Error::BatchNotFound))
    );
    assert_eq!(
        client.try_settle(&admin, &key(&e, 5)),
        Err(Ok(Error::BatchNotFound))
    );
}

#[test]
fn settle_rejects_double_settlement() {
    let (e, admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);
    client.submit_batch(&meter, &batch, &samples, &digest);

    client.settle(&admin, &batch);
    assert_eq!(
        client.try_settle(&admin, &batch),
        Err(Ok(Error::AlreadySettled))
    );
    assert!(client.is_settled(&batch));
    assert_eq!(client.total_mwh(&batch), 7_000);
}

#[test]
fn settle_requires_admin_auth() {
    let (e, admin, client) = setup();
    let meter = Address::generate(&e);
    let batch = key(&e, 1);
    let samples = entries(&e);
    let digest = client.compute_digest(&samples);
    client.submit_batch(&meter, &batch, &samples, &digest);

    let impostor = Address::generate(&e);
    assert_eq!(
        client.try_settle(&impostor, &batch),
        Err(Ok(Error::NotAdmin))
    );
    assert!(!client.is_settled(&batch));

    e.mock_auths(&[]);
    assert!(client.try_settle(&admin, &batch).is_err());
    assert!(!client.is_settled(&batch));
}

#[test]
fn digest_is_order_sensitive_and_domain_separated() {
    let (e, _admin, client) = setup();

    let a = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 10,
            recorded_at: 20,
        },
    ];
    let b = vec![
        &e,
        TelemetryEntry {
            node_id: 2,
            milli_wh: 30,
            recorded_at: 40,
        },
    ];
    let ab = vec![
        &e,
        TelemetryEntry {
            node_id: 1,
            milli_wh: 10,
            recorded_at: 20,
        },
        TelemetryEntry {
            node_id: 2,
            milli_wh: 30,
            recorded_at: 40,
        },
    ];
    let ba = vec![
        &e,
        TelemetryEntry {
            node_id: 2,
            milli_wh: 30,
            recorded_at: 40,
        },
        TelemetryEntry {
            node_id: 1,
            milli_wh: 10,
            recorded_at: 20,
        },
    ];

    let d_a = client.compute_digest(&a);
    let d_b = client.compute_digest(&b);
    let d_ab = client.compute_digest(&ab);
    let d_ba = client.compute_digest(&ba);

    assert_ne!(d_a, d_b);
    assert_ne!(d_ab, d_ba);
    assert_ne!(d_ab, d_a);
    assert_ne!(d_ab, d_b);
    assert_eq!(d_ab, client.compute_digest(&ab));
    assert_ne!(d_a, key(&e, 0));
}
#[test]
fn submitting_a_batch_renews_the_lifetime_of_the_batch_record() {
    let (e, _admin, id, client) = setup_tracked();
    let batch = key(&e, 11);
    let untouched = key(&e, 12);
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);
    let floor = e.ledger().get().min_persistent_entry_ttl;

    seed_unrenewed(&e, &id, &(symbol_short!("bat"), untouched.clone()), &record(&_admin, 1, 1, false));
    assert!(entry_ttl(&e, &id, &(symbol_short!("bat"), untouched.clone())) <= floor);

    client.submit_batch(&_admin, &batch, &entries(&e), &client.compute_digest(&entries(&e)));
    let renewed = entry_ttl(&e, &id, &(symbol_short!("bat"), batch.clone()));
    assert_eq!(renewed, max);
    assert!(renewed > floor);
    assert!(entry_ttl(&e, &id, &(symbol_short!("bat"), untouched)) <= floor);
}

#[test]
fn settling_a_batch_renews_the_lifetime_of_the_batch_record() {
    let (e, admin, id, client) = setup_tracked();
    let batch = key(&e, 21);
    client.submit_batch(&admin, &batch, &entries(&e), &client.compute_digest(&entries(&e)));
    let max = network_max_ttl(&e, &id);

    client.settle(&admin, &batch);
    assert!(client.is_settled(&batch));
    let renewed = entry_ttl(&e, &id, &(symbol_short!("bat"), batch));
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn every_write_path_renews_the_lifetime_of_the_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);
    assert_eq!(instance_ttl(&e, &id), max);

    client.submit_batch(&admin, &key(&e, 31), &entries(&e), &client.compute_digest(&entries(&e)));
    assert_eq!(client.batch_count(), 1);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn a_submission_restores_a_decayed_contract_instance() {
    let (e, admin, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);
    assert_eq!(client.batch_count(), 0);

    let batch = key(&e, 41);
    client.submit_batch(&admin, &batch, &entries(&e), &client.compute_digest(&entries(&e)));
    assert_eq!(client.batch_count(), 1);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
}
