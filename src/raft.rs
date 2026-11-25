// Shae Distributed Consensus Engine: Raft Consensus & Replicated State Machine
// Part of Option 4: Distributed Actor Mesh & Edge Clustering

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

impl RaftRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            RaftRole::Follower => "follower",
            RaftRole::Candidate => "candidate",
            RaftRole::Leader => "leader",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub index: usize,
    pub term: u64,
    pub command: String, // e.g. "SET key value" or "DEL key"
}

impl LogEntry {
    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("index".to_string(), Value::Int(self.index as i64));
        map.insert("term".to_string(), Value::Int(self.term as i64));
        map.insert("command".to_string(), Value::String(self.command.clone()));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

#[derive(Debug, Clone)]
pub struct RequestVoteArgs {
    pub term: u64,
    pub candidate_id: String,
    pub last_log_index: usize,
    pub last_log_term: u64,
}

#[derive(Debug, Clone)]
pub struct RequestVoteReply {
    pub term: u64,
    pub vote_granted: bool,
}

#[derive(Debug, Clone)]
pub struct AppendEntriesArgs {
    pub term: u64,
    pub leader_id: String,
    pub prev_log_index: usize,
    pub prev_log_term: u64,
    pub entries: Vec<LogEntry>,
    pub leader_commit: usize,
}

#[derive(Debug, Clone)]
pub struct AppendEntriesReply {
    pub term: u64,
    pub success: bool,
    pub match_index: usize,
}

pub struct RaftNode {
    pub id: String,
    pub peers: Vec<String>,

    // Persistent State
    pub current_term: RwLock<u64>,
    pub voted_for: RwLock<Option<String>>,
    pub log: RwLock<Vec<LogEntry>>,

    // Volatile State
    pub role: RwLock<RaftRole>,
    pub commit_index: RwLock<usize>,
    pub last_applied: RwLock<usize>,

    // Leader-only Volatile State
    pub next_index: RwLock<HashMap<String, usize>>,
    pub match_index: RwLock<HashMap<String, usize>>,
    pub votes_received: RwLock<HashMap<String, bool>>,

    // Replicated Key-Value State Machine
    pub state_machine: RwLock<HashMap<String, String>>,
}

impl RaftNode {
    pub fn new(id: String, peers: Vec<String>) -> Self {
        Self {
            id,
            peers,
            current_term: RwLock::new(0),
            voted_for: RwLock::new(None),
            log: RwLock::new(Vec::new()),
            role: RwLock::new(RaftRole::Follower),
            commit_index: RwLock::new(0),
            last_applied: RwLock::new(0),
            next_index: RwLock::new(HashMap::new()),
            match_index: RwLock::new(HashMap::new()),
            votes_received: RwLock::new(HashMap::new()),
            state_machine: RwLock::new(HashMap::new()),
        }
    }

    pub fn last_log_index_and_term(&self) -> (usize, u64) {
        let log = self.log.read().unwrap();
        if let Some(last) = log.last() {
            (last.index, last.term)
        } else {
            (0, 0)
        }
    }

    /// Starts a leader election by incrementing term, transitioning to Candidate, and voting for self.
    pub fn start_election(&self) -> (u64, Vec<RequestVoteArgs>) {
        let mut role = self.role.write().unwrap();
        *role = RaftRole::Candidate;

        let mut term = self.current_term.write().unwrap();
        *term += 1;
        let candidate_term = *term;

        let mut voted = self.voted_for.write().unwrap();
        *voted = Some(self.id.clone());

        let mut votes = self.votes_received.write().unwrap();
        votes.clear();
        votes.insert(self.id.clone(), true);

        let (last_idx, last_term) = self.last_log_index_and_term();

        let reqs = self
            .peers
            .iter()
            .map(|_peer| RequestVoteArgs {
                term: candidate_term,
                candidate_id: self.id.clone(),
                last_log_index: last_idx,
                last_log_term: last_term,
            })
            .collect();

        (candidate_term, reqs)
    }

    /// Handles an incoming RequestVote RPC.
    pub fn handle_request_vote(&self, args: &RequestVoteArgs) -> RequestVoteReply {
        let mut current_term = self.current_term.write().unwrap();
        let mut voted_for = self.voted_for.write().unwrap();
        let mut role = self.role.write().unwrap();

        if args.term > *current_term {
            *current_term = args.term;
            *role = RaftRole::Follower;
            *voted_for = None;
        }

        let mut vote_granted = false;
        if args.term == *current_term {
            let can_vote = match &*voted_for {
                None => true,
                Some(cand) => cand == &args.candidate_id,
            };

            if can_vote {
                let (last_idx, last_term) = self.last_log_index_and_term();
                let log_ok = args.last_log_term > last_term
                    || (args.last_log_term == last_term && args.last_log_index >= last_idx);

                if log_ok {
                    vote_granted = true;
                    *voted_for = Some(args.candidate_id.clone());
                }
            }
        }

        RequestVoteReply {
            term: *current_term,
            vote_granted,
        }
    }

    /// Records a received vote response and transitions to Leader if majority quorum achieved.
    pub fn receive_vote(&self, from_node: &str, reply_term: u64, vote_granted: bool) -> bool {
        let mut current_term = self.current_term.write().unwrap();
        let mut role = self.role.write().unwrap();

        if reply_term > *current_term {
            *current_term = reply_term;
            *role = RaftRole::Follower;
            *self.voted_for.write().unwrap() = None;
            return false;
        }

        if *role == RaftRole::Candidate && reply_term == *current_term && vote_granted {
            let mut votes = self.votes_received.write().unwrap();
            votes.insert(from_node.to_string(), true);

            let total_nodes = self.peers.len() + 1;
            let quorum = total_nodes / 2 + 1;

            if votes.len() >= quorum {
                *role = RaftRole::Leader;

                // Initialize next_index and match_index for all peers
                let (last_idx, _) = self.last_log_index_and_term();
                let mut next_idx = self.next_index.write().unwrap();
                let mut match_idx = self.match_index.write().unwrap();
                for peer in &self.peers {
                    next_idx.insert(peer.clone(), last_idx + 1);
                    match_idx.insert(peer.clone(), 0);
                }
                return true;
            }
        }

        false
    }

    /// Handles an incoming AppendEntries RPC (heartbeat or log replication).
    pub fn handle_append_entries(&self, args: &AppendEntriesArgs) -> AppendEntriesReply {
        let mut current_term = self.current_term.write().unwrap();
        let mut role = self.role.write().unwrap();
        let mut voted_for = self.voted_for.write().unwrap();

        if args.term > *current_term {
            *current_term = args.term;
            *role = RaftRole::Follower;
            *voted_for = None;
        } else if args.term == *current_term && *role != RaftRole::Follower {
            *role = RaftRole::Follower;
        }

        if args.term < *current_term {
            return AppendEntriesReply {
                term: *current_term,
                success: false,
                match_index: 0,
            };
        }

        let mut log = self.log.write().unwrap();

        // Check if log contains entry at prev_log_index matching prev_log_term
        if args.prev_log_index > 0 {
            if args.prev_log_index > log.len() {
                return AppendEntriesReply {
                    term: *current_term,
                    success: false,
                    match_index: log.len(),
                };
            }
            if log[args.prev_log_index - 1].term != args.prev_log_term {
                return AppendEntriesReply {
                    term: *current_term,
                    success: false,
                    match_index: args.prev_log_index - 1,
                };
            }
        }

        // Append any new entries not already in the log
        let mut insert_idx = args.prev_log_index;
        for entry in &args.entries {
            if insert_idx < log.len() {
                if log[insert_idx].term != entry.term {
                    log.truncate(insert_idx);
                    log.push(entry.clone());
                }
            } else {
                log.push(entry.clone());
            }
            insert_idx += 1;
        }

        let match_index = log.len();

        // Advance commit index if leader_commit > local commit_index
        let mut commit_idx = self.commit_index.write().unwrap();
        if args.leader_commit > *commit_idx {
            *commit_idx = args.leader_commit.min(match_index);
            self.apply_committed_entries(&log, *commit_idx);
        }

        AppendEntriesReply {
            term: *current_term,
            success: true,
            match_index,
        }
    }

    /// Proposes a new command to the replicated log. Only the leader can accept proposals.
    pub fn propose(&self, command: &str) -> Result<usize, String> {
        let role = *self.role.read().unwrap();
        if role != RaftRole::Leader {
            return Err("Cannot propose command: node is not the leader".to_string());
        }

        let term = *self.current_term.read().unwrap();
        let mut log = self.log.write().unwrap();
        let new_index = log.len() + 1;

        let entry = LogEntry {
            index: new_index,
            term,
            command: command.to_string(),
        };
        log.push(entry);

        // If single-node cluster, commit immediately
        if self.peers.is_empty() {
            let mut commit_idx = self.commit_index.write().unwrap();
            *commit_idx = new_index;
            self.apply_committed_entries(&log, new_index);
        }

        Ok(new_index)
    }

    /// Records replication progress on a peer node and advances leader commit index if majority reached.
    pub fn record_peer_replication(&self, peer: &str, match_idx: usize) -> bool {
        let role = *self.role.read().unwrap();
        if role != RaftRole::Leader {
            return false;
        }

        {
            let mut matches = self.match_index.write().unwrap();
            matches.insert(peer.to_string(), match_idx);
            let mut nexts = self.next_index.write().unwrap();
            nexts.insert(peer.to_string(), match_idx + 1);
        }

        let log = self.log.read().unwrap();
        let current_term = *self.current_term.read().unwrap();
        let mut commit_idx = self.commit_index.write().unwrap();

        // Check if there is an entry N > commit_idx replicated on a majority
        let mut new_commit = *commit_idx;
        for n in (*commit_idx + 1)..=log.len() {
            if log[n - 1].term == current_term {
                let mut match_count = 1; // leader itself matches
                let matches = self.match_index.read().unwrap();
                for &idx in matches.values() {
                    if idx >= n {
                        match_count += 1;
                    }
                }
                let total_nodes = self.peers.len() + 1;
                if match_count > total_nodes / 2 {
                    new_commit = n;
                }
            }
        }

        if new_commit > *commit_idx {
            *commit_idx = new_commit;
            self.apply_committed_entries(&log, new_commit);
            return true;
        }

        false
    }

    fn apply_committed_entries(&self, log: &[LogEntry], commit_target: usize) {
        let mut last_applied = self.last_applied.write().unwrap();
        let mut sm = self.state_machine.write().unwrap();

        while *last_applied < commit_target && *last_applied < log.len() {
            let entry = &log[*last_applied];
            apply_command_to_sm(&mut sm, &entry.command);
            *last_applied += 1;
        }
    }

    pub fn get_kv(&self, key: &str) -> Option<String> {
        let sm = self.state_machine.read().unwrap();
        sm.get(key).cloned()
    }
}

fn apply_command_to_sm(sm: &mut HashMap<String, String>, cmd: &str) {
    let mut parts = cmd.split_whitespace();
    if let Some(op) = parts.next() {
        match op.to_uppercase().as_str() {
            "SET" => {
                if let Some(key) = parts.next() {
                    let val = parts.collect::<Vec<&str>>().join(" ");
                    sm.insert(key.to_string(), val);
                }
            }
            "DEL" | "DELETE" => {
                if let Some(key) = parts.next() {
                    sm.remove(key);
                }
            }
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Global Registry for Active Raft Consensus Nodes
// -----------------------------------------------------------------------------

static RAFT_REGISTRY: OnceLock<Mutex<HashMap<usize, Arc<RaftNode>>>> = OnceLock::new();
static NEXT_RAFT_ID: AtomicUsize = AtomicUsize::new(1);

fn get_raft_registry() -> &'static Mutex<HashMap<usize, Arc<RaftNode>>> {
    RAFT_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:raft & std:cluster:raft
// -----------------------------------------------------------------------------

pub fn build_raft_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("create", raft_create),
        ("start_election", raft_start_election),
        ("receive_vote", raft_receive_vote),
        ("handle_request_vote", raft_handle_request_vote),
        ("handle_append_entries", raft_handle_append_entries),
        ("propose", raft_propose),
        ("record_replication", raft_record_replication),
        ("get_state", raft_get_state),
        ("get_kv", raft_get_kv),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("raft.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn raft_create(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.create", 2, &args, span)?;
    let id = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("raft.create: expected string id".to_string()).at(span)),
    };
    let peers = match &args[1] {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let mut list = Vec::new();
            for item in arr.iter() {
                if let Value::String(s) = item {
                    list.push(s.clone());
                }
            }
            list
        }
        _ => return Err(RuntimeError::new("raft.create: expected peers array".to_string()).at(span)),
    };

    let node = Arc::new(RaftNode::new(id, peers));
    let handle = NEXT_RAFT_ID.fetch_add(1, Ordering::SeqCst);
    get_raft_registry().lock().unwrap().insert(handle, node);

    let mut map = IndexMap::new();
    map.insert("handle".to_string(), Value::Int(handle as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn get_raft_from_val(v: &Value, span: Span) -> Result<Arc<RaftNode>, RuntimeError> {
    let handle = match v {
        Value::Int(i) => *i as usize,
        Value::Map(m) => {
            let map = m.read().unwrap();
            match map.get("handle") {
                Some(Value::Int(i)) => *i as usize,
                _ => return Err(RuntimeError::new("expected raft handle".to_string()).at(span)),
            }
        }
        _ => return Err(RuntimeError::new("expected raft handle or map".to_string()).at(span)),
    };

    let reg = get_raft_registry().lock().unwrap();
    reg.get(&handle)
        .cloned()
        .ok_or_else(|| RuntimeError::new(format!("raft node {} not found", handle)).at(span))
}

fn raft_start_election(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.start_election", 1, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let (term, reqs) = node.start_election();

    let mut map = IndexMap::new();
    map.insert("term".to_string(), Value::Int(term as i64));
    let req_vals: Vec<Value> = reqs
        .into_iter()
        .map(|r| {
            let mut rm = IndexMap::new();
            rm.insert("term".to_string(), Value::Int(r.term as i64));
            rm.insert("candidate_id".to_string(), Value::String(r.candidate_id));
            rm.insert("last_log_index".to_string(), Value::Int(r.last_log_index as i64));
            rm.insert("last_log_term".to_string(), Value::Int(r.last_log_term as i64));
            Value::Map(Arc::new(RwLock::new(rm)))
        })
        .collect();
    map.insert("requests".to_string(), Value::Array(Arc::new(RwLock::new(req_vals))));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn raft_receive_vote(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.receive_vote", 4, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let from = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string from_node".to_string()).at(span)),
    };
    let term = match &args[2] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };
    let granted = match &args[3] {
        Value::Bool(b) => *b,
        _ => false,
    };

    let is_leader = node.receive_vote(from, term, granted);
    Ok(Value::Bool(is_leader))
}

fn raft_handle_request_vote(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.handle_request_vote", 5, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let term = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };
    let candidate_id = match &args[2] {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };
    let last_log_index = match &args[3] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };
    let last_log_term = match &args[4] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };

    let req = RequestVoteArgs {
        term,
        candidate_id,
        last_log_index,
        last_log_term,
    };

    let reply = node.handle_request_vote(&req);
    let mut map = IndexMap::new();
    map.insert("term".to_string(), Value::Int(reply.term as i64));
    map.insert("vote_granted".to_string(), Value::Bool(reply.vote_granted));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn raft_handle_append_entries(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.handle_append_entries", 7, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let term = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };
    let leader_id = match &args[2] {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };
    let prev_log_index = match &args[3] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };
    let prev_log_term = match &args[4] {
        Value::Int(i) if *i >= 0 => *i as u64,
        _ => 0,
    };
    let entries = match &args[5] {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let mut list = Vec::new();
            for item in arr.iter() {
                if let Value::Map(m) = item {
                    let map = m.read().unwrap();
                    let idx = map.get("index").and_then(|v| match v { Value::Int(i) => Some(*i as usize), _ => None }).unwrap_or(0);
                    let trm = map.get("term").and_then(|v| match v { Value::Int(i) => Some(*i as u64), _ => None }).unwrap_or(0);
                    let cmd = map.get("command").and_then(|v| match v { Value::String(s) => Some(s.clone()), _ => None }).unwrap_or_default();
                    list.push(LogEntry { index: idx, term: trm, command: cmd });
                }
            }
            list
        }
        _ => Vec::new(),
    };
    let leader_commit = match &args[6] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };

    let req = AppendEntriesArgs {
        term,
        leader_id,
        prev_log_index,
        prev_log_term,
        entries,
        leader_commit,
    };

    let reply = node.handle_append_entries(&req);
    let mut map = IndexMap::new();
    map.insert("term".to_string(), Value::Int(reply.term as i64));
    map.insert("success".to_string(), Value::Bool(reply.success));
    map.insert("match_index".to_string(), Value::Int(reply.match_index as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn raft_propose(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.propose", 2, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let cmd = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string command".to_string()).at(span)),
    };

    node.propose(cmd)
        .map(|idx| Value::Int(idx as i64))
        .map_err(|e| RuntimeError::new(format!("raft.propose: {}", e)).at(span))
}

fn raft_record_replication(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.record_replication", 3, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let peer = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string peer_id".to_string()).at(span)),
    };
    let match_idx = match &args[2] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };

    let advanced = node.record_peer_replication(peer, match_idx);
    Ok(Value::Bool(advanced))
}

fn raft_get_state(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.get_state", 1, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;

    let mut map = IndexMap::new();
    map.insert("id".to_string(), Value::String(node.id.clone()));
    let role = *node.role.read().unwrap();
    map.insert("role".to_string(), Value::String(role.as_str().to_string()));
    map.insert("is_leader".to_string(), Value::Bool(role == RaftRole::Leader));
    map.insert("term".to_string(), Value::Int(*node.current_term.read().unwrap() as i64));
    map.insert("commit_index".to_string(), Value::Int(*node.commit_index.read().unwrap() as i64));
    map.insert("last_applied".to_string(), Value::Int(*node.last_applied.read().unwrap() as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn raft_get_kv(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("raft.get_kv", 2, &args, span)?;
    let node = get_raft_from_val(&args[0], span)?;
    let key = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("expected string key".to_string()).at(span)),
    };

    match node.get_kv(key) {
        Some(val) => Ok(Value::String(val)),
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
    fn test_raft_election_and_quorum() {
        let node1 = RaftNode::new("node1".into(), vec!["node2".into(), "node3".into()]);
        let node2 = RaftNode::new("node2".into(), vec!["node1".into(), "node3".into()]);

        // Node1 starts election
        let (term, reqs) = node1.start_election();
        assert_eq!(term, 1);
        assert_eq!(*node1.role.read().unwrap(), RaftRole::Candidate);
        assert_eq!(reqs.len(), 2);

        // Node2 handles vote request from Node1
        let reply = node2.handle_request_vote(&reqs[0]);
        assert_eq!(reply.term, 1);
        assert!(reply.vote_granted);

        // Node1 receives granted vote from Node2 (2 out of 3 is majority quorum)
        let is_leader = node1.receive_vote("node2", 1, true);
        assert!(is_leader);
        assert_eq!(*node1.role.read().unwrap(), RaftRole::Leader);
    }

    #[test]
    fn test_raft_replicated_log_and_state_machine() {
        let leader = RaftNode::new("n1".into(), vec!["n2".into()]);
        let follower = RaftNode::new("n2".into(), vec!["n1".into()]);

        // Force n1 to leader
        *leader.role.write().unwrap() = RaftRole::Leader;
        *leader.current_term.write().unwrap() = 1;

        // Propose command to leader
        let idx = leader.propose("SET user:42 Alice").expect("proposal succeeds");
        assert_eq!(idx, 1);

        // Follower receives AppendEntries
        let log = leader.log.read().unwrap();
        let entries = log.clone();
        let reply = follower.handle_append_entries(&AppendEntriesArgs {
            term: 1,
            leader_id: "n1".into(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries,
            leader_commit: 0,
        });
        assert!(reply.success);
        assert_eq!(reply.match_index, 1);

        // Leader records replication on follower (majority 2/2 -> commit!)
        let advanced = leader.record_peer_replication("n2", 1);
        assert!(advanced);
        assert_eq!(*leader.commit_index.read().unwrap(), 1);

        // Value must be applied in leader state machine
        assert_eq!(leader.get_kv("user:42"), Some("Alice".to_string()));
    }
}
