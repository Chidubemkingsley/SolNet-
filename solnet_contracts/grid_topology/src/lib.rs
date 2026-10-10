//! SolNet: Grid Topology Contract
//! Description: Manages the structural topology map of the physical microgrid nodes
//! and the distribution edges between them. Nodes are registered by the grid
//! administrator with a role and a nameplate capacity, then linked into an
//! undirected graph that the rest of the mesh uses for routing decisions.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    NodeExists = 3,
    NodeNotFound = 4,
    NodeInactive = 5,
    SelfLink = 6,
    DuplicateEdge = 7,
    InvalidCapacity = 8,
}

/// Physical role a topology node plays in the microgrid.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// Produces power (solar array, microturbine).
    Generation,
    /// Consumes power (shop, water pump, cold store).
    Consumption,
    /// Buffers power (community battery bank).
    Storage,
    /// Routed distribution point (relay / transformer).
    Distribution,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    /// Nameplate capacity in milli-watt-hours. Kept as an integer because
    /// Soroban has no floating point representation.
    pub capacity_mwh: i128,
    pub active: bool,
}

#[contract]
pub struct GridTopology;

#[contractimpl]
impl GridTopology {
    /// Initializes the grid topology contract state configuration.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("count"), &0u32);
        env.storage().instance().set(&symbol_short!("capsum"), &0i128);
        Ok(())
    }

    /// Returns the administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_admin(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("admin")).unwrap())
    }

    /// Registers a physical node in the topology map.
    pub fn register_node(
        env: Env,
        admin: Address,
        id: u32,
        kind: NodeKind,
        capacity_mwh: i128,
    ) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if capacity_mwh <= 0 {
            return Err(Error::InvalidCapacity);
        }
        if Self::node_key_exists(&env, id) {
            return Err(Error::NodeExists);
        }

        let node = Node {
            id,
            kind,
            capacity_mwh,
            active: true,
        };
        Self::store_node(&env, &node);

        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("count"), &(count + 1));

        let capsum: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("capsum"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("capsum"), &(capsum + capacity_mwh));

        env.storage()
            .persistent()
            .set(&(symbol_short!("nb"), id), &Vec::<u32>::new(&env));
        Ok(())
    }

    /// Returns the full record for a node.
    pub fn get_node(env: Env, id: u32) -> Result<Node, Error> {
        if !Self::node_key_exists(&env, id) {
            return Err(Error::NodeNotFound);
        }
        let kind: NodeKind = env
            .storage()
            .persistent()
            .get(&(symbol_short!("kind"), id))
            .unwrap();
        let capacity_mwh: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("cap"), id))
            .unwrap();
        let active: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("live"), id))
            .unwrap();
        Ok(Node {
            id,
            kind,
            capacity_mwh,
            active,
        })
    }

    /// Reports whether a node id has ever been registered.
    pub fn is_registered(env: Env, id: u32) -> bool {
        Self::node_key_exists(&env, id)
    }

    /// Number of nodes currently registered.
    pub fn node_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0)
    }

    /// Links two nodes with an undirected distribution edge.
    pub fn link_edge(env: Env, admin: Address, a: u32, b: u32) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        if a == b {
            return Err(Error::SelfLink);
        }
        Self::require_live(&env, a)?;
        Self::require_live(&env, b)?;

        let mut nb_a: Vec<u32> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("nb"), a))
            .unwrap_or(Vec::new(&env));
        let mut nb_b: Vec<u32> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("nb"), b))
            .unwrap_or(Vec::new(&env));

        if nb_a.contains(b) || nb_b.contains(a) {
            return Err(Error::DuplicateEdge);
        }

        nb_a.push_back(b);
        nb_b.push_back(a);
        env.storage()
            .persistent()
            .set(&(symbol_short!("nb"), a), &nb_a);
        env.storage()
            .persistent()
            .set(&(symbol_short!("nb"), b), &nb_b);
        Ok(())
    }

    /// Adjacent node ids for a node.
    pub fn neighbors(env: Env, id: u32) -> Result<Vec<u32>, Error> {
        if !Self::node_key_exists(&env, id) {
            return Err(Error::NodeNotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&(symbol_short!("nb"), id))
            .unwrap_or(Vec::new(&env)))
    }

    /// Breadth-first reachability check across the topology graph.
    pub fn are_connected(env: Env, a: u32, b: u32) -> Result<bool, Error> {
        if !Self::node_key_exists(&env, a) {
            return Err(Error::NodeNotFound);
        }
        if !Self::node_key_exists(&env, b) {
            return Err(Error::NodeNotFound);
        }
        if a == b {
            return Ok(true);
        }

        let mut seen: Vec<u32> = Vec::new(&env);
        let mut frontier: Vec<u32> = Vec::new(&env);
        seen.push_back(a);
        frontier.push_back(a);

        while !frontier.is_empty() {
            let current = frontier.pop_front().unwrap();
            let adjacent: Vec<u32> = env
                .storage()
                .persistent()
                .get(&(symbol_short!("nb"), current))
                .unwrap_or(Vec::new(&env));

            for neighbor in adjacent.iter() {
                if neighbor == b {
                    return Ok(true);
                }
                if !seen.contains(neighbor) {
                    seen.push_back(neighbor);
                    frontier.push_back(neighbor);
                }
            }
        }
        Ok(false)
    }

    /// Retires a node so it can no longer participate in new edges.
    pub fn deactivate_node(env: Env, admin: Address, id: u32) -> Result<(), Error> {
        Self::require_admin_auth(&env, &admin)?;
        Self::require_live(&env, id)?;

        let cap: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("cap"), id))
            .unwrap();
        let capsum: i128 = env
            .storage()
            .instance()
            .get(&symbol_short!("capsum"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&symbol_short!("capsum"), &(capsum - cap));
        env.storage()
            .persistent()
            .set(&(symbol_short!("live"), id), &false);
        Ok(())
    }

    /// Total nameplate capacity across all active nodes.
    pub fn total_capacity(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&symbol_short!("capsum"))
            .unwrap_or(0)
    }

    fn require_admin_auth(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(Error::NotAdmin)?;
        if &stored != admin {
            return Err(Error::NotAdmin);
        }
        admin.require_auth();
        Ok(())
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        if env
            .storage()
            .instance()
            .has(&symbol_short!("admin"))
        {
            Ok(())
        } else {
            Err(Error::NotAdmin)
        }
    }

    fn node_key_exists(env: &Env, id: u32) -> bool {
        env.storage()
            .persistent()
            .has(&(symbol_short!("kind"), id))
    }

    fn require_live(env: &Env, id: u32) -> Result<(), Error> {
        if !Self::node_key_exists(env, id) {
            return Err(Error::NodeNotFound);
        }
        let active: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("live"), id))
            .unwrap();
        if active {
            Ok(())
        } else {
            Err(Error::NodeInactive)
        }
    }

    fn store_node(env: &Env, node: &Node) {
        env.storage()
            .persistent()
            .set(&(symbol_short!("kind"), node.id), &node.kind);
        env.storage()
            .persistent()
            .set(&(symbol_short!("cap"), node.id), &node.capacity_mwh);
        env.storage()
            .persistent()
            .set(&(symbol_short!("live"), node.id), &node.active);
    }
}