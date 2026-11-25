// Shae Distributed Actor Mesh: SWiM Gossip Protocol & Probabilistic Failure Detector
// Part of Option 4: Distributed Actor Mesh & Edge Clustering

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Alive,
    Suspect,
    Dead,
    Left,
}

impl NodeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeState::Alive => "alive",
            NodeState::Suspect => "suspect",
            NodeState::Dead => "dead",
            NodeState::Left => "left",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "alive" => Some(NodeState::Alive),
            "suspect" => Some(NodeState::Suspect),
            "dead" => Some(NodeState::Dead),
            "left" => Some(NodeState::Left),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GossipMember {
    pub id: String,
    pub address: String,
    pub state: NodeState,
    pub incarnation: u64,
    pub last_seen: Instant,
    pub suspect_since: Option<Instant>,
}

impl GossipMember {
    pub fn new(id: String, address: String, state: NodeState, incarnation: u64) -> Self {
        Self {
            id,
            address,
            state,
            incarnation,
            last_seen: Instant::now(),
            suspect_since: if state == NodeState::Suspect {
                Some(Instant::now())
            } else {
                None
            },
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("id".to_string(), Value::String(self.id.clone()));
        map.insert("address".to_string(), Value::String(self.address.clone()));
        map.insert("state".to_string(), Value::String(self.state.as_str().to_string()));
        map.insert("incarnation".to_string(), Value::Int(self.incarnation as i64));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GossipRumor {
    pub member_id: String,
    pub address: String,
    pub state: NodeState,
    pub incarnation: u64,
}

impl GossipRumor {
    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("member_id".to_string(), Value::String(self.member_id.clone()));
        map.insert("address".to_string(), Value::String(self.address.clone()));
        map.insert("state".to_string(), Value::String(self.state.as_str().to_string()));
        map.insert("incarnation".to_string(), Value::Int(self.incarnation as i64));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

pub struct GossipEngine {
    pub local_id: String,
    pub local_addr: String,
    pub incarnation: AtomicU64,
    pub members: Arc<RwLock<HashMap<String, GossipMember>>>,
    pub rumor_buffer: Arc<Mutex<Vec<GossipRumor>>>,
}

impl GossipEngine {
    pub fn new(local_id: String, local_addr: String) -> Self {
        let mut members = HashMap::new();
        members.insert(
            local_id.clone(),
            GossipMember::new(local_id.clone(), local_addr.clone(), NodeState::Alive, 0),
        );

        Self {
            local_id,
            local_addr,
            incarnation: AtomicU64::new(0),
            members: Arc::new(RwLock::new(members)),
            rumor_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_member(&self, id: String, address: String) {
        let mut members = self.members.write().unwrap();
        members.entry(id.clone()).or_insert_with(|| {
            GossipMember::new(id, address, NodeState::Alive, 0)
        });
    }

    /// Processes an incoming rumor according to SWiM epidemic conflict resolution rules.
    /// Returns true if the rumor updated local cluster state or triggered a suspicion refutation.
    pub fn apply_rumor(&self, rumor: GossipRumor) -> bool {
        // Rule 1: Self-Refutation
        if rumor.member_id == self.local_id {
            if rumor.state == NodeState::Suspect && rumor.incarnation >= self.incarnation.load(Ordering::SeqCst) {
                // Someone suspects us! Increment incarnation and broadcast Alive rumor to refute.
                let next_inc = rumor.incarnation + 1;
                self.incarnation.store(next_inc, Ordering::SeqCst);

                let mut members = self.members.write().unwrap();
                if let Some(me) = members.get_mut(&self.local_id) {
                    me.incarnation = next_inc;
                    me.state = NodeState::Alive;
                    me.suspect_since = None;
                    me.last_seen = Instant::now();
                }

                let mut rumors = self.rumor_buffer.lock().unwrap();
                rumors.push(GossipRumor {
                    member_id: self.local_id.clone(),
                    address: self.local_addr.clone(),
                    state: NodeState::Alive,
                    incarnation: next_inc,
                });
                return true;
            }
            return false;
        }

        // Rule 2: Remote Member State Transition
        let mut members = self.members.write().unwrap();
        if let Some(member) = members.get_mut(&rumor.member_id) {
            let mut state_changed = false;

            if rumor.incarnation > member.incarnation {
                // Newer incarnation always supersedes
                member.incarnation = rumor.incarnation;
                member.state = rumor.state;
                if rumor.state == NodeState::Suspect {
                    member.suspect_since = Some(Instant::now());
                } else {
                    member.suspect_since = None;
                }
                member.last_seen = Instant::now();
                state_changed = true;
            } else if rumor.incarnation == member.incarnation {
                // Same incarnation: Suspect overrides Alive; Dead overrides Suspect and Alive
                match (member.state, rumor.state) {
                    (NodeState::Alive, NodeState::Suspect) => {
                        member.state = NodeState::Suspect;
                        member.suspect_since = Some(Instant::now());
                        state_changed = true;
                    }
                    (NodeState::Alive, NodeState::Dead) | (NodeState::Suspect, NodeState::Dead) => {
                        member.state = NodeState::Dead;
                        member.suspect_since = None;
                        state_changed = true;
                    }
                    _ => {}
                }
            }

            if state_changed {
                let mut rumors = self.rumor_buffer.lock().unwrap();
                rumors.push(rumor.clone());
            }
            state_changed
        } else {
            // New previously unseen member
            let new_member = GossipMember::new(
                rumor.member_id.clone(),
                rumor.address.clone(),
                rumor.state,
                rumor.incarnation,
            );
            members.insert(rumor.member_id.clone(), new_member);

            let mut rumors = self.rumor_buffer.lock().unwrap();
            rumors.push(rumor);
            true
        }
    }

    /// Updates failure detector state after a direct ping probe.
    pub fn probe_member(&self, target_id: &str, is_responsive: bool) {
        if target_id == self.local_id {
            return;
        }

        let mut members = self.members.write().unwrap();
        if let Some(m) = members.get_mut(target_id) {
            if is_responsive {
                m.last_seen = Instant::now();
                if m.state == NodeState::Suspect {
                    // Node answered probe: transition back to Alive
                    m.state = NodeState::Alive;
                    m.suspect_since = None;

                    let mut rumors = self.rumor_buffer.lock().unwrap();
                    rumors.push(GossipRumor {
                        member_id: m.id.clone(),
                        address: m.address.clone(),
                        state: NodeState::Alive,
                        incarnation: m.incarnation,
                    });
                }
            } else {
                // Unresponsive: if currently Alive, transition to Suspect
                if m.state == NodeState::Alive {
                    m.state = NodeState::Suspect;
                    m.suspect_since = Some(Instant::now());

                    let mut rumors = self.rumor_buffer.lock().unwrap();
                    rumors.push(GossipRumor {
                        member_id: m.id.clone(),
                        address: m.address.clone(),
                        state: NodeState::Suspect,
                        incarnation: m.incarnation,
                    });
                }
            }
        }
    }

    /// Evaluates suspicion timeouts and marks unrefuted suspect nodes as Dead.
    pub fn check_timeouts(&self, suspect_timeout: Duration) {
        let mut members = self.members.write().unwrap();
        let mut dead_rumors = Vec::new();

        for m in members.values_mut() {
            if m.state == NodeState::Suspect {
                if let Some(suspect_start) = m.suspect_since {
                    if suspect_start.elapsed() >= suspect_timeout {
                        m.state = NodeState::Dead;
                        m.suspect_since = None;
                        dead_rumors.push(GossipRumor {
                            member_id: m.id.clone(),
                            address: m.address.clone(),
                            state: NodeState::Dead,
                            incarnation: m.incarnation,
                        });
                    }
                }
            }
        }

        if !dead_rumors.is_empty() {
            let mut rumors = self.rumor_buffer.lock().unwrap();
            rumors.extend(dead_rumors);
        }
    }

    pub fn alive_members(&self) -> Vec<GossipMember> {
        let members = self.members.read().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .cloned()
            .collect()
    }

    pub fn suspect_members(&self) -> Vec<GossipMember> {
        let members = self.members.read().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Suspect)
            .cloned()
            .collect()
    }

    pub fn dead_members(&self) -> Vec<GossipMember> {
        let members = self.members.read().unwrap();
        members
            .values()
            .filter(|m| m.state == NodeState::Dead)
            .cloned()
            .collect()
    }

    pub fn is_alive(&self, id: &str) -> bool {
        let members = self.members.read().unwrap();
        members
            .get(id)
            .map(|m| m.state == NodeState::Alive)
            .unwrap_or(false)
    }

    pub fn is_suspect(&self, id: &str) -> bool {
        let members = self.members.read().unwrap();
        members
            .get(id)
            .map(|m| m.state == NodeState::Suspect)
            .unwrap_or(false)
    }

    pub fn drain_rumors(&self, max_count: usize) -> Vec<GossipRumor> {
        let mut rumors = self.rumor_buffer.lock().unwrap();
        let count = rumors.len().min(max_count);
        rumors.drain(0..count).collect()
    }
}

// -----------------------------------------------------------------------------
// Global Gossip Engine Registry
// -----------------------------------------------------------------------------

static GOSSIP_REGISTRY: OnceLock<Mutex<HashMap<usize, Arc<GossipEngine>>>> = OnceLock::new();
static NEXT_GOSSIP_ID: AtomicUsize = AtomicUsize::new(1);

fn get_gossip_registry() -> &'static Mutex<HashMap<usize, Arc<GossipEngine>>> {
    GOSSIP_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:actor:gossip
// -----------------------------------------------------------------------------

pub fn build_gossip_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("create", gossip_create),
        ("add_member", gossip_add_member),
        ("apply_rumor", gossip_apply_rumor),
        ("probe", gossip_probe),
        ("check_timeouts", gossip_check_timeouts),
        ("alive_members", gossip_alive_members),
        ("suspect_members", gossip_suspect_members),
        ("dead_members", gossip_dead_members),
        ("is_alive", gossip_is_alive),
        ("is_suspect", gossip_is_suspect),
        ("drain_rumors", gossip_drain_rumors),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("gossip.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn gossip_create(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.create", 2, &args, span)?;
    let id = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("gossip.create: expected string local_id".to_string()).at(span)),
    };
    let addr = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("gossip.create: expected string local_addr".to_string()).at(span)),
    };

    let engine = Arc::new(GossipEngine::new(id, addr));
    let handle_id = NEXT_GOSSIP_ID.fetch_add(1, Ordering::SeqCst);
    get_gossip_registry().lock().unwrap().insert(handle_id, engine);

    let mut map = IndexMap::new();
    map.insert("handle".to_string(), Value::Int(handle_id as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn get_engine_from_val(v: &Value, span: Span) -> Result<Arc<GossipEngine>, RuntimeError> {
    let handle = match v {
        Value::Int(i) => *i as usize,
        Value::Map(m) => {
            let map = m.read().unwrap();
            match map.get("handle") {
                Some(Value::Int(i)) => *i as usize,
                _ => return Err(RuntimeError::new("expected gossip handle".to_string()).at(span)),
            }
        }
        _ => return Err(RuntimeError::new("expected gossip handle or map".to_string()).at(span)),
    };

    let reg = get_gossip_registry().lock().unwrap();
    reg.get(&handle)
        .cloned()
        .ok_or_else(|| RuntimeError::new(format!("gossip engine {} not found", handle)).at(span))
}

fn gossip_add_member(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.add_member", 3, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let id = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("expected string member id".to_string()).at(span)),
    };
    let addr = match &args[2] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("expected string address".to_string()).at(span)),
    };

    engine.add_member(id, addr);
    Ok(Value::Bool(true))
}

fn gossip_apply_rumor(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.apply_rumor", 5, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let member_id = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("expected string member_id".to_string()).at(span)),
    };
    let address = match &args[2] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("expected string address".to_string()).at(span)),
    };
    let state_str = match &args[3] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string state ('alive', 'suspect', 'dead')".to_string()).at(span)),
    };
    let state = NodeState::from_str(state_str)
        .ok_or_else(|| RuntimeError::new(format!("invalid node state '{}'", state_str)).at(span))?;
    let incarnation = match &args[4] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };

    let rumor = GossipRumor {
        member_id,
        address,
        state,
        incarnation,
    };

    let updated = engine.apply_rumor(rumor);
    Ok(Value::Bool(updated))
}

fn gossip_probe(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.probe", 3, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let target_id = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("expected string target_id".to_string()).at(span)),
    };
    let success = match &args[2] {
        Value::Bool(b) => *b,
        _ => true,
    };

    engine.probe_member(&target_id, success);
    Ok(Value::Bool(true))
}

fn gossip_check_timeouts(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.check_timeouts", 2, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let timeout_ms = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as u64,
        Value::Float(f) if *f >= 0.0 => *f as u64,
        _ => 500,
    };

    engine.check_timeouts(Duration::from_millis(timeout_ms));
    Ok(Value::Bool(true))
}

fn gossip_alive_members(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.alive_members", 1, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let members: Vec<Value> = engine.alive_members().into_iter().map(|m| m.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(members))))
}

fn gossip_suspect_members(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.suspect_members", 1, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let members: Vec<Value> = engine.suspect_members().into_iter().map(|m| m.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(members))))
}

fn gossip_dead_members(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.dead_members", 1, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let members: Vec<Value> = engine.dead_members().into_iter().map(|m| m.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(members))))
}

fn gossip_is_alive(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.is_alive", 2, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let id = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string id".to_string()).at(span)),
    };
    Ok(Value::Bool(engine.is_alive(id)))
}

fn gossip_is_suspect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.is_suspect", 2, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let id = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string id".to_string()).at(span)),
    };
    Ok(Value::Bool(engine.is_suspect(id)))
}

fn gossip_drain_rumors(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gossip.drain_rumors", 2, &args, span)?;
    let engine = get_engine_from_val(&args[0], span)?;
    let max_count = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 10,
    };
    let rumors: Vec<Value> = engine.drain_rumors(max_count).into_iter().map(|r| r.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(rumors))))
}

// -----------------------------------------------------------------------------
// Built-in Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gossip_initial_state() {
        let engine = GossipEngine::new("node_a".into(), "127.0.0.1:8001".into());
        assert!(engine.is_alive("node_a"));
        assert!(!engine.is_suspect("node_a"));
        assert_eq!(engine.alive_members().len(), 1);
    }

    #[test]
    fn test_gossip_probe_failure_detection() {
        let engine = GossipEngine::new("node_a".into(), "127.0.0.1:8001".into());
        engine.add_member("node_b".into(), "127.0.0.1:8002".into());
        assert!(engine.is_alive("node_b"));

        // Failed probe makes node_b suspect
        engine.probe_member("node_b", false);
        assert!(!engine.is_alive("node_b"));
        assert!(engine.is_suspect("node_b"));
        assert_eq!(engine.suspect_members().len(), 1);

        // Successful probe restores node_b to alive
        engine.probe_member("node_b", true);
        assert!(engine.is_alive("node_b"));
        assert!(!engine.is_suspect("node_b"));
    }

    #[test]
    fn test_gossip_self_refutation() {
        let engine = GossipEngine::new("node_a".into(), "127.0.0.1:8001".into());

        // Rumor claims node_a is Suspect with incarnation 0
        let rumor = GossipRumor {
            member_id: "node_a".into(),
            address: "127.0.0.1:8001".into(),
            state: NodeState::Suspect,
            incarnation: 0,
        };

        // Node_a refutes the rumor by bumping incarnation to 1 and emitting Alive rumor
        let updated = engine.apply_rumor(rumor);
        assert!(updated);
        assert!(engine.is_alive("node_a"));
        assert_eq!(engine.incarnation.load(Ordering::SeqCst), 1);

        let rumors = engine.drain_rumors(10);
        assert_eq!(rumors.len(), 1);
        assert_eq!(rumors[0].state, NodeState::Alive);
        assert_eq!(rumors[0].incarnation, 1);
    }
}
