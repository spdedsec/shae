//! Distributed Actor Mesh (`src/actor_cluster.rs`)
//!
//! Provides TCP-based network clustering with cross-node actor discovery,
//! message routing (asynchronous send and synchronous ask), request-response correlation,
//! broadcast channels, and Erlang/OTP-style cluster node meshes.

use crate::actor::{ActorError, ActorRef, ActorRegistry};
use crate::ast::Span;
use crate::channel::Channel;
use crate::eval::Evaluator;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

static NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);

/// Binary wire framing: 4-byte big-endian payload length followed by bytes.
pub fn write_frame<W: Write>(writer: &mut W, data: &[u8]) -> std::io::Result<()> {
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(data)?;
    writer.flush()?;
    Ok(())
}

/// Binary wire frame reading with a 64MB protection threshold.
pub fn read_frame<R: Read>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Frame size exceeds 64MB limit",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

/// Wire protocol envelopes exchanged across cluster nodes over TCP.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClusterWireMsg {
    Handshake {
        node: String,
        address: String,
        cookie: Option<String>,
        actors: Vec<String>,
    },
    HandshakeAck {
        node: String,
        address: String,
        actors: Vec<String>,
    },
    HandshakeReject {
        reason: String,
    },
    Ping,
    Pong,
    Send {
        from_node: String,
        target: String,
        payload: serde_json::Value,
    },
    Ask {
        request_id: String,
        from_node: String,
        target: String,
        payload: serde_json::Value,
        timeout_ms: u64,
    },
    Reply {
        request_id: String,
        status: String, // "ok" or "err"
        payload: Option<serde_json::Value>,
        error: Option<String>,
    },
    RegisterUpdate {
        action: String, // "register" or "unregister"
        actor: String,
    },
    Broadcast {
        from_node: String,
        topic: String,
        payload: serde_json::Value,
    },
}

/// Active connection to an established remote cluster peer.
pub struct PeerConnection {
    pub node_name: String,
    pub address: String,
    pub writer: Arc<Mutex<TcpStream>>,
    pub known_actors: Arc<RwLock<HashSet<String>>>,
    pub is_alive: Arc<AtomicBool>,
}

impl PeerConnection {
    pub fn send_msg(&self, msg: &ClusterWireMsg) -> Result<(), ActorError> {
        if !self.is_alive.load(Ordering::Relaxed) {
            return Err(ActorError::ActorNotAlive);
        }
        let serialized = serde_json::to_vec(msg).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to serialize wire message: {}", e))
        })?;
        let mut guard = self.writer.lock().map_err(|_| {
            ActorError::EvaluationError("Peer connection write lock poisoned".into())
        })?;
        write_frame(&mut *guard, &serialized).map_err(|e| {
            self.is_alive.store(false, Ordering::Relaxed);
            ActorError::EvaluationError(format!("Failed to write frame to peer: {}", e))
        })
    }

    pub fn close(&self) {
        self.is_alive.store(false, Ordering::Relaxed);
        if let Ok(guard) = self.writer.lock() {
            let _ = guard.shutdown(Shutdown::Both);
        }
    }
}

/// Handle to a remote actor residing on a clustered node.
#[derive(Clone)]
pub struct RemoteActorRef {
    pub node_name: String,
    pub node_address: Option<String>,
    pub actor_name: String,
    pub cluster_node: Arc<ClusterNode>,
}

impl RemoteActorRef {
    pub fn new(
        node_name: String,
        node_address: Option<String>,
        actor_name: String,
        cluster_node: Arc<ClusterNode>,
    ) -> Self {
        Self {
            node_name,
            node_address,
            actor_name,
            cluster_node,
        }
    }

    pub fn send(&self, msg: Value) -> Result<(), ActorError> {
        self.cluster_node
            .send_to_remote(&self.node_name, &self.actor_name, msg)
    }

    pub fn ask(&self, msg: Value, timeout: Option<Duration>) -> Result<Value, ActorError> {
        self.cluster_node
            .ask_remote(&self.node_name, &self.actor_name, msg, timeout)
    }

    pub fn is_alive(&self) -> bool {
        self.cluster_node.is_peer_connected(&self.node_name)
    }

    pub fn node(&self) -> &str {
        &self.node_name
    }

    pub fn name(&self) -> &str {
        &self.actor_name
    }
}

impl fmt::Debug for RemoteActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemoteActorRef({}@{})", self.actor_name, self.node_name)
    }
}

impl fmt::Display for RemoteActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<remote_actor:{}@{}>", self.actor_name, self.node_name)
    }
}

impl PartialEq for RemoteActorRef {
    fn eq(&self, other: &Self) -> bool {
        self.node_name == other.node_name && self.actor_name == other.actor_name
    }
}

impl Eq for RemoteActorRef {}

impl Hash for RemoteActorRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node_name.hash(state);
        self.actor_name.hash(state);
    }
}

/// An active node in the distributed actor mesh.
pub struct ClusterNode {
    pub name: String,
    pub bind_address: String,
    actual_address: RwLock<String>,
    pub cookie: Option<String>,
    is_stopped: Arc<AtomicBool>,
    listener_thread: Mutex<Option<JoinHandle<()>>>,
    pub peers: Arc<RwLock<HashMap<String, Arc<PeerConnection>>>>,
    pub local_actors: Arc<RwLock<HashMap<String, Arc<ActorRef>>>>,
    pub pending_requests: Arc<RwLock<HashMap<String, Arc<Channel>>>>,
    pub broadcast_subscribers: Arc<RwLock<Vec<Value>>>,
}

impl ClusterNode {
    /// Starts a new cluster node bound to the specified TCP address.
    pub fn start(
        name: impl Into<String>,
        address: impl Into<String>,
        cookie: Option<String>,
    ) -> Result<Arc<Self>, ActorError> {
        let name_str = name.into();
        let addr_str = address.into();
        let listener = TcpListener::bind(&addr_str).map_err(|e| {
            ActorError::SpawnError(
                name_str.clone(),
                format!(
                    "Failed to bind cluster TCP listener to '{}': {}",
                    addr_str, e
                ),
            )
        })?;

        let bound_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| addr_str.clone());

        let node = Arc::new(Self {
            name: name_str,
            bind_address: addr_str,
            actual_address: RwLock::new(bound_addr),
            cookie,
            is_stopped: Arc::new(AtomicBool::new(false)),
            listener_thread: Mutex::new(None),
            peers: Arc::new(RwLock::new(HashMap::new())),
            local_actors: Arc::new(RwLock::new(HashMap::new())),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            broadcast_subscribers: Arc::new(RwLock::new(Vec::new())),
        });

        // Spawn accept loop
        let node_clone = Arc::clone(&node);
        let handle = thread::Builder::new()
            .name(format!("cluster-accept-{}", node.name))
            .spawn(move || {
                node_clone.run_accept_loop(listener);
            })
            .map_err(|e| {
                ActorError::SpawnError(
                    node.name.clone(),
                    format!("Failed to spawn accept loop: {}", e),
                )
            })?;

        if let Ok(mut g) = node.listener_thread.lock() {
            *g = Some(handle);
        }

        Ok(node)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn address(&self) -> String {
        self.actual_address.read().unwrap().clone()
    }

    pub fn is_alive(&self) -> bool {
        !self.is_stopped.load(Ordering::Relaxed)
    }

    pub fn is_peer_connected(&self, peer_name: &str) -> bool {
        let guard = self.peers.read().unwrap();
        guard
            .get(peer_name)
            .map(|p| p.is_alive.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Connects to a remote peer node and performs handshake exchange.
    pub fn connect(&self, peer_addr: &str) -> Result<Arc<PeerConnection>, ActorError> {
        if self.is_stopped.load(Ordering::Relaxed) {
            return Err(ActorError::ActorStopped);
        }

        let mut stream = TcpStream::connect(peer_addr).map_err(|e| {
            ActorError::EvaluationError(format!(
                "Failed to connect to cluster peer at '{}': {}",
                peer_addr, e
            ))
        })?;

        // 1. Send Handshake
        let local_registered = self.registered();
        let handshake = ClusterWireMsg::Handshake {
            node: self.name.clone(),
            address: self.address(),
            cookie: self.cookie.clone(),
            actors: local_registered,
        };
        let bytes = serde_json::to_vec(&handshake).map_err(|e| {
            ActorError::EvaluationError(format!("Handshake serialization failed: {}", e))
        })?;
        write_frame(&mut stream, &bytes).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to write handshake frame: {}", e))
        })?;

        // 2. Await HandshakeAck or HandshakeReject
        let reply_bytes = read_frame(&mut stream).map_err(|e| {
            ActorError::EvaluationError(format!(
                "Failed to read handshake response from peer: {}",
                e
            ))
        })?;
        let reply_msg: ClusterWireMsg = serde_json::from_slice(&reply_bytes).map_err(|e| {
            ActorError::EvaluationError(format!(
                "Failed to parse handshake response from peer: {}",
                e
            ))
        })?;

        let (peer_node, peer_actors) = match reply_msg {
            ClusterWireMsg::HandshakeAck { node, actors, .. } => (node, actors),
            ClusterWireMsg::HandshakeReject { reason } => {
                return Err(ActorError::EvaluationError(format!(
                    "Peer rejected handshake: {}",
                    reason
                )));
            }
            other => {
                return Err(ActorError::EvaluationError(format!(
                    "Unexpected response to handshake: {:?}",
                    other
                )));
            }
        };

        let stream_read = stream.try_clone().map_err(|e| {
            ActorError::EvaluationError(format!("Failed to clone TCP stream for reader: {}", e))
        })?;

        let peer = Arc::new(PeerConnection {
            node_name: peer_node.clone(),
            address: peer_addr.to_string(),
            writer: Arc::new(Mutex::new(stream)),
            known_actors: Arc::new(RwLock::new(peer_actors.into_iter().collect())),
            is_alive: Arc::new(AtomicBool::new(true)),
        });

        {
            let mut peers = self.peers.write().unwrap();
            peers.insert(peer_node.clone(), Arc::clone(&peer));
        }

        // Spawn reader thread for peer
        self.spawn_peer_reader(stream_read, Arc::clone(&peer));

        Ok(peer)
    }

    /// Returns list of connected peer node names.
    pub fn peers(&self) -> Vec<String> {
        let guard = self.peers.read().unwrap();
        guard
            .iter()
            .filter(|(_, p)| p.is_alive.load(Ordering::Relaxed))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Spawns a named actor locally on this node and registers it with the cluster.
    pub fn spawn_actor(
        &self,
        name: &str,
        initial_state: Option<Value>,
        handler: Value,
    ) -> Result<Arc<ActorRef>, ActorError> {
        let actor = ActorRef::spawn_named(name, initial_state, handler)?;
        self.register(name, Arc::clone(&actor));
        Ok(actor)
    }

    /// Registers a local actor in the node cluster directory.
    pub fn register(&self, name: &str, actor: Arc<ActorRef>) {
        {
            let mut local = self.local_actors.write().unwrap();
            local.insert(name.to_string(), Arc::clone(&actor));
        }
        let _ = ActorRegistry::register(name, actor);

        // Notify connected peers
        let update = ClusterWireMsg::RegisterUpdate {
            action: "register".into(),
            actor: name.to_string(),
        };
        let peers = self.peers.read().unwrap();
        for peer in peers.values() {
            let _ = peer.send_msg(&update);
        }
    }

    /// Unregisters an actor from the node cluster directory.
    pub fn unregister(&self, name: &str) {
        {
            let mut local = self.local_actors.write().unwrap();
            local.remove(name);
        }
        let _ = ActorRegistry::unregister(name);

        let update = ClusterWireMsg::RegisterUpdate {
            action: "unregister".into(),
            actor: name.to_string(),
        };
        let peers = self.peers.read().unwrap();
        for peer in peers.values() {
            let _ = peer.send_msg(&update);
        }
    }

    /// Returns a list of all locally registered actor names.
    pub fn registered(&self) -> Vec<String> {
        let local = self.local_actors.read().unwrap();
        local.keys().cloned().collect()
    }

    /// Obtains a RemoteActorRef handle targeting an actor on a peer node.
    pub fn actor(&self, name: &str, peer_node: &str) -> Arc<RemoteActorRef> {
        Arc::new(RemoteActorRef::new(
            peer_node.to_string(),
            None,
            name.to_string(),
            Arc::new(self.clone_ref()),
        ))
    }

    /// Clones the Arc reference wrapper of self.
    fn clone_ref(&self) -> Self {
        Self {
            name: self.name.clone(),
            bind_address: self.bind_address.clone(),
            actual_address: RwLock::new(self.actual_address.read().unwrap().clone()),
            cookie: self.cookie.clone(),
            is_stopped: Arc::clone(&self.is_stopped),
            listener_thread: Mutex::new(None),
            peers: Arc::clone(&self.peers),
            local_actors: Arc::clone(&self.local_actors),
            pending_requests: Arc::clone(&self.pending_requests),
            broadcast_subscribers: Arc::clone(&self.broadcast_subscribers),
        }
    }

    /// Resolves an actor across the cluster: checks local node first, then connected peers.
    pub fn whereis(self: &Arc<Self>, name: &str, node_hint: Option<&str>) -> Option<Value> {
        if let Some(target_node) = node_hint {
            if target_node == self.name {
                let local = self.local_actors.read().unwrap();
                return local.get(name).map(|a| Value::Actor(Arc::clone(a)));
            }
            return Some(Value::RemoteActor(Arc::new(RemoteActorRef::new(
                target_node.to_string(),
                None,
                name.to_string(),
                Arc::clone(self),
            ))));
        }

        // Check local
        {
            let local = self.local_actors.read().unwrap();
            if let Some(act) = local.get(name) {
                return Some(Value::Actor(Arc::clone(act)));
            }
        }

        // Check peers known actors
        let peers = self.peers.read().unwrap();
        for (peer_name, peer) in peers.iter() {
            if peer.is_alive.load(Ordering::Relaxed) {
                let known = peer.known_actors.read().unwrap();
                if known.contains(name) {
                    return Some(Value::RemoteActor(Arc::new(RemoteActorRef::new(
                        peer_name.clone(),
                        Some(peer.address.clone()),
                        name.to_string(),
                        Arc::clone(self),
                    ))));
                }
            }
        }

        None
    }

    /// Sends a message to a remote actor on a peer node.
    pub fn send_to_remote(
        &self,
        peer_node: &str,
        target_actor: &str,
        msg: Value,
    ) -> Result<(), ActorError> {
        let peer = {
            let guard = self.peers.read().unwrap();
            guard.get(peer_node).cloned()
        };

        let peer = peer.ok_or_else(|| {
            ActorError::EvaluationError(format!("Node '{}' is not connected to cluster", peer_node))
        })?;

        let envelope = ClusterWireMsg::Send {
            from_node: self.name.clone(),
            target: target_actor.to_string(),
            payload: msg.to_json(),
        };

        peer.send_msg(&envelope)
    }

    /// Sends a request to a remote actor and awaits response.
    pub fn ask_remote(
        &self,
        peer_node: &str,
        target_actor: &str,
        msg: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, ActorError> {
        let peer = {
            let guard = self.peers.read().unwrap();
            guard.get(peer_node).cloned()
        };

        let peer = peer.ok_or_else(|| {
            ActorError::EvaluationError(format!("Node '{}' is not connected to cluster", peer_node))
        })?;

        let duration = timeout.unwrap_or(Duration::from_millis(5000));
        let timeout_ms = duration.as_millis() as u64;

        let req_id = format!(
            "req-{}-{}",
            NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let reply_chan = Channel::new(Some(1));
        {
            let mut pending = self.pending_requests.write().unwrap();
            pending.insert(req_id.clone(), Arc::clone(&reply_chan));
        }

        let envelope = ClusterWireMsg::Ask {
            request_id: req_id.clone(),
            from_node: self.name.clone(),
            target: target_actor.to_string(),
            payload: msg.to_json(),
            timeout_ms,
        };

        if let Err(e) = peer.send_msg(&envelope) {
            let mut pending = self.pending_requests.write().unwrap();
            pending.remove(&req_id);
            return Err(e);
        }

        let span = Span::new(1, 1);
        let reply = reply_chan.recv(Some(timeout_ms), span);

        // Cleanup pending table
        {
            let mut pending = self.pending_requests.write().unwrap();
            pending.remove(&req_id);
        }

        match reply {
            Ok(Value::Array(arr)) => {
                let items = arr.read().unwrap();
                if items.len() == 2 {
                    if let Value::String(ref tag) = items[0] {
                        if tag == "ok" {
                            return Ok(items[1].clone());
                        } else if tag == "err" {
                            let err_str = match &items[1] {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            return Err(ActorError::ActorFailed(err_str));
                        }
                    }
                }
                Ok(Value::Array(Arc::clone(&arr)))
            }
            Ok(Value::Null) => Err(ActorError::Timeout),
            Ok(other) => Ok(other),
            Err(e) => Err(ActorError::EvaluationError(e.message)),
        }
    }

    /// Broadcasts a message to all connected peers and local subscribers.
    pub fn broadcast(&self, topic: &str, msg: Value) {
        let envelope = ClusterWireMsg::Broadcast {
            from_node: self.name.clone(),
            topic: topic.to_string(),
            payload: msg.to_json(),
        };

        let peers = self.peers.read().unwrap();
        for peer in peers.values() {
            let _ = peer.send_msg(&envelope);
        }

        self.deliver_local_broadcast(topic, &msg);
    }

    /// Registers a Shae callback for broadcast messages.
    pub fn on_broadcast(&self, callback: Value) {
        let mut subs = self.broadcast_subscribers.write().unwrap();
        subs.push(callback);
    }

    fn deliver_local_broadcast(&self, topic: &str, msg: &Value) {
        let subs = {
            let guard = self.broadcast_subscribers.read().unwrap();
            guard.clone()
        };

        if subs.is_empty() {
            return;
        }

        let topic_val = Value::String(topic.to_string());
        let payload_val = msg.clone();

        thread::spawn(move || {
            let mut evaluator = Evaluator::new();
            let span = Span::new(1, 1);
            for cb in subs {
                let _ =
                    evaluator.call_value(&cb, vec![topic_val.clone(), payload_val.clone()], span);
            }
        });
    }

    /// Shuts down the cluster node, closing all peer connections and listener.
    pub fn stop(&self) {
        if self.is_stopped.swap(true, Ordering::SeqCst) {
            return;
        }

        // Stop and unregister all local actors
        {
            let mut local = self.local_actors.write().unwrap();
            for (name, actor) in local.drain() {
                actor.stop();
                let _ = ActorRegistry::unregister(&name);
            }
        }

        // Close all peer connections
        {
            let peers = self.peers.read().unwrap();
            for peer in peers.values() {
                peer.close();
            }
        }

        // Cancel all pending requests
        {
            let mut pending = self.pending_requests.write().unwrap();
            let span = Span::new(1, 1);
            for chan in pending.values() {
                let err_envelope = Value::Array(Arc::new(RwLock::new(vec![
                    Value::String("err".into()),
                    Value::String("Cluster node stopped".into()),
                ])));
                let _ = chan.send(err_envelope, span);
            }
            pending.clear();
        }

        // Wake listener by connecting locally
        let addr = self.address();
        let _ = TcpStream::connect(&addr);

        if let Ok(mut g) = self.listener_thread.lock() {
            if let Some(h) = g.take() {
                let _ = h.join();
            }
        }
    }

    fn run_accept_loop(self: Arc<Self>, listener: TcpListener) {
        for stream_res in listener.incoming() {
            if self.is_stopped.load(Ordering::Relaxed) {
                break;
            }

            let mut stream = match stream_res {
                Ok(s) => s,
                Err(_) => continue,
            };

            let node_self = Arc::clone(&self);
            thread::spawn(move || {
                node_self.handle_incoming_connection(&mut stream);
            });
        }
    }

    fn handle_incoming_connection(&self, stream: &mut TcpStream) {
        // Read Handshake
        let frame = match read_frame(stream) {
            Ok(f) => f,
            Err(_) => return,
        };

        let msg: ClusterWireMsg = match serde_json::from_slice(&frame) {
            Ok(m) => m,
            Err(_) => return,
        };

        let (peer_node, peer_addr, peer_cookie, peer_actors) = match msg {
            ClusterWireMsg::Handshake {
                node,
                address,
                cookie,
                actors,
            } => (node, address, cookie, actors),
            _ => {
                let reject = ClusterWireMsg::HandshakeReject {
                    reason: "Expected Handshake message".into(),
                };
                if let Ok(bytes) = serde_json::to_vec(&reject) {
                    let _ = write_frame(stream, &bytes);
                }
                return;
            }
        };

        // Validate auth cookie if configured
        if let Some(ref expected_cookie) = self.cookie {
            if peer_cookie.as_ref() != Some(expected_cookie) {
                let reject = ClusterWireMsg::HandshakeReject {
                    reason: "Authentication cookie mismatch".into(),
                };
                if let Ok(bytes) = serde_json::to_vec(&reject) {
                    let _ = write_frame(stream, &bytes);
                }
                return;
            }
        }

        let stream_read = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

        let stream_write = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

        let peer = Arc::new(PeerConnection {
            node_name: peer_node.clone(),
            address: peer_addr,
            writer: Arc::new(Mutex::new(stream_write)),
            known_actors: Arc::new(RwLock::new(peer_actors.into_iter().collect())),
            is_alive: Arc::new(AtomicBool::new(true)),
        });

        {
            let mut peers = self.peers.write().unwrap();
            peers.insert(peer_node, Arc::clone(&peer));
        }

        self.spawn_peer_reader(stream_read, Arc::clone(&peer));

        // Reply with HandshakeAck
        let ack = ClusterWireMsg::HandshakeAck {
            node: self.name.clone(),
            address: self.address(),
            actors: self.registered(),
        };
        let ack_bytes = match serde_json::to_vec(&ack) {
            Ok(b) => b,
            Err(_) => return,
        };
        let _ = write_frame(stream, &ack_bytes);
    }

    fn spawn_peer_reader(&self, mut reader_stream: TcpStream, peer: Arc<PeerConnection>) {
        let node_self = Arc::new(self.clone_ref());
        let peer_clone = Arc::clone(&peer);

        thread::Builder::new()
            .name(format!("peer-reader-{}", peer.node_name))
            .spawn(move || {
                while !node_self.is_stopped.load(Ordering::Relaxed)
                    && peer_clone.is_alive.load(Ordering::Relaxed)
                {
                    let frame = match read_frame(&mut reader_stream) {
                        Ok(f) => f,
                        Err(_) => break, // Peer closed or connection broke
                    };

                    let msg: ClusterWireMsg = match serde_json::from_slice(&frame) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    match msg {
                        ClusterWireMsg::Ping => {
                            let pong = ClusterWireMsg::Pong;
                            let _ = peer_clone.send_msg(&pong);
                        }
                        ClusterWireMsg::Pong => {}
                        ClusterWireMsg::Send {
                            target, payload, ..
                        } => {
                            let value = Value::from_json(&payload);
                            let local_opt = {
                                let local = node_self.local_actors.read().unwrap();
                                local.get(&target).cloned()
                            };
                            let actor_target =
                                local_opt.or_else(|| ActorRegistry::whereis(&target));
                            if let Some(act) = actor_target {
                                let _ = act.send(value);
                            }
                        }
                        ClusterWireMsg::Ask {
                            request_id,
                            target,
                            payload,
                            timeout_ms,
                            ..
                        } => {
                            let value = Value::from_json(&payload);
                            let local_opt = {
                                let local = node_self.local_actors.read().unwrap();
                                local.get(&target).cloned()
                            };
                            let actor_target =
                                local_opt.or_else(|| ActorRegistry::whereis(&target));
                            let peer_reply = Arc::clone(&peer_clone);

                            thread::spawn(move || match actor_target {
                                Some(act) => {
                                    match act.ask(value, Some(Duration::from_millis(timeout_ms))) {
                                        Ok(res) => {
                                            let reply = ClusterWireMsg::Reply {
                                                request_id,
                                                status: "ok".into(),
                                                payload: Some(res.to_json()),
                                                error: None,
                                            };
                                            let _ = peer_reply.send_msg(&reply);
                                        }
                                        Err(e) => {
                                            let reply = ClusterWireMsg::Reply {
                                                request_id,
                                                status: "err".into(),
                                                payload: None,
                                                error: Some(e.to_string()),
                                            };
                                            let _ = peer_reply.send_msg(&reply);
                                        }
                                    }
                                }
                                None => {
                                    let reply = ClusterWireMsg::Reply {
                                        request_id,
                                        status: "err".into(),
                                        payload: None,
                                        error: Some(format!("Actor '{}' not found", target)),
                                    };
                                    let _ = peer_reply.send_msg(&reply);
                                }
                            });
                        }
                        ClusterWireMsg::Reply {
                            request_id,
                            status,
                            payload,
                            error,
                        } => {
                            let chan_opt = {
                                let mut pending = node_self.pending_requests.write().unwrap();
                                pending.remove(&request_id)
                            };

                            if let Some(chan) = chan_opt {
                                let span = Span::new(1, 1);
                                if status == "ok" {
                                    let content = payload
                                        .map(|p| Value::from_json(&p))
                                        .unwrap_or(Value::Null);
                                    let envelope = Value::Array(Arc::new(RwLock::new(vec![
                                        Value::String("ok".into()),
                                        content,
                                    ])));
                                    let _ = chan.send(envelope, span);
                                } else {
                                    let err_msg = error.unwrap_or_else(|| "Unknown error".into());
                                    let envelope = Value::Array(Arc::new(RwLock::new(vec![
                                        Value::String("err".into()),
                                        Value::String(err_msg),
                                    ])));
                                    let _ = chan.send(envelope, span);
                                }
                            }
                        }
                        ClusterWireMsg::RegisterUpdate { action, actor } => {
                            let mut known = peer_clone.known_actors.write().unwrap();
                            if action == "register" {
                                known.insert(actor);
                            } else {
                                known.remove(&actor);
                            }
                        }
                        ClusterWireMsg::Broadcast { topic, payload, .. } => {
                            let val = Value::from_json(&payload);
                            node_self.deliver_local_broadcast(&topic, &val);
                        }
                        _ => {}
                    }
                }

                peer_clone.is_alive.store(false, Ordering::Relaxed);
                let mut peers = node_self.peers.write().unwrap();
                peers.remove(&peer_clone.node_name);
            })
            .unwrap();
    }
}

impl fmt::Debug for ClusterNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClusterNode({}@{})", self.name, self.address())
    }
}

impl fmt::Display for ClusterNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<cluster_node:{}@{}>", self.name, self.address())
    }
}

impl PartialEq for ClusterNode {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.address() == other.address()
    }
}

impl Eq for ClusterNode {}

impl Hash for ClusterNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.address().hash(state);
    }
}

// ---------------------------------------------------------------------------
// Global Default Cluster Singleton
// ---------------------------------------------------------------------------

static GLOBAL_CLUSTER: OnceLock<RwLock<Option<Arc<ClusterNode>>>> = OnceLock::new();

pub fn get_global_cluster() -> Option<Arc<ClusterNode>> {
    GLOBAL_CLUSTER
        .get_or_init(|| RwLock::new(None))
        .read()
        .ok()
        .and_then(|g| g.clone())
}

pub fn set_global_cluster(node: Arc<ClusterNode>) {
    let lock = GLOBAL_CLUSTER.get_or_init(|| RwLock::new(None));
    if let Ok(mut g) = lock.write() {
        *g = Some(node);
    }
}

pub fn clear_global_cluster() {
    if let Some(lock) = GLOBAL_CLUSTER.get() {
        if let Ok(mut g) = lock.write() {
            if let Some(ref node) = *g {
                node.stop();
            }
            *g = None;
        }
    }
}
