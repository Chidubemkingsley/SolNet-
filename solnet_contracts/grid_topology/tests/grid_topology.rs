use soroban_sdk::{
    symbol_short,
    testutils::{storage::Instance as _, storage::Persistent as _, Address as _, Ledger as _},
    vec, Address, Env,
};
use solnet_grid_topology::{Error, GridTopology, GridTopologyClient, NodeKind};

fn setup() -> (Env, Address, GridTopologyClient<'static>) {
    let (e, admin, _, client) = setup_tracked();
    (e, admin, client)
}

fn setup_tracked() -> (Env, Address, Address, GridTopologyClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(GridTopology, ());
    let client = GridTopologyClient::new(&e, &id);
    client.init(&admin);
    (e, admin, id, client)
}

fn node(id: u32, kind: NodeKind, cap: i128) -> solnet_grid_topology::Node {
    solnet_grid_topology::Node {
        id,
        kind,
        capacity_mwh: cap,
        active: true,
    }
}

#[test]
fn init_stores_admin() {
    let (_e, admin, client) = setup();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.node_count(), 0);
    // init is idempotent-guarded
    assert_eq!(client.try_init(&admin), Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn register_node_increments_count() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &5_000);
    assert_eq!(client.node_count(), 1);
    client.register_node(&admin, &1, &NodeKind::Consumption, &2_000);
    assert_eq!(client.node_count(), 2);
}

#[test]
fn register_node_persists_kind_and_capacity() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &7, &NodeKind::Storage, &12_345);
    assert_eq!(client.get_node(&7), node(7, NodeKind::Storage, 12_345));
}

#[test]
fn register_node_is_addressable_by_sparse_ids() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &900, &NodeKind::Distribution, &1_000);
    assert!(client.is_registered(&900));
    assert!(!client.is_registered(&0));
    assert_eq!(client.get_node(&900).capacity_mwh, 1_000);
}

#[test]
fn duplicate_registration_is_rejected() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &3, &NodeKind::Generation, &1_000);
    assert_eq!(
        client.try_register_node(&admin, &3, &NodeKind::Storage, &1_000),
        Err(Ok(Error::NodeExists))
    );
    assert_eq!(client.node_count(), 1);
}

#[test]
fn non_positive_capacity_is_rejected() {
    let (_e, admin, client) = setup();
    assert_eq!(
        client.try_register_node(&admin, &1, &NodeKind::Generation, &0),
        Err(Ok(Error::InvalidCapacity))
    );
    assert_eq!(
        client.try_register_node(&admin, &1, &NodeKind::Generation, &-5),
        Err(Ok(Error::InvalidCapacity))
    );
    assert_eq!(client.node_count(), 0);
}

#[test]
fn unregistered_node_reads_fail() {
    let (_e, _admin, client) = setup();
    assert!(!client.is_registered(&42));
    assert_eq!(client.try_get_node(&42), Err(Ok(Error::NodeNotFound)));
}

#[test]
fn non_admin_cannot_register() {
    let (e, admin, client) = setup();
    let impostor = Address::generate(&e);
    assert_eq!(
        client.try_register_node(&impostor, &1, &NodeKind::Generation, &1_000),
        Err(Ok(Error::NotAdmin))
    );
    // and the real admin still works afterwards
    client.register_node(&admin, &1, &NodeKind::Generation, &1_000);
}

#[test]
fn link_edge_is_symmetric() {
    let (e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    client.register_node(&admin, &1, &NodeKind::Consumption, &1_000);
    client.link_edge(&admin, &0, &1);

    assert_eq!(client.neighbors(&0), vec![&e, 1]);
    assert_eq!(client.neighbors(&1), vec![&e, 0]);
}

#[test]
fn link_edge_rejects_self_and_duplicates() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    client.register_node(&admin, &1, &NodeKind::Consumption, &1_000);

    assert_eq!(
        client.try_link_edge(&admin, &1, &1),
        Err(Ok(Error::SelfLink))
    );
    client.link_edge(&admin, &0, &1);
    assert_eq!(
        client.try_link_edge(&admin, &1, &0),
        Err(Ok(Error::DuplicateEdge))
    );
}

#[test]
fn link_edge_requires_live_endpoints() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    assert_eq!(
        client.try_link_edge(&admin, &0, &99),
        Err(Ok(Error::NodeNotFound))
    );
    client.deactivate_node(&admin, &0);
    assert_eq!(
        client.try_link_edge(&admin, &0, &1),
        Err(Ok(Error::NodeInactive))
    );
}

#[test]
fn connectivity_is_transitive() {
    let (_e, admin, client) = setup();
    for id in 0..4u32 {
        client.register_node(&admin, &id, &NodeKind::Distribution, &1_000);
    }
    client.link_edge(&admin, &0, &1);
    client.link_edge(&admin, &1, &2);
    client.link_edge(&admin, &2, &3);

    assert!(client.are_connected(&0, &3));
    assert!(client.are_connected(&3, &0));
    assert!(client.are_connected(&1, &2));
}

#[test]
fn disconnected_components_are_not_connected() {
    let (_e, admin, client) = setup();
    for id in 0..4u32 {
        client.register_node(&admin, &id, &NodeKind::Distribution, &1_000);
    }
    client.link_edge(&admin, &0, &1);
    client.link_edge(&admin, &2, &3);

    assert!(!client.are_connected(&0, &2));
    assert!(client.are_connected(&0, &0));
}

#[test]
fn connectivity_requires_both_nodes_registered() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    assert_eq!(
        client.try_are_connected(&0, &1),
        Err(Ok(Error::NodeNotFound))
    );
}

#[test]
fn deactivation_updates_capacity_and_is_not_repeatable() {
    let (_e, admin, client) = setup();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    client.register_node(&admin, &9, &NodeKind::Storage, &2_500);

    assert_eq!(client.total_capacity(), 3_500);
    client.deactivate_node(&admin, &9);
    assert_eq!(client.total_capacity(), 1_000);
    assert!(!client.get_node(&9).active);

    assert_eq!(
        client.try_deactivate_node(&admin, &9),
        Err(Ok(Error::NodeInactive))
    );
    assert_eq!(client.total_capacity(), 1_000);
}
#[test]
fn traversal_beyond_the_budget_ceiling_fails_loudly() {
    let (e, admin, cid, client) = setup_tracked();
    let chain = client.traversal_limit() + 2;

    // Seed a chain just past the ceiling directly into storage. Going through
    // `register_node`/`link_edge` would cost thousands of contract invocations
    // to describe a shape that is four keys per node, and the ceiling is what
    // is under test here, not node creation.
    e.as_contract(&cid, || {
        for n in 0..chain {
            e.storage()
                .persistent()
                .set(&(symbol_short!("kind"), n), &NodeKind::Distribution);
            e.storage()
                .persistent()
                .set(&(symbol_short!("cap"), n), &1_000i128);
            e.storage().persistent().set(&(symbol_short!("live"), n), &true);
            let mut nbs: soroban_sdk::Vec<u32> = soroban_sdk::Vec::new(&e);
            if n > 0 {
                nbs.push_back(n - 1);
            }
            if n + 1 < chain {
                nbs.push_back(n + 1);
            }
            e.storage().persistent().set(&(symbol_short!("nb"), n), &nbs);
        }
    });

    assert_eq!(
        client.try_are_connected(&0, &(chain - 1)),
        Err(Ok(Error::TraversalTooLarge))
    );
    // A walk that stays inside the ceiling still answers normally.
    assert!(client.are_connected(&0, &2));
    let _ = admin;
}

#[test]
fn raising_the_ceiling_admits_a_longer_walk() {
    let (e, admin, cid, client) = setup_tracked();
    let chain = client.traversal_limit() + 2;

    e.as_contract(&cid, || {
        for n in 0..chain {
            e.storage()
                .persistent()
                .set(&(symbol_short!("kind"), n), &NodeKind::Distribution);
            e.storage().persistent().set(&(symbol_short!("live"), n), &true);
            let mut nbs: soroban_sdk::Vec<u32> = soroban_sdk::Vec::new(&e);
            if n > 0 {
                nbs.push_back(n - 1);
            }
            if n + 1 < chain {
                nbs.push_back(n + 1);
            }
            e.storage().persistent().set(&(symbol_short!("nb"), n), &nbs);
        }
    });

    assert_eq!(
        client.try_are_connected(&0, &(chain - 1)),
        Err(Ok(Error::TraversalTooLarge))
    );

    client.set_traversal_limit(&admin, &(chain + 10));
    assert_eq!(client.traversal_limit(), chain + 10);
    assert!(client.are_connected(&0, &(chain - 1)));
}

#[test]
fn traversal_limit_is_validated_and_admin_only() {
    let (e, admin, client) = setup();
    assert_eq!(client.traversal_limit(), 64);

    assert_eq!(
        client.try_set_traversal_limit(&admin, &0),
        Err(Ok(Error::InvalidTraversalLimit))
    );
    assert_eq!(
        client.try_set_traversal_limit(&admin, &1_000_000),
        Err(Ok(Error::InvalidTraversalLimit))
    );

    let impostor = Address::generate(&e);
    assert_eq!(
        client.try_set_traversal_limit(&impostor, &128),
        Err(Ok(Error::NotAdmin))
    );

    client.set_traversal_limit(&admin, &1);
    assert_eq!(client.traversal_limit(), 1);
    client.set_traversal_limit(&admin, &4096);
    assert_eq!(client.traversal_limit(), 4096);
}

#[test]
fn register_node_renews_persistent_entries() {
    let (e, admin, cid, client) = setup_tracked();
    client.register_node(&admin, &5, &NodeKind::Generation, &1_000);

    let max = e.storage().max_ttl();
    let min = e.ledger().get().min_persistent_entry_ttl;
    for tag in ["kind", "cap", "live", "nb"] {
        let ttl = e.as_contract(&cid, || {
            e.storage()
                .persistent()
                .get_ttl(&(soroban_sdk::Symbol::new(&e, tag), 5u32))
        });
        assert_eq!(ttl, max, "entry `{tag}` should be renewed to max");
        assert!(ttl > min, "renewal must beat the minimum entry TTL");
    }
}

#[test]
fn link_edge_renews_both_neighbour_lists() {
    let (e, admin, cid, client) = setup_tracked();
    client.register_node(&admin, &0, &NodeKind::Generation, &1_000);
    client.register_node(&admin, &1, &NodeKind::Consumption, &1_000);
    client.link_edge(&admin, &0, &1);

    let max = e.storage().max_ttl();
    for node in [0u32, 1u32] {
        let ttl = e.as_contract(&cid, || {
            e.storage()
                .persistent()
                .get_ttl(&(symbol_short!("nb"), node))
        });
        assert_eq!(ttl, max, "neighbour list for node {node}");
    }
}

#[test]
fn init_renews_the_contract_instance() {
    let e = Env::default();
    e.mock_all_auths();
    let cid = e.register(GridTopology, ());
    let client = GridTopologyClient::new(&e, &cid);
    let admin = Address::generate(&e);
    client.init(&admin);

    let ttl = e.as_contract(&cid, || e.storage().instance().get_ttl());
    assert_eq!(ttl, e.storage().max_ttl());
}
