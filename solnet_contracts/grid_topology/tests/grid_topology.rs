use soroban_sdk::{testutils::Address as _, vec, Address, Env};
use solnet_grid_topology::{Error, GridTopology, GridTopologyClient, NodeKind};

fn setup() -> (Env, Address, GridTopologyClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let id = e.register(GridTopology, ());
    let client = GridTopologyClient::new(&e, &id);
    client.init(&admin);
    (e, admin, client)
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