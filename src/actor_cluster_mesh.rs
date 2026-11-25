// Shae Distributed Actor Mesh: Consistent Hash Ring & Dynamic Actor State Migration
// Part of Option 4: Distributed Actor Mesh & Edge Clustering

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[inline]
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3u64);
    }
    hash
}

// -----------------------------------------------------------------------------
// Consistent Hashing Ring with Virtual Nodes
// -----------------------------------------------------------------------------

pub struct ConsistentHashRing {
    pub vnodes_per_node: usize,
    pub ring: BTreeMap<u64, String>, // hash -> physical_node_id
    pub nodes: HashSet<String>,
}

impl ConsistentHashRing {
    pub fn new(vnodes_per_node: usize) -> Self {
        Self {
            vnodes_per_node: vnodes_per_node.max(1),
            ring: BTreeMap::new(),
            nodes: HashSet::new(),
        }
    }

    pub fn add_node(&mut self, node_id: &str) {
        if self.nodes.contains(node_id) {
            return;
        }
        self.nodes.insert(node_id.to_string());

        for v in 0..self.vnodes_per_node {
            let vnode_key = format!("{}:#vnode:{}", node_id, v);
            let hash = fnv1a_64(vnode_key.as_bytes());
            self.ring.insert(hash, node_id.to_string());
        }
    }

    pub fn remove_node(&mut self, node_id: &str) {
        if !self.nodes.remove(node_id) {
            return;
        }

        for v in 0..self.vnodes_per_node {
            let vnode_key = format!("{}:#vnode:{}", node_id, v);
            let hash = fnv1a_64(vnode_key.as_bytes());
            self.ring.remove(&hash);
        }
    }

    /// Finds the primary responsible node on the ring clockwise from entity_key hash.
    pub fn get_node(&self, entity_key: &str) -> Option<String> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = fnv1a_64(entity_key.as_bytes());

        // Find first vnode with hash >= entity_key hash
        if let Some((_, node)) = self.ring.range(hash..).next() {
            Some(node.clone())
        } else {
            // Wrap around to start of ring
            self.ring.values().next().cloned()
        }
    }

    /// Returns up to `count` distinct physical replica nodes for an entity key.
    pub fn get_replicas(&self, entity_key: &str, count: usize) -> Vec<String> {
        if self.ring.is_empty() || count == 0 {
            return Vec::new();
        }

        let hash = fnv1a_64(entity_key.as_bytes());
        let mut replicas = Vec::new();
        let mut seen = HashSet::new();

        let total_available = self.nodes.len().min(count);

        // Scan from hash to end
        for (_, node) in self.ring.range(hash..) {
            if seen.insert(node.clone()) {
                replicas.push(node.clone());
                if replicas.len() >= total_available {
                    return replicas;
                }
            }
        }

        // Wrap around from beginning of ring
        for (_, node) in self.ring.range(..hash) {
            if seen.insert(node.clone()) {
                replicas.push(node.clone());
                if replicas.len() >= total_available {
                    return replicas;
                }
            }
        }

        replicas
    }
}

// -----------------------------------------------------------------------------
// Actor State Migration & Mailbox Buffering
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Active,
    Migrating,
    Completed,
}

pub struct MigrationRecord {
    pub actor_id: String,
    pub source_node: String,
    pub target_node: String,
    pub status: MigrationStatus,
    pub snapshot: Option<Value>,
    pub buffered_messages: Vec<Value>,
}

pub struct ActorMigrationCoordinator {
    pub migrations: RwLock<HashMap<String, MigrationRecord>>,
    pub routing_table: RwLock<HashMap<String, String>>, // actor_id -> current_node
}

impl ActorMigrationCoordinator {
    pub fn new() -> Self {
        Self {
            migrations: RwLock::new(HashMap::new()),
            routing_table: RwLock::new(HashMap::new()),
        }
    }

    /// Initiates zero-downtime migration of an actor from source_node to target_node.
    pub fn start_migration(
        &self,
        actor_id: &str,
        source_node: &str,
        target_node: &str,
        snapshot: Value,
    ) {
        let mut table = self.routing_table.write().unwrap();
        table.insert(actor_id.to_string(), target_node.to_string());

        let mut migs = self.migrations.write().unwrap();
        migs.insert(
            actor_id.to_string(),
            MigrationRecord {
                actor_id: actor_id.to_string(),
                source_node: source_node.to_string(),
                target_node: target_node.to_string(),
                status: MigrationStatus::Migrating,
                snapshot: Some(snapshot),
                buffered_messages: Vec::new(),
            },
        );
    }

    /// Buffers an incoming message if actor is currently migrating.
    /// Returns None if buffered, or Some(current_node) if actor is ready.
    pub fn route_or_buffer(&self, actor_id: &str, message: Value) -> Option<String> {
        let mut migs = self.migrations.write().unwrap();
        if let Some(record) = migs.get_mut(actor_id) {
            if record.status == MigrationStatus::Migrating {
                record.buffered_messages.push(message);
                return None; // Message buffered!
            }
        }

        let table = self.routing_table.read().unwrap();
        table.get(actor_id).cloned()
    }

    /// Completes the migration on target node and returns all buffered messages to replay.
    pub fn complete_migration(&self, actor_id: &str) -> Option<Vec<Value>> {
        let mut migs = self.migrations.write().unwrap();
        if let Some(record) = migs.get_mut(actor_id) {
            record.status = MigrationStatus::Completed;
            let flushed = std::mem::take(&mut record.buffered_messages);
            Some(flushed)
        } else {
            None
        }
    }

    pub fn get_location(&self, actor_id: &str) -> Option<String> {
        let table = self.routing_table.read().unwrap();
        table.get(actor_id).cloned()
    }
}

// -----------------------------------------------------------------------------
// Global Registry for Active Hash Rings & Migration Coordinators
// -----------------------------------------------------------------------------

static RING_REGISTRY: OnceLock<Mutex<HashMap<usize, Arc<RwLock<ConsistentHashRing>>>>> = OnceLock::new();
static NEXT_RING_ID: AtomicUsize = AtomicUsize::new(1);

fn get_ring_registry() -> &'static Mutex<HashMap<usize, Arc<RwLock<ConsistentHashRing>>>> {
    RING_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

static MIGRATION_COORDINATOR: OnceLock<ActorMigrationCoordinator> = OnceLock::new();

pub fn get_migration_coordinator() -> &'static ActorMigrationCoordinator {
    MIGRATION_COORDINATOR.get_or_init(ActorMigrationCoordinator::new)
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:actor:mesh & std:cluster:mesh
// -----------------------------------------------------------------------------

pub fn build_mesh_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("ring", mesh_ring),
        ("ring_add", mesh_ring_add),
        ("ring_remove", mesh_ring_remove),
        ("ring_get_node", mesh_ring_get_node),
        ("ring_get_replicas", mesh_ring_get_replicas),
        ("start_migration", mesh_start_migration),
        ("route_message", mesh_route_message),
        ("complete_migration", mesh_complete_migration),
        ("get_actor_location", mesh_get_actor_location),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("mesh.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn mesh_ring(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let vnodes = if !args.is_empty() {
        match &args[0] {
            Value::Int(i) if *i > 0 => *i as usize,
            _ => 50,
        }
    } else {
        50
    };

    let mut ring = ConsistentHashRing::new(vnodes);
    if args.len() > 1 {
        if let Value::Array(a) = &args[1] {
            let arr = a.read().unwrap();
            for item in arr.iter() {
                if let Value::String(s) = item {
                    ring.add_node(s);
                }
            }
        }
    }

    let handle = NEXT_RING_ID.fetch_add(1, Ordering::SeqCst);
    get_ring_registry().lock().unwrap().insert(handle, Arc::new(RwLock::new(ring)));

    let mut map = IndexMap::new();
    map.insert("handle".to_string(), Value::Int(handle as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn get_ring_from_val(v: &Value, span: Span) -> Result<Arc<RwLock<ConsistentHashRing>>, RuntimeError> {
    let handle = match v {
        Value::Int(i) => *i as usize,
        Value::Map(m) => {
            let map = m.read().unwrap();
            match map.get("handle") {
                Some(Value::Int(i)) => *i as usize,
                _ => return Err(RuntimeError::new("expected ring handle".to_string()).at(span)),
            }
        }
        _ => return Err(RuntimeError::new("expected ring handle or map".to_string()).at(span)),
    };

    let reg = get_ring_registry().lock().unwrap();
    reg.get(&handle)
        .cloned()
        .ok_or_else(|| RuntimeError::new(format!("hash ring {} not found", handle)).at(span))
}

fn mesh_ring_add(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.ring_add", 2, &args, span)?;
    let ring = get_ring_from_val(&args[0], span)?;
    let node_id = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string node_id".to_string()).at(span)),
    };

    ring.write().unwrap().add_node(node_id);
    Ok(Value::Bool(true))
}

fn mesh_ring_remove(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.ring_remove", 2, &args, span)?;
    let ring = get_ring_from_val(&args[0], span)?;
    let node_id = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string node_id".to_string()).at(span)),
    };

    ring.write().unwrap().remove_node(node_id);
    Ok(Value::Bool(true))
}

fn mesh_ring_get_node(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.ring_get_node", 2, &args, span)?;
    let ring = get_ring_from_val(&args[0], span)?;
    let key = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string entity key".to_string()).at(span)),
    };

    match ring.read().unwrap().get_node(key) {
        Some(node) => Ok(Value::String(node)),
        None => Ok(Value::Null),
    }
}

fn mesh_ring_get_replicas(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.ring_get_replicas", 3, &args, span)?;
    let ring = get_ring_from_val(&args[0], span)?;
    let key = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string entity key".to_string()).at(span)),
    };
    let count = match &args[2] {
        Value::Int(i) if *i > 0 => *i as usize,
        _ => 1,
    };

    let replicas: Vec<Value> = ring
        .read()
        .unwrap()
        .get_replicas(key, count)
        .into_iter()
        .map(Value::String)
        .collect();
    Ok(Value::Array(Arc::new(RwLock::new(replicas))))
}

fn mesh_start_migration(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.start_migration", 4, &args, span)?;
    let actor_id = match &args[0] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string actor_id".to_string()).at(span)),
    };
    let from_node = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string from_node".to_string()).at(span)),
    };
    let to_node = match &args[2] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string to_node".to_string()).at(span)),
    };
    let snapshot = args[3].clone();

    get_migration_coordinator().start_migration(actor_id, from_node, to_node, snapshot);
    Ok(Value::Bool(true))
}

fn mesh_route_message(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.route_message", 2, &args, span)?;
    let actor_id = match &args[0] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string actor_id".to_string()).at(span)),
    };
    let message = args[1].clone();

    match get_migration_coordinator().route_or_buffer(actor_id, message) {
        Some(target) => {
            let mut map = IndexMap::new();
            map.insert("status".to_string(), Value::String("routed".to_string()));
            map.insert("destination".to_string(), Value::String(target));
            Ok(Value::Map(Arc::new(RwLock::new(map))))
        }
        None => {
            let mut map = IndexMap::new();
            map.insert("status".to_string(), Value::String("buffered".to_string()));
            Ok(Value::Map(Arc::new(RwLock::new(map))))
        }
    }
}

fn mesh_complete_migration(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.complete_migration", 1, &args, span)?;
    let actor_id = match &args[0] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string actor_id".to_string()).at(span)),
    };

    match get_migration_coordinator().complete_migration(actor_id) {
        Some(flushed) => Ok(Value::Array(Arc::new(RwLock::new(flushed)))),
        None => Ok(Value::Array(Arc::new(RwLock::new(Vec::new())))),
    }
}

fn mesh_get_actor_location(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mesh.get_actor_location", 1, &args, span)?;
    let actor_id = match &args[0] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string actor_id".to_string()).at(span)),
    };

    match get_migration_coordinator().get_location(actor_id) {
        Some(loc) => Ok(Value::String(loc)),
        None => Ok(Value::Null),
    }
}

// -----------------------------------------------------------------------------
// Built-in Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistent_hash_ring() {
        let mut ring = ConsistentHashRing::new(50);
        ring.add_node("node_1");
        ring.add_node("node_2");
        ring.add_node("node_3");

        let target_a = ring.get_node("user_1001").expect("should find node");
        let target_b = ring.get_node("user_1002").expect("should find node");
        assert!(ring.nodes.contains(&target_a));
        assert!(ring.nodes.contains(&target_b));

        // Replicas test
        let replicas = ring.get_replicas("order_999", 2);
        assert_eq!(replicas.len(), 2);
        assert_ne!(replicas[0], replicas[1]);
    }

    #[test]
    fn test_actor_migration_buffering() {
        let coord = ActorMigrationCoordinator::new();

        // Start migration of actor 'cart_42' from node_A to node_B
        coord.start_migration("cart_42", "node_A", "node_B", Value::String("snapshot_data".into()));

        // Message sent while migrating should be buffered (returns None)
        let route1 = coord.route_or_buffer("cart_42", Value::String("msg1".into()));
        let route2 = coord.route_or_buffer("cart_42", Value::String("msg2".into()));
        assert!(route1.is_none());
        assert!(route2.is_none());

        // Complete migration on node_B -> flushes buffered messages
        let flushed = coord.complete_migration("cart_42").expect("migration record exists");
        assert_eq!(flushed.len(), 2);
        assert_eq!(flushed[0], Value::String("msg1".into()));
        assert_eq!(flushed[1], Value::String("msg2".into()));

        // Subsequent messages are directly routed to node_B
        let route3 = coord.route_or_buffer("cart_42", Value::String("msg3".into()));
        assert_eq!(route3, Some("node_B".into()));
    }
}
