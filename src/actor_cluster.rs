//! Distributed Actor Mesh (`src/actor_cluster.rs`)
//!
//! Provides TCP/TLS-based network clustering with cross-node actor discovery,
//! mutual-TLS (mTLS) transport security, message routing (asynchronous send and synchronous ask),
//! request-response correlation, broadcast channels, and Erlang/OTP-style cluster node meshes.

use crate::actor::{ActorError, ActorRef, ActorRegistry};
use crate::ast::Span;
use crate::channel::Channel;
use crate::eval::Evaluator;
use crate::value::Value;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig, StreamOwned};
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

fn default_cluster_timeout_ms() -> u64 {
    5000
}

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

/// Adaptive wire frame reading for non-blocking worker loops.
/// Polls for the first byte using a short timeout (e.g. 2ms), then switches to
/// a standard 10s timeout to read the complete frame payload before restoring the short timeout.
pub fn read_frame_adaptive(transport: &mut ClusterTransport) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    let mut first = [0u8; 1];
    let n = transport.read(&mut first)?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "Connection closed by peer",
        ));
    }
    len_buf[0] = first[0];

    // Data is incoming: switch temporarily to 10s timeout to complete frame read
    let _ = transport.set_read_timeout(Some(Duration::from_secs(10)));
    let res = (|| -> std::io::Result<Vec<u8>> {
        transport.read_exact(&mut len_buf[1..4])?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Frame size exceeds 64MB limit",
            ));
        }
        let mut buf = vec![0u8; len];
        transport.read_exact(&mut buf)?;
        Ok(buf)
    })();

    // Restore 2ms timeout
    let _ = transport.set_read_timeout(Some(Duration::from_millis(2)));
    res
}

/// Wire protocol envelopes exchanged across cluster nodes over TCP/TLS.
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
    Reload {
        request_id: String,
        from_node: String,
        #[serde(alias = "target_actor")]
        target: String,
        #[serde(alias = "new_handler_code", alias = "code")]
        handler_code: String,
        #[serde(alias = "migration_code", default)]
        migration_code: Option<String>,
        #[serde(default = "default_cluster_timeout_ms")]
        timeout_ms: u64,
    },
}

// ---------------------------------------------------------------------------
// TLS & mTLS Transport Support
// ---------------------------------------------------------------------------

/// Loads certificates from an inline PEM string or a file path on disk.
pub fn load_certs(input: &str) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let mut reader: Box<dyn std::io::BufRead> = if input.contains("-----BEGIN CERTIFICATE-----") {
        Box::new(std::io::BufReader::new(input.as_bytes()))
    } else {
        Box::new(std::io::BufReader::new(std::fs::File::open(input)?))
    };
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Loads a private key from an inline PEM string or a file path on disk.
pub fn load_key(input: &str) -> std::io::Result<PrivateKeyDer<'static>> {
    let mut reader: Box<dyn std::io::BufRead> = if input.contains("PRIVATE KEY-----") {
        Box::new(std::io::BufReader::new(input.as_bytes()))
    } else {
        Box::new(std::io::BufReader::new(std::fs::File::open(input)?))
    };
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No private key found"))
}

/// Custom certificate verifier allowing unverified/self-signed server certificates (when verify_peer is false).
#[derive(Debug)]
pub struct NoCertVerification(pub Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// TLS & Mutual-TLS configuration for encrypted cluster meshes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClusterTlsConfig {
    /// Node certificate in PEM format or path to file
    pub cert: String,
    /// Node private key in PEM format or path to file
    pub key: String,
    /// Optional CA certificate in PEM format or path to file for verifying peers / client mTLS
    pub ca: Option<String>,
    /// Whether to require and verify peer certificates (defaults to true if ca is provided)
    pub verify_peer: Option<bool>,
    /// Optional domain name (SNI) to use when connecting to a remote peer (defaults to host or "localhost")
    pub server_name: Option<String>,
}

impl ClusterTlsConfig {
    pub fn build_server_config(&self) -> Result<Arc<ServerConfig>, ActorError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs = load_certs(&self.cert).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to load TLS server cert: {}", e))
        })?;
        let key = load_key(&self.key).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to load TLS server key: {}", e))
        })?;

        let should_verify_client =
            self.verify_peer.unwrap_or(self.ca.is_some()) && self.ca.is_some();

        let builder = if should_verify_client {
            let ca_str = self.ca.as_ref().unwrap();
            let ca_certs = load_certs(ca_str).map_err(|e| {
                ActorError::EvaluationError(format!("Failed to load TLS CA cert: {}", e))
            })?;
            let mut root_store = RootCertStore::empty();
            for c in ca_certs {
                root_store.add(c).map_err(|e| {
                    ActorError::EvaluationError(format!(
                        "Failed to add CA cert to root store: {}",
                        e
                    ))
                })?;
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
                .build()
                .map_err(|e| {
                    ActorError::EvaluationError(format!("Failed to build client verifier: {}", e))
                })?;
            ServerConfig::builder().with_client_cert_verifier(verifier)
        } else {
            ServerConfig::builder().with_no_client_auth()
        };

        let config = builder
            .with_single_cert(certs, key)
            .map_err(|e| ActorError::EvaluationError(format!("TLS server config error: {}", e)))?;

        Ok(Arc::new(config))
    }

    pub fn build_client_config(&self) -> Result<Arc<ClientConfig>, ActorError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let should_verify = self.verify_peer.unwrap_or(true);

        let builder = if let Some(ref ca_str) = self.ca {
            let ca_certs = load_certs(ca_str).map_err(|e| {
                ActorError::EvaluationError(format!("Failed to load TLS CA cert: {}", e))
            })?;
            let mut root_store = RootCertStore::empty();
            for c in ca_certs {
                root_store.add(c).map_err(|e| {
                    ActorError::EvaluationError(format!(
                        "Failed to add CA cert to root store: {}",
                        e
                    ))
                })?;
            }
            ClientConfig::builder().with_root_certificates(root_store)
        } else if !should_verify {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertVerification(provider)))
        } else {
            let root_store = RootCertStore::empty();
            ClientConfig::builder().with_root_certificates(root_store)
        };

        let client_config = if !self.cert.is_empty() && !self.key.is_empty() {
            let certs = load_certs(&self.cert).map_err(|e| {
                ActorError::EvaluationError(format!("Failed to load TLS client cert: {}", e))
            })?;
            let key = load_key(&self.key).map_err(|e| {
                ActorError::EvaluationError(format!("Failed to load TLS client key: {}", e))
            })?;
            builder.with_client_auth_cert(certs, key).map_err(|e| {
                ActorError::EvaluationError(format!("TLS client auth config error: {}", e))
            })?
        } else {
            builder.with_no_client_auth()
        };

        Ok(Arc::new(client_config))
    }
}

/// Unified network transport abstraction for plain TCP and TLS-wrapped streams.
pub enum ClusterTransport {
    Plain(TcpStream),
    TlsServer(StreamOwned<rustls::ServerConnection, TcpStream>),
    TlsClient(StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl Read for ClusterTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::TlsServer(s) => s.read(buf),
            Self::TlsClient(s) => s.read(buf),
        }
    }
}

impl Write for ClusterTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::TlsServer(s) => s.write(buf),
            Self::TlsClient(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::TlsServer(s) => s.flush(),
            Self::TlsClient(s) => s.flush(),
        }
    }
}

impl ClusterTransport {
    pub fn close(&mut self) {
        match self {
            Self::Plain(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            Self::TlsServer(s) => {
                let _ = s.sock.shutdown(Shutdown::Both);
            }
            Self::TlsClient(s) => {
                let _ = s.sock.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.set_read_timeout(timeout),
            Self::TlsServer(s) => s.sock.set_read_timeout(timeout),
            Self::TlsClient(s) => s.sock.set_read_timeout(timeout),
        }
    }
}

/// Active connection to an established remote cluster peer.
pub struct PeerConnection {
    pub node_name: String,
    pub address: String,
    pub sender: std::sync::mpsc::Sender<Vec<u8>>,
    pub known_actors: Arc<RwLock<HashSet<String>>>,
    pub is_alive: Arc<AtomicBool>,
    pub is_tls: bool,
}

impl PeerConnection {
    pub fn send_msg(&self, msg: &ClusterWireMsg) -> Result<(), ActorError> {
        if !self.is_alive.load(Ordering::Relaxed) {
            return Err(ActorError::ActorNotAlive);
        }
        let serialized = serde_json::to_vec(msg).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to serialize wire message: {}", e))
        })?;
        let len = serialized.len() as u32;
        let mut frame = Vec::with_capacity(4 + serialized.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&serialized);

        self.sender.send(frame).map_err(|_| {
            self.is_alive.store(false, Ordering::Relaxed);
            ActorError::EvaluationError("Peer connection worker channel disconnected".into())
        })
    }

    pub fn close(&self) {
        self.is_alive.store(false, Ordering::Relaxed);
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

/// Serializes a callable Shae value (string, AST function, or bytecode closure) into source representation.
pub fn callable_to_source(val: &Value) -> Result<String, ActorError> {
    match val {
        Value::String(s) => Ok(s.clone()),
        Value::Function { params, body, .. } => {
            let prog = crate::ast::Program {
                statements: body.as_ref().clone(),
            };
            let body_str = crate::fmt::Formatter::new().format_program(&prog);
            Ok(format!("fn({}) {{\n{}\n}}", params.join(", "), body_str))
        }
        Value::Closure(c) => {
            use base64::Engine;
            let bytes = c.function.chunk.to_bytes();
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(format!("BYTECODE:{}:{}", c.function.arity, b64))
        }
        Value::CompiledFunction(f) => {
            use base64::Engine;
            let bytes = f.chunk.to_bytes();
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(format!("BYTECODE:{}:{}", f.arity, b64))
        }
        other => Err(ActorError::EvaluationError(format!(
            "Expected handler code string or callable, got {}",
            other.type_name()
        ))),
    }
}

/// Unpacks a handler code string into a runnable Value (either compiled from Shae code or decoded bytecode).
pub fn unpack_handler_code(code: &str) -> Result<Value, String> {
    if let Some(rest) = code.strip_prefix("BYTECODE:") {
        use base64::Engine;
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        let (arity, b64) = if parts.len() == 2 {
            (parts[0].parse::<usize>().unwrap_or(2), parts[1])
        } else {
            (2, parts[0])
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        let chunk = crate::chunk::Chunk::from_bytes(&bytes)
            .map_err(|e| format!("Bytecode decode error: {}", e))?;
        let comp_fn = Arc::new(crate::value::CompiledFunction::new(
            arity,
            chunk,
            None,
            Vec::new(),
            false,
        ));
        let closure = Arc::new(crate::value::Closure {
            function: comp_fn,
            upvalues: Vec::new(),
            module_globals: None,
        });
        Ok(Value::Closure(closure))
    } else {
        let trimmed = code.trim();
        let runnable = if trimmed.starts_with("fn(") || trimmed.starts_with("fn (") {
            format!("let _h = {}; _h", trimmed)
        } else {
            code.to_string()
        };
        crate::run(&runnable).map_err(|e| format!("Failed to compile handler code: {}", e))
    }
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

    /// Hot-reloads the remote actor with new handler code and optional state migration.
    pub fn reload(
        &self,
        new_handler: Value,
        migration_fn: Option<Value>,
        timeout: Option<Duration>,
    ) -> Result<bool, ActorError> {
        let handler_str = callable_to_source(&new_handler)?;
        let migration_str = match &migration_fn {
            Some(v) => Some(callable_to_source(v)?),
            None => None,
        };
        self.cluster_node.reload_remote(
            &self.node_name,
            &self.actor_name,
            &handler_str,
            migration_str.as_deref(),
            timeout,
        )
    }

    /// Convenience alias to reload only the message handler on the remote actor.
    pub fn reload_handler(&self, new_handler: Value) -> Result<bool, ActorError> {
        self.reload(new_handler, None, None)
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
    pub tls_config: Option<ClusterTlsConfig>,
    server_tls_config: Option<Arc<ServerConfig>>,
    client_tls_config: Option<Arc<ClientConfig>>,
    is_stopped: Arc<AtomicBool>,
    listener_thread: Mutex<Option<JoinHandle<()>>>,
    pub peers: Arc<RwLock<HashMap<String, Arc<PeerConnection>>>>,
    pub local_actors: Arc<RwLock<HashMap<String, Arc<ActorRef>>>>,
    pub pending_requests: Arc<RwLock<HashMap<String, Arc<Channel>>>>,
    pub broadcast_subscribers: Arc<RwLock<Vec<Value>>>,
}

impl ClusterNode {
    /// Starts a new cluster node bound to the specified TCP address without TLS.
    pub fn start(
        name: impl Into<String>,
        address: impl Into<String>,
        cookie: Option<String>,
    ) -> Result<Arc<Self>, ActorError> {
        Self::start_with_tls(name, address, cookie, None)
    }

    /// Starts a new cluster node with optional TLS / mTLS configuration.
    pub fn start_with_tls(
        name: impl Into<String>,
        address: impl Into<String>,
        cookie: Option<String>,
        tls_config: Option<ClusterTlsConfig>,
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

        let (server_tls, client_tls) = if let Some(ref tls) = tls_config {
            let s = tls.build_server_config()?;
            let c = tls.build_client_config()?;
            (Some(s), Some(c))
        } else {
            (None, None)
        };

        let node = Arc::new(Self {
            name: name_str,
            bind_address: addr_str,
            actual_address: RwLock::new(bound_addr),
            cookie,
            tls_config,
            server_tls_config: server_tls,
            client_tls_config: client_tls,
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

    pub fn is_tls(&self) -> bool {
        self.tls_config.is_some()
    }

    pub fn tls_config(&self) -> Option<&ClusterTlsConfig> {
        self.tls_config.as_ref()
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

        let tcp_stream = TcpStream::connect(peer_addr).map_err(|e| {
            ActorError::EvaluationError(format!(
                "Failed to connect to cluster peer at '{}': {}",
                peer_addr, e
            ))
        })?;

        let is_tls = self.client_tls_config.is_some();
        let mut transport = if let Some(ref client_cfg) = self.client_tls_config {
            let sni_name = self
                .tls_config
                .as_ref()
                .and_then(|t| t.server_name.clone())
                .unwrap_or_else(|| {
                    peer_addr
                        .split(':')
                        .next()
                        .unwrap_or("localhost")
                        .to_string()
                });

            let server_name = ServerName::try_from(sni_name.clone()).map_err(|e| {
                ActorError::EvaluationError(format!(
                    "Invalid TLS SNI server name '{}': {}",
                    sni_name, e
                ))
            })?;

            let conn = rustls::ClientConnection::new(Arc::clone(client_cfg), server_name).map_err(
                |e| {
                    ActorError::EvaluationError(format!(
                        "Failed to initialize TLS client connection: {}",
                        e
                    ))
                },
            )?;

            ClusterTransport::TlsClient(StreamOwned::new(conn, tcp_stream))
        } else {
            ClusterTransport::Plain(tcp_stream)
        };

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
        write_frame(&mut transport, &bytes).map_err(|e| {
            ActorError::EvaluationError(format!("Failed to write handshake frame: {}", e))
        })?;

        // 2. Await HandshakeAck or HandshakeReject
        let reply_bytes = read_frame(&mut transport).map_err(|e| {
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

        let (tx, rx) = std::sync::mpsc::channel();

        let peer = Arc::new(PeerConnection {
            node_name: peer_node.clone(),
            address: peer_addr.to_string(),
            sender: tx,
            known_actors: Arc::new(RwLock::new(peer_actors.into_iter().collect())),
            is_alive: Arc::new(AtomicBool::new(true)),
            is_tls,
        });

        {
            let mut peers = self.peers.write().unwrap();
            peers.insert(peer_node.clone(), Arc::clone(&peer));
        }

        // Spawn connection worker thread
        self.spawn_connection_worker(transport, rx, Arc::clone(&peer));

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
            tls_config: self.tls_config.clone(),
            server_tls_config: self.server_tls_config.clone(),
            client_tls_config: self.client_tls_config.clone(),
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

    /// Sends a hot-code reload command to an actor running on a remote cluster node.
    pub fn reload_remote(
        &self,
        peer_node: &str,
        target_actor: &str,
        handler_code: &str,
        migration_code: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<bool, ActorError> {
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
            "req-reload-{}-{}",
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

        let envelope = ClusterWireMsg::Reload {
            request_id: req_id.clone(),
            from_node: self.name.clone(),
            target: target_actor.to_string(),
            handler_code: handler_code.to_string(),
            migration_code: migration_code.map(|s| s.to_string()),
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
                            return Ok(true);
                        } else if tag == "err" {
                            let err_str = match &items[1] {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            return Err(ActorError::ActorFailed(err_str));
                        }
                    }
                }
                Ok(true)
            }
            Ok(Value::Null) => Err(ActorError::Timeout),
            Ok(_) => Ok(true),
            Err(e) => Err(ActorError::EvaluationError(e.message)),
        }
    }

    /// Hot-reloads an actor across local registry or remote cluster peers.
    pub fn reload_actor(
        self: &Arc<Self>,
        actor_name: &str,
        new_handler: Value,
        migration_fn: Option<Value>,
        timeout: Option<Duration>,
    ) -> Result<bool, ActorError> {
        // 1. Check local actors
        let local_act = {
            let local = self.local_actors.read().unwrap();
            local.get(actor_name).cloned()
        };
        let local_act = local_act.or_else(|| ActorRegistry::whereis(actor_name));

        if let Some(act) = local_act {
            let handler_val = match &new_handler {
                Value::String(code) => unpack_handler_code(code)
                    .map_err(ActorError::EvaluationError)?,
                other => other.clone(),
            };
            let mig_val = match migration_fn {
                Some(Value::String(ref code)) => Some(
                    unpack_handler_code(code).map_err(ActorError::EvaluationError)?,
                ),
                other => other,
            };
            return act.reload(handler_val, mig_val, timeout);
        }

        // 2. Check if actor_name contains '@' (e.g. "worker@peer")
        if actor_name.contains('@') {
            let parts: Vec<&str> = actor_name.splitn(2, '@').collect();
            let target = parts[0];
            let peer = parts[1];
            let handler_str = callable_to_source(&new_handler)?;
            let migration_str = match &migration_fn {
                Some(v) => Some(callable_to_source(v)?),
                None => None,
            };
            return self.reload_remote(
                peer,
                target,
                &handler_str,
                migration_str.as_deref(),
                timeout,
            );
        }

        // 3. Search peer known actors
        let peers = self.peers.read().unwrap();
        for (peer_name, peer) in peers.iter() {
            let known = peer.known_actors.read().unwrap();
            if known.contains(actor_name) {
                let handler_str = callable_to_source(&new_handler)?;
                let migration_str = match &migration_fn {
                    Some(v) => Some(callable_to_source(v)?),
                    None => None,
                };
                return self.reload_remote(
                    peer_name,
                    actor_name,
                    &handler_str,
                    migration_str.as_deref(),
                    timeout,
                );
            }
        }

        Err(ActorError::NameNotFound(format!(
            "Actor '{}' not found locally or in cluster",
            actor_name
        )))
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

            let stream = match stream_res {
                Ok(s) => s,
                Err(_) => continue,
            };

            let node_self = Arc::clone(&self);
            thread::spawn(move || {
                node_self.handle_incoming_connection(stream);
            });
        }
    }

    fn handle_incoming_connection(&self, tcp_stream: TcpStream) {
        if self.is_stopped.load(Ordering::Relaxed) {
            return;
        }

        let is_tls = self.server_tls_config.is_some();
        let mut transport = if let Some(ref server_cfg) = self.server_tls_config {
            let conn = match rustls::ServerConnection::new(Arc::clone(server_cfg)) {
                Ok(c) => c,
                Err(_) => return,
            };
            ClusterTransport::TlsServer(StreamOwned::new(conn, tcp_stream))
        } else {
            ClusterTransport::Plain(tcp_stream)
        };

        // Read Handshake
        let frame = match read_frame(&mut transport) {
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
                    let _ = write_frame(&mut transport, &bytes);
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
                    let _ = write_frame(&mut transport, &bytes);
                }
                return;
            }
        }

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
        if write_frame(&mut transport, &ack_bytes).is_err() {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();

        let peer = Arc::new(PeerConnection {
            node_name: peer_node.clone(),
            address: peer_addr,
            sender: tx,
            known_actors: Arc::new(RwLock::new(peer_actors.into_iter().collect())),
            is_alive: Arc::new(AtomicBool::new(true)),
            is_tls,
        });

        {
            let mut peers = self.peers.write().unwrap();
            peers.insert(peer_node, Arc::clone(&peer));
        }

        self.spawn_connection_worker(transport, rx, Arc::clone(&peer));
    }

    fn spawn_connection_worker(
        &self,
        mut transport: ClusterTransport,
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
        peer: Arc<PeerConnection>,
    ) {
        let node_self = Arc::new(self.clone_ref());
        let peer_clone = Arc::clone(&peer);

        thread::Builder::new()
            .name(format!("peer-worker-{}", peer.node_name))
            .spawn(move || {
                let _ = transport.set_read_timeout(Some(Duration::from_millis(2)));

                while !node_self.is_stopped.load(Ordering::Relaxed)
                    && peer_clone.is_alive.load(Ordering::Relaxed)
                {
                    // A. Send any queued outgoing frames
                    let mut has_sent = false;
                    while let Ok(frame) = rx.try_recv() {
                        if transport.write_all(&frame).is_err() {
                            peer_clone.is_alive.store(false, Ordering::Relaxed);
                            break;
                        }
                        has_sent = true;
                    }
                    if has_sent {
                        if transport.flush().is_err() {
                            peer_clone.is_alive.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                    if !peer_clone.is_alive.load(Ordering::Relaxed) {
                        break;
                    }

                    // B. Read incoming frame
                    match read_frame_adaptive(&mut transport) {
                        Ok(frame) => {
                            let msg: ClusterWireMsg = match serde_json::from_slice(&frame) {
                                Ok(m) => m,
                                Err(_) => continue,
                            };
                            node_self.dispatch_incoming_wire_msg(msg, &peer_clone);
                        }
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            // Normal timeout when idle
                        }
                        Err(_) => {
                            // Connection closed or broken
                            break;
                        }
                    }
                }

                peer_clone.is_alive.store(false, Ordering::Relaxed);
                transport.close();
                let mut peers = node_self.peers.write().unwrap();
                peers.remove(&peer_clone.node_name);
            })
            .unwrap();
    }

    fn dispatch_incoming_wire_msg(&self, msg: ClusterWireMsg, peer: &Arc<PeerConnection>) {
        match msg {
            ClusterWireMsg::Ping => {
                let pong = ClusterWireMsg::Pong;
                let _ = peer.send_msg(&pong);
            }
            ClusterWireMsg::Pong => {}
            ClusterWireMsg::Send {
                target, payload, ..
            } => {
                let value = Value::from_json(&payload);
                let local_opt = {
                    let local = self.local_actors.read().unwrap();
                    local.get(&target).cloned()
                };
                let actor_target = local_opt.or_else(|| ActorRegistry::whereis(&target));
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
                    let local = self.local_actors.read().unwrap();
                    local.get(&target).cloned()
                };
                let actor_target = local_opt.or_else(|| ActorRegistry::whereis(&target));
                let peer_reply = Arc::clone(peer);

                thread::spawn(move || match actor_target {
                    Some(act) => match act.ask(value, Some(Duration::from_millis(timeout_ms))) {
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
                    },
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
                    let mut pending = self.pending_requests.write().unwrap();
                    pending.remove(&request_id)
                };

                if let Some(chan) = chan_opt {
                    let span = Span::new(1, 1);
                    if status == "ok" {
                        let content = payload.map(|p| Value::from_json(&p)).unwrap_or(Value::Null);
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
                let mut known = peer.known_actors.write().unwrap();
                if action == "register" {
                    known.insert(actor);
                } else {
                    known.remove(&actor);
                }
            }
            ClusterWireMsg::Broadcast { topic, payload, .. } => {
                let val = Value::from_json(&payload);
                self.deliver_local_broadcast(&topic, &val);
            }
            ClusterWireMsg::Reload {
                request_id,
                target,
                handler_code,
                migration_code,
                timeout_ms,
                ..
            } => {
                let local_opt = {
                    let local = self.local_actors.read().unwrap();
                    local.get(&target).cloned()
                };
                let actor_target = local_opt.or_else(|| ActorRegistry::whereis(&target));
                let peer_reply = Arc::clone(peer);

                thread::spawn(move || match actor_target {
                    Some(act) => {
                        let handler_val = match unpack_handler_code(&handler_code) {
                            Ok(v) => v,
                            Err(e) => {
                                let reply = ClusterWireMsg::Reply {
                                    request_id,
                                    status: "err".into(),
                                    payload: None,
                                    error: Some(format!("Failed to compile handler code: {}", e)),
                                };
                                let _ = peer_reply.send_msg(&reply);
                                return;
                            }
                        };
                        let migration_val = match migration_code {
                            Some(ref code) => match unpack_handler_code(code) {
                                Ok(v) => Some(v),
                                Err(e) => {
                                    let reply = ClusterWireMsg::Reply {
                                        request_id,
                                        status: "err".into(),
                                        payload: None,
                                        error: Some(format!("Failed to compile migration code: {}", e)),
                                    };
                                    let _ = peer_reply.send_msg(&reply);
                                    return;
                                }
                            },
                            None => None,
                        };

                        match act.reload(handler_val, migration_val, Some(Duration::from_millis(timeout_ms))) {
                            Ok(ok) => {
                                let reply = ClusterWireMsg::Reply {
                                    request_id,
                                    status: "ok".into(),
                                    payload: Some(serde_json::Value::Bool(ok)),
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
            _ => {}
        }
    }
}

impl fmt::Debug for ClusterNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ClusterNode({}@{} tls={})",
            self.name,
            self.address(),
            self.is_tls()
        )
    }
}

impl fmt::Display for ClusterNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<cluster_node:{}@{}{}>",
            self.name,
            self.address(),
            if self.is_tls() { " (tls)" } else { "" }
        )
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
