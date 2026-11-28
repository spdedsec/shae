//! Pure-Rust HTTP/1.1, HTTP/2 (HPACK), and QUIC/HTTP/3 (QPACK) web engine for Shae.
//!
//! Zero native C dependencies: built solely on `std::net`, `ring`, `rustls`,
//! `rustls-pemfile`, `indexmap`, `serde_json`, `fastrand`, and `crate::async_rt`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM};
use ring::hkdf::{self, HKDF_SHA256};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

use crate::ast::Span;
use crate::buffer::ByteBuffer;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::stdlib::make_map;
use crate::value::Value;

// ============================================================================
// Common Utilities, Worker Env Context, Routing, and Shae Value Conversions
// ============================================================================

#[derive(Clone)]
pub(crate) struct WorkerEnvContext {
    pub env_root: Option<Arc<RwLock<Environment>>>,
    pub current_file: Option<PathBuf>,
    pub embedded_archive: Option<Arc<HashMap<String, String>>>,
    pub embedded_bytecode: Option<Arc<HashMap<String, Vec<u8>>>>,
}

impl WorkerEnvContext {
    pub fn capture(ev: &Evaluator) -> Self {
        Self {
            env_root: Some(ev.global_env.clone()),
            current_file: ev.current_file.clone(),
            embedded_archive: ev.embedded_archive.clone(),
            embedded_bytecode: ev.embedded_bytecode.clone(),
        }
    }

    pub fn build_evaluator(&self) -> Evaluator {
        let mut ev = match &self.env_root {
            Some(root) => Evaluator::with_env(root.clone()),
            None => Evaluator::new(),
        };
        ev.current_file = self.current_file.clone();
        ev.embedded_archive = self.embedded_archive.clone();
        ev.embedded_bytecode = self.embedded_bytecode.clone();
        ev
    }
}

#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
    pub authority: String,
}

pub fn parse_url(input: &str) -> Result<ParsedUrl, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("URL cannot be empty".to_string());
    }
    let (scheme, rest) = if let Some(idx) = trimmed.find("://") {
        let s = trimmed[..idx].to_ascii_lowercase();
        (s, &trimmed[idx + 3..])
    } else {
        ("http".to_string(), trimmed)
    };

    let default_port = match scheme.as_str() {
        "https" | "h2" | "h3" | "quic" => 443,
        _ => 80,
    };

    let (auth_part, path_part) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => match rest.find('?') {
            Some(q) => (&rest[..q], &rest[q..]),
            None => (rest, "/"),
        },
    };

    if auth_part.is_empty() {
        return Err(format!("Missing host in URL: {}", input));
    }
    if auth_part.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err("Invalid control or whitespace character in URL authority".to_string());
    }
    if path_part.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err("Invalid control or whitespace character in URL path".to_string());
    }

    let (host, port) = if auth_part.starts_with('[') {
        if let Some(end) = auth_part.find(']') {
            let h = auth_part[1..end].to_string();
            let after = &auth_part[end + 1..];
            let p = if let Some(p_str) = after.strip_prefix(':') {
                p_str
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port in URL: {}", input))?
            } else {
                default_port
            };
            (h, p)
        } else {
            return Err(format!("Malformed IPv6 host in URL: {}", input));
        }
    } else if let Some(colon) = auth_part.rfind(':') {
        let h = auth_part[..colon].to_string();
        let p = auth_part[colon + 1..]
            .parse::<u16>()
            .map_err(|_| format!("Invalid port in URL: {}", input))?;
        (h, p)
    } else {
        (auth_part.to_string(), default_port)
    };

    let path_and_query = if path_part.starts_with('?') {
        format!("/{}", path_part)
    } else if path_part.is_empty() {
        "/".to_string()
    } else {
        path_part.to_string()
    };

    let authority = if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else if port == default_port && (scheme == "http" || scheme == "https") {
        host.clone()
    } else {
        format!("{}:{}", host, port)
    };

    Ok(ParsedUrl {
        scheme,
        host,
        port,
        path_and_query,
        authority,
    })
}

fn format_socket_addr(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

fn wake_addr_for(bind_addr: SocketAddr) -> SocketAddr {
    match bind_addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bind_addr.port())
        }
        IpAddr::V6(v6) if v6.is_unspecified() => {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), bind_addr.port())
        }
        _ => bind_addr,
    }
}

fn parse_bind_addr_val(val: &Value, span: Span) -> Result<String, RuntimeError> {
    match val {
        Value::String(s) => {
            if s.contains(':') {
                Ok(s.clone())
            } else if let Ok(port) = s.parse::<u16>() {
                Ok(format!("127.0.0.1:{}", port))
            } else {
                Ok(s.clone())
            }
        }
        Value::Int(port) if *port >= 0 && *port <= 65535 => Ok(format!("127.0.0.1:{}", port)),
        other => Err(RuntimeError::new(format!(
            "Expected bind address string or port int, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn value_to_bytes(val: &Value) -> Result<Vec<u8>, String> {
    match val {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::ByteBuffer(bb) => Ok(bb.read().unwrap().to_bytes()),
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut out = Vec::with_capacity(guard.len());
            for item in guard.iter() {
                match item {
                    Value::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                    other => {
                        return Err(format!(
                            "Byte array elements must be ints in 0..=255, got {}",
                            other.type_name()
                        ));
                    }
                }
            }
            Ok(out)
        }
        Value::Null => Ok(Vec::new()),
        other => Err(format!(
            "Expected String, ByteBuffer, or byte Array, got {}",
            other.type_name()
        )),
    }
}

pub fn bytes_to_value(bytes: Vec<u8>) -> Value {
    Value::ByteBuffer(Arc::new(RwLock::new(ByteBuffer::from_vec(bytes))))
}

fn json_to_shae_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(json_to_shae_value).collect();
            Value::Array(Arc::new(RwLock::new(items)))
        }
        serde_json::Value::Object(obj) => {
            let mut map = IndexMap::new();
            for (k, val) in obj {
                map.insert(k.clone(), json_to_shae_value(val));
            }
            Value::Map(Arc::new(RwLock::new(map)))
        }
    }
}

fn shae_value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) | Value::Number(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Array(arr) => {
            let items = arr.read().unwrap();
            serde_json::Value::Array(items.iter().map(shae_value_to_json).collect())
        }
        Value::Map(m) => {
            let guard = m.read().unwrap();
            let mut obj = serde_json::Map::new();
            for (k, v) in guard.iter() {
                obj.insert(k.clone(), shae_value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        other => serde_json::Value::String(other.to_display()),
    }
}

fn headers_to_shae_map(headers: &IndexMap<String, Vec<String>>) -> Value {
    let mut map = IndexMap::new();
    for (k, vals) in headers {
        map.insert(k.clone(), Value::String(vals.join(", ")));
    }
    Value::Map(Arc::new(RwLock::new(map)))
}

fn extract_headers_from_shae_map(val: &Value) -> IndexMap<String, Vec<String>> {
    let mut out: IndexMap<String, Vec<String>> = IndexMap::new();
    if let Value::Map(m) = val {
        let guard = m.read().unwrap();
        for (k, v) in guard.iter() {
            let name = k.to_ascii_lowercase();
            match v {
                Value::Array(arr) => {
                    let items = arr.read().unwrap();
                    let entry = out.entry(name).or_default();
                    for item in items.iter() {
                        entry.push(item.to_display());
                    }
                }
                _ => {
                    out.entry(name).or_default().push(v.to_display());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct WireResponse {
    pub status: u16,
    pub headers: IndexMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub trailers: IndexMap<String, Vec<String>>,
}

fn shae_value_to_wire_response(val: &Value) -> WireResponse {
    match val {
        Value::Map(m) => {
            let guard = m.read().unwrap();
            let status = match guard.get("status") {
                Some(Value::Int(s)) => (*s).clamp(100, 599) as u16,
                Some(Value::Float(f)) | Some(Value::Number(f)) => (*f as i64).clamp(100, 599) as u16,
                _ => 200,
            };
            let mut headers = guard
                .get("headers")
                .map(extract_headers_from_shae_map)
                .unwrap_or_default();
            let trailers = guard
                .get("trailers")
                .map(extract_headers_from_shae_map)
                .unwrap_or_default();
            let body = if let Some(b) = guard
                .get("bytes")
                .or_else(|| guard.get("body"))
                .or_else(|| guard.get("text"))
            {
                value_to_bytes(b).unwrap_or_else(|_| b.to_display().into_bytes())
            } else if let Some(j) = guard.get("json") {
                if !headers.contains_key("content-type") {
                    headers.insert(
                        "content-type".to_string(),
                        vec!["application/json".to_string()],
                    );
                }
                serde_json::to_vec(&shae_value_to_json(j)).unwrap_or_default()
            } else {
                Vec::new()
            };
            WireResponse {
                status,
                headers,
                body,
                trailers,
            }
        }
        Value::String(s) => {
            let mut headers = IndexMap::new();
            headers.insert(
                "content-type".to_string(),
                vec!["text/plain; charset=utf-8".to_string()],
            );
            WireResponse {
                status: 200,
                headers,
                body: s.as_bytes().to_vec(),
                trailers: IndexMap::new(),
            }
        }
        Value::ByteBuffer(bb) => {
            let mut headers = IndexMap::new();
            headers.insert(
                "content-type".to_string(),
                vec!["application/octet-stream".to_string()],
            );
            WireResponse {
                status: 200,
                headers,
                body: bb.read().unwrap().to_bytes(),
                trailers: IndexMap::new(),
            }
        }
        Value::Int(status) => WireResponse {
            status: (*status).clamp(100, 599) as u16,
            headers: IndexMap::new(),
            body: Vec::new(),
            trailers: IndexMap::new(),
        },
        Value::Null => WireResponse {
            status: 204,
            headers: IndexMap::new(),
            body: Vec::new(),
            trailers: IndexMap::new(),
        },
        other => {
            let mut headers = IndexMap::new();
            headers.insert(
                "content-type".to_string(),
                vec!["text/plain; charset=utf-8".to_string()],
            );
            WireResponse {
                status: 200,
                headers,
                body: other.to_display().into_bytes(),
                trailers: IndexMap::new(),
            }
        }
    }
}

fn url_decode_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query_params_local(query: &str) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let k = url_decode_component(parts.next().unwrap_or(""));
        let v = url_decode_component(parts.next().unwrap_or(""));
        map.insert(k, Value::String(v));
    }
    map
}

fn wire_request_to_shae_value(
    method: &str,
    path_and_query: &str,
    headers: &IndexMap<String, Vec<String>>,
    body: &[u8],
    trailers: &IndexMap<String, Vec<String>>,
    params: IndexMap<String, Value>,
    version: &str,
    stream_id: u64,
) -> Value {
    let (path, query) = match path_and_query.find('?') {
        Some(idx) => (&path_and_query[..idx], &path_and_query[idx + 1..]),
        None => (path_and_query, ""),
    };

    let qmap = parse_query_params_local(query);
    let query_params_val = Value::Map(Arc::new(RwLock::new(qmap)));

    let json_val = if !body.is_empty() {
        serde_json::from_slice::<serde_json::Value>(body)
            .map(|j| json_to_shae_value(&j))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    let mut map = IndexMap::new();
    map.insert("method".to_string(), Value::String(method.to_string()));
    map.insert("path".to_string(), Value::String(path.to_string()));
    map.insert("target".to_string(), Value::String(path_and_query.to_string()));
    map.insert("query".to_string(), Value::String(query.to_string()));
    map.insert("query_params".to_string(), query_params_val.clone());
    map.insert("queryParams".to_string(), query_params_val);
    map.insert("params".to_string(), Value::Map(Arc::new(RwLock::new(params))));
    map.insert("headers".to_string(), headers_to_shae_map(headers));
    map.insert("trailers".to_string(), headers_to_shae_map(trailers));
    map.insert(
        "body".to_string(),
        Value::String(String::from_utf8_lossy(body).into_owned()),
    );
    map.insert("bytes".to_string(), bytes_to_value(body.to_vec()));
    map.insert("json".to_string(), json_val);
    map.insert("version".to_string(), Value::String(version.to_string()));
    map.insert("stream_id".to_string(), Value::Int(stream_id as i64));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn wire_response_to_shae_value(
    status: u16,
    headers: &IndexMap<String, Vec<String>>,
    body: &[u8],
    trailers: &IndexMap<String, Vec<String>>,
    version: &str,
    stream_id: u64,
) -> Value {
    let json_val = if !body.is_empty() {
        serde_json::from_slice::<serde_json::Value>(body)
            .map(|j| json_to_shae_value(&j))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    let body_str = String::from_utf8_lossy(body).into_owned();
    let mut map = IndexMap::new();
    map.insert("status".to_string(), Value::Int(status as i64));
    map.insert("ok".to_string(), Value::Bool((200..300).contains(&status)));
    map.insert("headers".to_string(), headers_to_shae_map(headers));
    map.insert("trailers".to_string(), headers_to_shae_map(trailers));
    map.insert("body".to_string(), Value::String(body_str.clone()));
    map.insert("text".to_string(), Value::String(body_str));
    map.insert("bytes".to_string(), bytes_to_value(body.to_vec()));
    map.insert("json".to_string(), json_val);
    map.insert("version".to_string(), Value::String(version.to_string()));
    map.insert("stream_id".to_string(), Value::Int(stream_id as i64));
    Value::Map(Arc::new(RwLock::new(map)))
}

#[derive(Clone)]
pub(crate) struct RouteEntry {
    method: String, // "*" matches any method
    pattern: String,
    handler: Value,
}

#[derive(Clone, Default)]
pub(crate) struct RouterState {
    routes: Vec<RouteEntry>,
    fallback: Option<Value>,
}

impl RouterState {
    fn resolve(&self, method: &str, path: &str) -> Option<(Value, IndexMap<String, Value>)> {
        let method_upper = method.to_ascii_uppercase();
        for r in &self.routes {
            if r.method == "*" || r.method == method_upper {
                if let Some(params) = crate::builtins::match_route_pattern(&r.pattern, path) {
                    return Some((r.handler.clone(), params));
                }
            }
        }
        self.fallback
            .as_ref()
            .map(|h| (h.clone(), IndexMap::new()))
    }
}

fn dispatch_shae_handler(
    router: &RouterState,
    ctx: &WorkerEnvContext,
    method: &str,
    path_and_query: &str,
    headers: &IndexMap<String, Vec<String>>,
    body: &[u8],
    trailers: &IndexMap<String, Vec<String>>,
    version: &str,
    stream_id: u64,
) -> WireResponse {
    let path_only = path_and_query
        .split('?')
        .next()
        .unwrap_or(path_and_query);
    let Some((handler, params)) = router.resolve(method, path_only) else {
        return WireResponse {
            status: 404,
            headers: {
                let mut h = IndexMap::new();
                h.insert("content-type".to_string(), vec!["text/plain".to_string()]);
                h
            },
            body: b"Not Found".to_vec(),
            trailers: IndexMap::new(),
        };
    };

    let req_val = wire_request_to_shae_value(
        method,
        path_and_query,
        headers,
        body,
        trailers,
        params,
        version,
        stream_id,
    );
    let mut ev = ctx.build_evaluator();
    match ev.call_value(&handler, vec![req_val], Span::new(1, 1)) {
        Ok(val) => shae_value_to_wire_response(&val),
        Err(err) => WireResponse {
            status: 500,
            headers: {
                let mut h = IndexMap::new();
                h.insert("content-type".to_string(), vec!["text/plain".to_string()]);
                h
            },
            body: format!("Internal Server Error: {}", err.message).into_bytes(),
            trailers: IndexMap::new(),
        },
    }
}

// ============================================================================
// Rustls Helper Utilities (ALPN "h2" / "h3", PEM Cert/Key Loading)
// ============================================================================

#[derive(Debug)]
struct AcceptAnyCertVerifier(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCertVerifier {
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
        self.0
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn load_pem_certs(input: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let mut reader: Box<dyn io::BufRead> = if input.contains("-----BEGIN") {
        Box::new(io::BufReader::new(input.as_bytes()))
    } else {
        let f = std::fs::File::open(input)
            .map_err(|e| format!("Failed to open certificate '{}': {}", input, e))?;
        Box::new(io::BufReader::new(f))
    };
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse PEM certificates: {}", e))
}

fn load_pem_key(input: &str) -> Result<PrivateKeyDer<'static>, String> {
    let mut reader: Box<dyn io::BufRead> = if input.contains("-----BEGIN") {
        Box::new(io::BufReader::new(input.as_bytes()))
    } else {
        let f = std::fs::File::open(input)
            .map_err(|e| format!("Failed to open private key '{}': {}", input, e))?;
        Box::new(io::BufReader::new(f))
    };
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| format!("Failed to parse PEM private key: {}", e))?
        .ok_or_else(|| "No private key found in PEM input".to_string())
}

fn build_tls_server_config(
    cert_input: &str,
    key_input: &str,
    alpn: Vec<Vec<u8>>,
) -> Result<Arc<ServerConfig>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certs = load_pem_certs(cert_input)?;
    let key = load_pem_key(key_input)?;
    let mut cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TLS ServerConfig error: {}", e))?;
    cfg.alpn_protocols = alpn;
    Ok(Arc::new(cfg))
}

fn build_tls_client_config(
    ca_input: Option<&str>,
    insecure: bool,
    alpn: Vec<Vec<u8>>,
) -> Result<Arc<ClientConfig>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = if let Some(ca) = ca_input {
        let ca_certs = load_pem_certs(ca)?;
        let mut store = RootCertStore::empty();
        for c in ca_certs {
            store
                .add(c)
                .map_err(|e| format!("Failed to add CA cert: {}", e))?;
        }
        ClientConfig::builder().with_root_certificates(store)
    } else if insecure {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertVerifier(provider)))
    } else {
        ClientConfig::builder().with_root_certificates(RootCertStore::empty())
    };
    let mut cfg = builder.with_no_client_auth();
    cfg.alpn_protocols = alpn;
    Ok(Arc::new(cfg))
}

// ============================================================================
// Part 1: HTTP/1.1 Wire Codec, BufStream, Server & Pooled Pipelined Client
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H1Error {
    Eof,
    IdleTimeout,
    RequestTimeout,
    UnexpectedEof,
    BadRequest(String),
    HeadersTooLarge,
    PayloadTooLarge,
    UnsupportedVersion(String),
    Io(String),
}

impl std::fmt::Display for H1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eof => write!(f, "Clean EOF"),
            Self::IdleTimeout => write!(f, "Idle keep-alive timeout"),
            Self::RequestTimeout => write!(f, "HTTP/1.1 request timeout"),
            Self::UnexpectedEof => write!(f, "Unexpected EOF"),
            Self::BadRequest(msg) => write!(f, "Bad HTTP/1.1 message: {}", msg),
            Self::HeadersTooLarge => write!(f, "HTTP/1.1 headers too large"),
            Self::PayloadTooLarge => write!(f, "HTTP/1.1 payload too large"),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported HTTP version: {}", v),
            Self::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl From<io::Error> for H1Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            ErrorKind::UnexpectedEof => Self::UnexpectedEof,
            _ => Self::Io(e.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct H1Limits {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
}

impl Default for H1Limits {
    fn default() -> Self {
        Self {
            max_header_bytes: 64 * 1024,
            max_body_bytes: 16 * 1024 * 1024,
        }
    }
}

pub struct BufStream<S: Read + Write> {
    pub inner: S,
    buf: Vec<u8>,
    pos: usize,
    filled: usize,
    running: Option<Arc<AtomicBool>>,
    request_timeout: Duration,
}

impl<S: Read + Write> BufStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buf: vec![0u8; 8192],
            pos: 0,
            filled: 0,
            running: None,
            request_timeout: Duration::from_secs(10),
        }
    }

    pub fn with_running(mut self, running: Arc<AtomicBool>) -> Self {
        self.running = Some(running);
        self
    }

    fn is_running(&self) -> bool {
        self.running
            .as_ref()
            .map(|r| r.load(Ordering::Acquire))
            .unwrap_or(true)
    }

    fn is_timeout_err(err: &io::Error) -> bool {
        matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    }

    fn read_with_deadline(&mut self, out: &mut [u8], deadline: Instant) -> Result<usize, H1Error> {
        loop {
            match self.inner.read(out) {
                Ok(n) => return Ok(n),
                Err(e) if Self::is_timeout_err(&e) => {
                    if !self.is_running() {
                        return Err(H1Error::Eof);
                    }
                    if Instant::now() >= deadline {
                        return Err(H1Error::RequestTimeout);
                    }
                    continue;
                }
                Err(e) => return Err(H1Error::Io(e.to_string())),
            }
        }
    }

    pub fn has_buffered_data(&self) -> bool {
        self.pos < self.filled
    }

    pub fn buffered_len(&self) -> usize {
        self.filled - self.pos
    }

    fn compact_or_grow(&mut self, max_cap: usize) -> Result<(), H1Error> {
        if self.pos == self.filled {
            self.pos = 0;
            self.filled = 0;
            return Ok(());
        }
        if self.filled == self.buf.len() {
            if self.pos > 0 {
                self.buf.copy_within(self.pos..self.filled, 0);
                self.filled -= self.pos;
                self.pos = 0;
            } else {
                if self.buf.len() >= max_cap {
                    return Err(H1Error::HeadersTooLarge);
                }
                let new_len = (self.buf.len() * 2).min(max_cap);
                self.buf.resize(new_len, 0);
            }
        }
        Ok(())
    }

    fn fill_more_checked(
        &mut self,
        max_cap: usize,
        allow_idle: bool,
        deadline: Instant,
    ) -> Result<usize, H1Error> {
        self.compact_or_grow(max_cap)?;
        let n = if allow_idle {
            match self.inner.read(&mut self.buf[self.filled..]) {
                Ok(n) => n,
                Err(e) if Self::is_timeout_err(&e) => {
                    return Err(if self.is_running() {
                        H1Error::IdleTimeout
                    } else {
                        H1Error::Eof
                    });
                }
                Err(e) => return Err(H1Error::Io(e.to_string())),
            }
        } else {
            loop {
                match self.inner.read(&mut self.buf[self.filled..]) {
                    Ok(n) => break n,
                    Err(e) if Self::is_timeout_err(&e) => {
                        if !self.is_running() {
                            return Err(H1Error::Eof);
                        }
                        if Instant::now() >= deadline {
                            return Err(H1Error::RequestTimeout);
                        }
                        continue;
                    }
                    Err(e) => return Err(H1Error::Io(e.to_string())),
                }
            }
        };
        self.filled += n;
        Ok(n)
    }

    pub fn read_header_block(
        &mut self,
        max_bytes: usize,
        skip_leading_crlf: bool,
    ) -> Result<Vec<u8>, H1Error> {
        let mut saw_any_byte = self.pos < self.filled;
        let mut deadline = Instant::now() + self.request_timeout;

        if skip_leading_crlf {
            while self.filled - self.pos < 2 {
                if self.filled - self.pos == 1 && self.buf[self.pos] != b'\r' {
                    break;
                }
                let n = self.fill_more_checked(max_bytes + 4, !saw_any_byte, deadline)?;
                if n == 0 {
                    return if !saw_any_byte && self.pos == self.filled {
                        Err(H1Error::Eof)
                    } else {
                        Err(H1Error::UnexpectedEof)
                    };
                }
                if !saw_any_byte {
                    saw_any_byte = true;
                    deadline = Instant::now() + self.request_timeout;
                }
            }
            if self.filled - self.pos >= 2
                && self.buf[self.pos] == b'\r'
                && self.buf[self.pos + 1] == b'\n'
            {
                self.pos += 2;
                saw_any_byte = true;
            }
        }

        loop {
            let unread = &self.buf[self.pos..self.filled];
            if let Some(idx) = unread.windows(4).position(|w| w == b"\r\n\r\n") {
                if idx + 4 > max_bytes {
                    return Err(H1Error::HeadersTooLarge);
                }
                let block = unread[..idx + 2].to_vec();
                self.pos += idx + 4;
                return Ok(block);
            }
            if unread.len() >= max_bytes {
                return Err(H1Error::HeadersTooLarge);
            }
            let n = self.fill_more_checked(max_bytes + 8, !saw_any_byte, deadline)?;
            if n == 0 {
                return if !saw_any_byte && self.pos == self.filled {
                    Err(H1Error::Eof)
                } else {
                    Err(H1Error::UnexpectedEof)
                };
            }
            if !saw_any_byte {
                saw_any_byte = true;
                deadline = Instant::now() + self.request_timeout;
            }
        }
    }

    pub fn read_line_crlf(&mut self, max_len: usize) -> Result<Vec<u8>, H1Error> {
        let deadline = Instant::now() + self.request_timeout;
        loop {
            let unread = &self.buf[self.pos..self.filled];
            if let Some(idx) = unread.windows(2).position(|w| w == b"\r\n") {
                if idx > max_len {
                    return Err(H1Error::BadRequest("Line exceeds max length".to_string()));
                }
                let line = unread[..idx].to_vec();
                self.pos += idx + 2;
                return Ok(line);
            }
            if unread.len() > max_len {
                return Err(H1Error::BadRequest("Line exceeds max length".to_string()));
            }
            let n = self.fill_more_checked(max_len + 8, false, deadline)?;
            if n == 0 {
                return Err(H1Error::UnexpectedEof);
            }
        }
    }

    pub fn read_exact_bytes(&mut self, len: usize) -> Result<Vec<u8>, H1Error> {
        let deadline = Instant::now() + self.request_timeout;
        let mut out = Vec::with_capacity(len.min(1024 * 1024));
        let mut rem = len;
        while rem > 0 {
            let avail = self.filled - self.pos;
            if avail > 0 {
                let take = avail.min(rem);
                out.extend_from_slice(&self.buf[self.pos..self.pos + take]);
                self.pos += take;
                rem -= take;
            } else {
                self.pos = 0;
                self.filled = 0;
                let mut tmp = vec![0u8; rem.min(65536)];
                let n = self.read_with_deadline(&mut tmp, deadline)?;
                if n == 0 {
                    return Err(H1Error::UnexpectedEof);
                }
                out.extend_from_slice(&tmp[..n]);
                rem -= n;
            }
        }
        Ok(out)
    }

    pub fn read_until_eof(&mut self, max_body: usize) -> Result<Vec<u8>, H1Error> {
        let deadline = Instant::now() + self.request_timeout;
        let mut out = Vec::new();
        if self.pos < self.filled {
            let avail = self.filled - self.pos;
            if avail > max_body {
                return Err(H1Error::PayloadTooLarge);
            }
            out.extend_from_slice(&self.buf[self.pos..self.filled]);
            self.pos = 0;
            self.filled = 0;
        }
        let mut tmp = [0u8; 8192];
        loop {
            let n = self.read_with_deadline(&mut tmp, deadline)?;
            if n == 0 {
                break;
            }
            if out.len() + n > max_body {
                return Err(H1Error::PayloadTooLarge);
            }
            out.extend_from_slice(&tmp[..n]);
        }
        Ok(out)
    }

    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.inner.write_all(data)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
            | b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

fn validate_header_name(raw: &[u8]) -> Result<String, H1Error> {
    if raw.is_empty() || !raw.iter().all(|&b| is_tchar(b)) {
        return Err(H1Error::BadRequest(format!(
            "Invalid HTTP header name: {:?}",
            String::from_utf8_lossy(raw)
        )));
    }
    Ok(String::from_utf8_lossy(raw).to_ascii_lowercase())
}

fn validate_header_value(raw: &[u8]) -> Result<String, H1Error> {
    if raw.iter().any(|&b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(H1Error::BadRequest(
            "Forbidden control character in header value".to_string(),
        ));
    }
    let s = String::from_utf8_lossy(raw);
    Ok(s.trim_matches(|c| c == ' ' || c == '\t').to_string())
}

fn validate_outbound_header(name: &str, value: &str) -> Result<(), H1Error> {
    if name.is_empty() || !name.as_bytes().iter().all(|&b| is_tchar(b)) {
        return Err(H1Error::BadRequest(format!(
            "Invalid outbound header name: {}",
            name
        )));
    }
    if value.as_bytes().iter().any(|&b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(H1Error::BadRequest(format!(
            "CRLF/NUL injection rejected in outbound header '{}'",
            name
        )));
    }
    Ok(())
}

fn parse_headers_lines(
    lines: &[&[u8]],
    allow_obs_fold: bool,
) -> Result<IndexMap<String, Vec<String>>, H1Error> {
    let mut headers: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut last_key: Option<String> = None;

    for &line in lines {
        if line.is_empty() {
            continue;
        }
        if line[0] == b' ' || line[0] == b'\t' {
            if !allow_obs_fold {
                return Err(H1Error::BadRequest(
                    "Obsolete header line folding (obs-fold) rejected".to_string(),
                ));
            }
            let key = last_key.as_ref().ok_or_else(|| {
                H1Error::BadRequest("Leading obs-fold without prior header".to_string())
            })?;
            let folded = validate_header_value(line)?;
            if let Some(vals) = headers.get_mut(key) {
                if let Some(last) = vals.last_mut() {
                    last.push(' ');
                    last.push_str(&folded);
                }
            }
            continue;
        }

        let colon = line
            .iter()
            .position(|&b| b == b':')
            .ok_or_else(|| H1Error::BadRequest("Header line missing ':'".to_string()))?;
        let name = validate_header_name(&line[..colon])?;
        let val = validate_header_value(&line[colon + 1..])?;
        last_key = Some(name.clone());
        headers.entry(name).or_default().push(val);
    }

    Ok(headers)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BodyFraming {
    None,
    Chunked,
    ContentLength(usize),
    UntilEof,
}

fn determine_body_framing(
    headers: &IndexMap<String, Vec<String>>,
    is_request: bool,
    max_body_bytes: usize,
) -> Result<BodyFraming, H1Error> {
    let has_te = headers.contains_key("transfer-encoding");
    let has_cl = headers.contains_key("content-length");

    if has_te && has_cl {
        return Err(H1Error::BadRequest(
            "Both Transfer-Encoding and Content-Length present (smuggling rejected)".to_string(),
        ));
    }

    if let Some(te_vals) = headers.get("transfer-encoding") {
        let mut codings = Vec::new();
        for v in te_vals {
            for part in v.split(',') {
                let c = part.trim().to_ascii_lowercase();
                if !c.is_empty() {
                    codings.push(c);
                }
            }
        }
        if codings.last().map(|s| s.as_str()) != Some("chunked") {
            return Err(H1Error::BadRequest(
                "Transfer-Encoding must end with 'chunked'".to_string(),
            ));
        }
        return Ok(BodyFraming::Chunked);
    }

    if let Some(cl_vals) = headers.get("content-length") {
        let mut parsed_len: Option<usize> = None;
        for v in cl_vals {
            for part in v.split(',') {
                let s = part.trim();
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(H1Error::BadRequest(format!(
                        "Invalid Content-Length value: '{}'",
                        s
                    )));
                }
                let n = s.parse::<usize>().map_err(|_| H1Error::PayloadTooLarge)?;
                if let Some(prev) = parsed_len {
                    if prev != n {
                        return Err(H1Error::BadRequest(
                            "Conflicting Content-Length headers".to_string(),
                        ));
                    }
                } else {
                    parsed_len = Some(n);
                }
            }
        }
        let len = parsed_len.ok_or_else(|| {
            H1Error::BadRequest("Empty Content-Length header".to_string())
        })?;
        if len > max_body_bytes {
            return Err(H1Error::PayloadTooLarge);
        }
        return Ok(BodyFraming::ContentLength(len));
    }

    if is_request {
        Ok(BodyFraming::None)
    } else {
        Ok(BodyFraming::UntilEof)
    }
}

fn read_chunked_body<S: Read + Write>(
    stream: &mut BufStream<S>,
    limits: H1Limits,
) -> Result<(Vec<u8>, IndexMap<String, Vec<String>>), H1Error> {
    let mut body = Vec::new();
    loop {
        let line_bytes = stream.read_line_crlf(4096)?;
        let line_str = String::from_utf8(line_bytes)
            .map_err(|_| H1Error::BadRequest("Non-UTF8 chunk size line".to_string()))?;
        let hex_part = line_str
            .split(';')
            .next()
            .unwrap_or("")
            .trim_end_matches(|c| c == ' ' || c == '\t');
        if hex_part.is_empty() || !hex_part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(H1Error::BadRequest(format!(
                "Invalid chunk size '{}'",
                hex_part
            )));
        }
        let chunk_len = usize::from_str_radix(hex_part, 16)
            .map_err(|_| H1Error::PayloadTooLarge)?;
        if chunk_len == 0 {
            let mut trailers_raw: Vec<Vec<u8>> = Vec::new();
            let mut total_trailer_bytes = 0usize;
            loop {
                let tline = stream.read_line_crlf(limits.max_header_bytes)?;
                if tline.is_empty() {
                    break;
                }
                total_trailer_bytes += tline.len() + 2;
                if total_trailer_bytes > limits.max_header_bytes {
                    return Err(H1Error::HeadersTooLarge);
                }
                trailers_raw.push(tline);
            }
            let refs: Vec<&[u8]> = trailers_raw.iter().map(|v| v.as_slice()).collect();
            let trailers = parse_headers_lines(&refs, false)?;
            for forbidden in ["transfer-encoding", "content-length", "host", "trailer"] {
                if trailers.contains_key(forbidden) {
                    return Err(H1Error::BadRequest(format!(
                        "Forbidden trailer field '{}'",
                        forbidden
                    )));
                }
            }
            return Ok((body, trailers));
        }

        if body.len().saturating_add(chunk_len) > limits.max_body_bytes {
            return Err(H1Error::PayloadTooLarge);
        }
        let chunk_data = stream.read_exact_bytes(chunk_len)?;
        body.extend_from_slice(&chunk_data);
        let crlf = stream.read_exact_bytes(2)?;
        if crlf != b"\r\n" {
            return Err(H1Error::BadRequest(
                "Missing CRLF after chunk payload".to_string(),
            ));
        }
    }
}

fn connection_keep_alive(version: &str, headers: &IndexMap<String, Vec<String>>) -> bool {
    let mut saw_close = false;
    let mut saw_keep_alive = false;
    if let Some(vals) = headers.get("connection") {
        for v in vals {
            for tok in v.split(',') {
                let t = tok.trim().to_ascii_lowercase();
                if t == "close" {
                    saw_close = true;
                } else if t == "keep-alive" {
                    saw_keep_alive = true;
                }
            }
        }
    }
    if version == "HTTP/1.0" {
        saw_keep_alive && !saw_close
    } else {
        !saw_close
    }
}

#[derive(Debug, Clone)]
pub struct H1Request {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: IndexMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub trailers: IndexMap<String, Vec<String>>,
    pub keep_alive: bool,
}

#[derive(Debug, Clone)]
pub struct H1Response {
    pub version: String,
    pub status: u16,
    pub reason: String,
    pub headers: IndexMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub trailers: IndexMap<String, Vec<String>>,
    pub keep_alive: bool,
}

fn split_strict_crlf_lines(block: &[u8]) -> Result<Vec<&[u8]>, H1Error> {
    for (i, &b) in block.iter().enumerate() {
        if b == b'\n' && (i == 0 || block[i - 1] != b'\r') {
            return Err(H1Error::BadRequest(
                "Bare LF rejected in HTTP/1.1 header block".to_string(),
            ));
        }
        if b == b'\r' && block.get(i + 1) != Some(&b'\n') {
            return Err(H1Error::BadRequest(
                "Bare CR rejected in HTTP/1.1 header block".to_string(),
            ));
        }
    }
    let lines: Vec<&[u8]> = block
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .filter(|l| !l.is_empty())
        .collect();
    Ok(lines)
}

pub fn parse_h1_request<S: Read + Write>(
    stream: &mut BufStream<S>,
    limits: H1Limits,
    send_100_continue: bool,
) -> Result<H1Request, H1Error> {
    let block = stream.read_header_block(limits.max_header_bytes, true)?;
    let lines = split_strict_crlf_lines(&block)?;
    if lines.is_empty() {
        return Err(H1Error::BadRequest("Empty request".to_string()));
    }

    let req_line = lines[0];
    let parts: Vec<&[u8]> = req_line.split(|&b| b == b' ').collect();
    if parts.len() != 3 {
        return Err(H1Error::BadRequest(
            "Request line must have exactly 3 SP-delimited fields".to_string(),
        ));
    }
    let method_bytes = parts[0];
    if method_bytes.is_empty() || !method_bytes.iter().all(|&b| is_tchar(b)) {
        return Err(H1Error::BadRequest("Invalid HTTP method".to_string()));
    }
    let method = String::from_utf8_lossy(method_bytes).into_owned();

    let target_bytes = parts[1];
    if target_bytes.is_empty()
        || target_bytes
            .iter()
            .any(|&b| b <= 0x20 || b == 0x7f)
    {
        return Err(H1Error::BadRequest("Invalid request-target".to_string()));
    }
    let target = String::from_utf8_lossy(target_bytes).into_owned();

    let version_bytes = parts[2];
    let version = match version_bytes {
        b"HTTP/1.1" => "HTTP/1.1".to_string(),
        b"HTTP/1.0" => "HTTP/1.0".to_string(),
        other => {
            return Err(H1Error::UnsupportedVersion(
                String::from_utf8_lossy(other).into_owned(),
            ));
        }
    };

    let headers = parse_headers_lines(&lines[1..], false)?;
    if version == "HTTP/1.1" {
        let host_count = headers.get("host").map(|v| v.len()).unwrap_or(0);
        if host_count != 1 {
            return Err(H1Error::BadRequest(
                "HTTP/1.1 request must contain exactly one Host header".to_string(),
            ));
        }
    }

    let framing = determine_body_framing(&headers, true, limits.max_body_bytes)?;

    if send_100_continue && version == "HTTP/1.1" {
        if let Some(exp) = headers.get("expect") {
            if exp.iter().any(|v| v.eq_ignore_ascii_case("100-continue"))
                && !stream.has_buffered_data()
                && framing != BodyFraming::None
            {
                stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
                stream.flush()?;
            }
        }
    }

    let (body, trailers) = match framing {
        BodyFraming::None => (Vec::new(), IndexMap::new()),
        BodyFraming::ContentLength(len) => {
            if len == 0 {
                (Vec::new(), IndexMap::new())
            } else {
                (stream.read_exact_bytes(len)?, IndexMap::new())
            }
        }
        BodyFraming::Chunked => read_chunked_body(stream, limits)?,
        BodyFraming::UntilEof => (Vec::new(), IndexMap::new()),
    };

    let keep_alive = connection_keep_alive(&version, &headers);
    Ok(H1Request {
        method,
        target,
        version,
        headers,
        body,
        trailers,
        keep_alive,
    })
}

pub fn parse_h1_response<S: Read + Write>(
    stream: &mut BufStream<S>,
    req_method: &str,
    limits: H1Limits,
) -> Result<H1Response, H1Error> {
    loop {
        let block = stream.read_header_block(limits.max_header_bytes, false)?;
        let lines = split_strict_crlf_lines(&block)?;
        if lines.is_empty() {
            return Err(H1Error::BadRequest("Empty status block".to_string()));
        }

        let status_line = lines[0];
        let mut split_iter = status_line.splitn(3, |&b| b == b' ');
        let version_b = split_iter
            .next()
            .ok_or_else(|| H1Error::BadRequest("Missing HTTP version".to_string()))?;
        let code_b = split_iter
            .next()
            .ok_or_else(|| H1Error::BadRequest("Missing status code".to_string()))?;
        let reason_b = split_iter.next().unwrap_or(b"");

        if !version_b.starts_with(b"HTTP/1.") || version_b.len() != 8 {
            return Err(H1Error::UnsupportedVersion(
                String::from_utf8_lossy(version_b).into_owned(),
            ));
        }
        let version = String::from_utf8_lossy(version_b).into_owned();
        if code_b.len() != 3 || !code_b.iter().all(|b| b.is_ascii_digit()) {
            return Err(H1Error::BadRequest("Invalid 3-digit status code".to_string()));
        }
        let status: u16 = std::str::from_utf8(code_b)
            .unwrap()
            .parse()
            .map_err(|_| H1Error::BadRequest("Invalid status code".to_string()))?;
        if !(100..=599).contains(&status) {
            return Err(H1Error::BadRequest("Status code out of range".to_string()));
        }
        let reason = String::from_utf8_lossy(reason_b).into_owned();
        let headers = parse_headers_lines(&lines[1..], true)?;

        // Interim 1xx responses (RFC 9112 §6.3): 0 body bytes, continue reading final response
        if (100..200).contains(&status) && status != 101 {
            continue;
        }

        let framing = determine_body_framing(&headers, false, limits.max_body_bytes)?;
        let is_bodyless = req_method.eq_ignore_ascii_case("HEAD")
            || (100..200).contains(&status)
            || status == 204
            || status == 304;

        let mut keep_alive = connection_keep_alive(&version, &headers);
        let (body, trailers) = if is_bodyless {
            (Vec::new(), IndexMap::new())
        } else {
            match framing {
                BodyFraming::None => (Vec::new(), IndexMap::new()),
                BodyFraming::ContentLength(len) => {
                    if len == 0 {
                        (Vec::new(), IndexMap::new())
                    } else {
                        (stream.read_exact_bytes(len)?, IndexMap::new())
                    }
                }
                BodyFraming::Chunked => read_chunked_body(stream, limits)?,
                BodyFraming::UntilEof => {
                    keep_alive = false;
                    (stream.read_until_eof(limits.max_body_bytes)?, IndexMap::new())
                }
            }
        };

        return Ok(H1Response {
            version,
            status,
            reason,
            headers,
            body,
            trailers,
            keep_alive,
        });
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "OK",
    }
}

pub fn serialize_h1_request(
    method: &str,
    target: &str,
    host: &str,
    headers: &IndexMap<String, Vec<String>>,
    body: &[u8],
    keep_alive: bool,
) -> Result<Vec<u8>, H1Error> {
    if !method.as_bytes().iter().all(|&b| is_tchar(b)) || method.is_empty() {
        return Err(H1Error::BadRequest(format!("Invalid method: {}", method)));
    }
    if target.is_empty() || target.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err(H1Error::BadRequest(format!(
            "Invalid HTTP/1.1 request target: {:?}",
            target
        )));
    }
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(format!("{} {} HTTP/1.1\r\n", method, target).as_bytes());

    let mut has_host = false;
    let mut has_cl = false;
    let mut has_te = false;
    let mut has_conn = false;

    for (k, vals) in headers {
        let lower = k.to_ascii_lowercase();
        if lower == "host" {
            has_host = true;
        } else if lower == "content-length" {
            has_cl = true;
        } else if lower == "transfer-encoding" {
            has_te = true;
        } else if lower == "connection" {
            has_conn = true;
        }
        for v in vals {
            validate_outbound_header(&lower, v)?;
            out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
        }
    }

    if !has_host {
        validate_outbound_header("host", host)?;
        out.extend_from_slice(format!("host: {}\r\n", host).as_bytes());
    }
    if !has_conn {
        out.extend_from_slice(
            if keep_alive {
                b"connection: keep-alive\r\n".as_slice()
            } else {
                b"connection: close\r\n".as_slice()
            },
        );
    }
    if !has_te && !has_cl && (!body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH")) {
        out.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    Ok(out)
}

pub fn serialize_h1_response(
    resp: &WireResponse,
    req_method: &str,
    keep_alive: bool,
) -> Result<Vec<u8>, H1Error> {
    let mut out = Vec::with_capacity(256 + resp.body.len());
    out.extend_from_slice(
        format!("HTTP/1.1 {} {}\r\n", resp.status, reason_phrase(resp.status)).as_bytes(),
    );

    let bodyless_status = (100..200).contains(&resp.status) || resp.status == 204 || resp.status == 304;
    let is_head = req_method.eq_ignore_ascii_case("HEAD");
    let explicit_chunked = resp.headers.iter().any(|(k, vals)| {
        k.eq_ignore_ascii_case("transfer-encoding")
            && vals
                .iter()
                .any(|v| v.to_ascii_lowercase().contains("chunked"))
    });
    let use_chunked = !bodyless_status && !is_head && (!resp.trailers.is_empty() || explicit_chunked);

    let mut has_cl = false;
    let mut has_conn = false;
    let mut has_te = false;
    for (k, vals) in &resp.headers {
        let lower = k.to_ascii_lowercase();
        if lower == "content-length" {
            if bodyless_status || use_chunked {
                continue;
            }
            has_cl = true;
        } else if lower == "transfer-encoding" {
            if bodyless_status {
                continue;
            }
            has_te = true;
        } else if lower == "connection" {
            has_conn = true;
        }
        for v in vals {
            validate_outbound_header(&lower, v)?;
            out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
        }
    }

    if !has_conn {
        out.extend_from_slice(
            if keep_alive {
                b"connection: keep-alive\r\n".as_slice()
            } else {
                b"connection: close\r\n".as_slice()
            },
        );
    }

    if !bodyless_status {
        if use_chunked {
            if !has_te {
                out.extend_from_slice(b"transfer-encoding: chunked\r\n");
            }
        } else if !has_cl && !has_te {
            out.extend_from_slice(format!("content-length: {}\r\n", resp.body.len()).as_bytes());
        }
    }

    out.extend_from_slice(b"\r\n");

    if !bodyless_status && !is_head {
        if use_chunked {
            if !resp.body.is_empty() {
                out.extend_from_slice(format!("{:x}\r\n", resp.body.len()).as_bytes());
                out.extend_from_slice(&resp.body);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"0\r\n");
            for (k, vals) in &resp.trailers {
                for v in vals {
                    validate_outbound_header(k, v)?;
                    out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
                }
            }
            out.extend_from_slice(b"\r\n");
        } else {
            out.extend_from_slice(&resp.body);
        }
    }

    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerProtocol {
    Http1,
    Http2,
}

struct ServerStats {
    active_connections: AtomicUsize,
    total_connections: AtomicU64,
    total_requests: AtomicU64,
}

impl Default for ServerStats {
    fn default() -> Self {
        Self {
            active_connections: AtomicUsize::new(0),
            total_connections: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
        }
    }
}

pub struct HttpServerHandle {
    pub id: u64,
    protocol: Mutex<ServerProtocol>,
    router: Arc<RwLock<RouterState>>,
    worker_ctx: Arc<RwLock<Option<WorkerEnvContext>>>,
    running: Arc<AtomicBool>,
    bind_addr: Mutex<Option<SocketAddr>>,
    accept_handle: Mutex<Option<JoinHandle<()>>>,
    tls_config: Mutex<Option<Arc<ServerConfig>>>,
    stats: Arc<ServerStats>,
}

struct ConnGuard(Arc<ServerStats>);
impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

impl HttpServerHandle {
    fn new(id: u64, protocol: ServerProtocol) -> Self {
        Self {
            id,
            protocol: Mutex::new(protocol),
            router: Arc::new(RwLock::new(RouterState::default())),
            worker_ctx: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            bind_addr: Mutex::new(None),
            accept_handle: Mutex::new(None),
            tls_config: Mutex::new(None),
            stats: Arc::new(ServerStats::default()),
        }
    }

    pub fn bind_and_start(&self, addr: &str) -> Result<SocketAddr, String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("HTTP server is already running".to_string());
        }
        let listener =
            TcpListener::bind(addr).map_err(|e| format!("Failed to bind '{}': {}", addr, e))?;
        let local = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local_addr: {}", e))?;
        *self.bind_addr.lock().unwrap() = Some(local);
        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let router = self.router.clone();
        let worker_ctx = self.worker_ctx.clone();
        let stats = self.stats.clone();
        let proto = *self.protocol.lock().unwrap();
        let tls_cfg = self.tls_config.lock().unwrap().clone();

        let jh = thread::spawn(move || {
            const MAX_CONNS: usize = 256;
            while running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        if !running.load(Ordering::SeqCst) {
                            drop(stream);
                            break;
                        }
                        if stats.active_connections.load(Ordering::Relaxed) >= MAX_CONNS {
                            drop(stream);
                            continue;
                        }
                        stats.active_connections.fetch_add(1, Ordering::Relaxed);
                        stats.total_connections.fetch_add(1, Ordering::Relaxed);
                        let r_clone = router.clone();
                        let ctx_clone = worker_ctx
                            .read()
                            .unwrap()
                            .clone()
                            .unwrap_or(WorkerEnvContext {
                                env_root: None,
                                current_file: None,
                                embedded_archive: None,
                                embedded_bytecode: None,
                            });
                        let run_clone = running.clone();
                        let stats_clone = stats.clone();
                        let tls_clone = tls_cfg.clone();

                        let _ = thread::Builder::new().name("shae-http-conn".to_string()).spawn(move || {
                            let _guard = ConnGuard(stats_clone.clone());
                            match proto {
                                ServerProtocol::Http1 => {
                                    serve_h1_connection(
                                        stream,
                                        tls_clone,
                                        &r_clone,
                                        &ctx_clone,
                                        &run_clone,
                                        &stats_clone,
                                    );
                                }
                                ServerProtocol::Http2 => {
                                    serve_h2_connection(
                                        stream,
                                        tls_clone,
                                        &r_clone,
                                        &ctx_clone,
                                        &run_clone,
                                        &stats_clone,
                                    );
                                }
                            }
                        });
                    }
                    Err(_) => {
                        if !running.load(Ordering::SeqCst) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        });

        *self.accept_handle.lock().unwrap() = Some(jh);
        Ok(local)
    }

    pub fn stop(&self) -> bool {
        if !self.running.swap(false, Ordering::SeqCst) {
            return false;
        }
        if let Some(addr) = *self.bind_addr.lock().unwrap() {
            let wake = wake_addr_for(addr);
            let _ = TcpStream::connect_timeout(&wake, Duration::from_millis(150));
        }
        if let Some(jh) = self.accept_handle.lock().unwrap().take() {
            let _ = jh.join();
        }
        true
    }

    pub fn port(&self) -> u16 {
        self.bind_addr.lock().unwrap().map(|a| a.port()).unwrap_or(0)
    }

    pub fn local_addr_str(&self) -> String {
        self.bind_addr
            .lock()
            .unwrap()
            .map(|a| a.to_string())
            .unwrap_or_default()
    }
}

impl Drop for HttpServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

enum ServerIo {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ServerConnection, TcpStream>),
}

impl Read for ServerIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for ServerIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

fn serve_h1_connection(
    tcp: TcpStream,
    tls_cfg: Option<Arc<ServerConfig>>,
    router: &Arc<RwLock<RouterState>>,
    ctx: &WorkerEnvContext,
    running: &Arc<AtomicBool>,
    stats: &Arc<ServerStats>,
) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(200)));

    let io_stream = if let Some(cfg) = tls_cfg {
        match rustls::ServerConnection::new(cfg) {
            Ok(mut conn) => {
                let mut stream = tcp;
                if conn.complete_io(&mut stream).is_err() {
                    return;
                }
                ServerIo::Tls(rustls::StreamOwned::new(conn, stream))
            }
            Err(_) => return,
        }
    } else {
        ServerIo::Plain(tcp)
    };

    let mut buf = BufStream::new(io_stream).with_running(running.clone());
    let limits = H1Limits::default();
    let keepalive_idle_limit = Duration::from_secs(5);
    let mut idle_since = Instant::now();
    let mut req_count = 0usize;

    while running.load(Ordering::Acquire) && req_count < 1000 {
        match parse_h1_request(&mut buf, limits, true) {
            Ok(req) => {
                idle_since = Instant::now();
                req_count += 1;
                stats.total_requests.fetch_add(1, Ordering::Relaxed);

                let router_snap = router.read().unwrap().clone();
                let resp = dispatch_shae_handler(
                    &router_snap,
                    ctx,
                    &req.method,
                    &req.target,
                    &req.headers,
                    &req.body,
                    &req.trailers,
                    &req.version,
                    0,
                );
                let keep_alive = req.keep_alive && running.load(Ordering::Acquire) && req_count < 1000;
                if let Ok(wire) = serialize_h1_response(&resp, &req.method, keep_alive) {
                    if buf.write_all(&wire).is_err() || buf.flush().is_err() {
                        break;
                    }
                } else {
                    break;
                }
                if !keep_alive {
                    break;
                }
            }
            Err(H1Error::IdleTimeout) if running.load(Ordering::Acquire) => {
                if idle_since.elapsed() >= keepalive_idle_limit {
                    break;
                }
                continue;
            }
            Err(H1Error::IdleTimeout) | Err(H1Error::Eof) | Err(H1Error::Io(_)) => break,
            Err(err) => {
                let (status, msg) = match err {
                    H1Error::HeadersTooLarge => (431, "Request Header Fields Too Large"),
                    H1Error::PayloadTooLarge => (413, "Payload Too Large"),
                    H1Error::RequestTimeout => (408, "Request Timeout"),
                    H1Error::UnsupportedVersion(_) => (505, "HTTP Version Not Supported"),
                    _ => (400, "Bad Request"),
                };
                let wire = format!(
                    "HTTP/1.1 {} {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status,
                    msg,
                    msg.len(),
                    msg
                );
                let _ = buf.write_all(wire.as_bytes());
                let _ = buf.flush();
                break;
            }
        }
    }
}

enum PoolStream {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl PoolStream {
    fn tcp_ref(&self) -> &TcpStream {
        match self {
            Self::Plain(s) => s,
            Self::Tls(s) => &s.sock,
        }
    }

    fn is_conn_alive(&self) -> bool {
        let tcp = self.tcp_ref();
        if tcp.set_nonblocking(true).is_err() {
            return false;
        }
        let mut probe = [0u8; 1];
        let res = tcp.peek(&mut probe);
        // Always restore blocking mode before returning!
        let _ = tcp.set_nonblocking(false);
        matches!(res, Err(ref e) if e.kind() == ErrorKind::WouldBlock)
    }
}

impl Read for PoolStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for PoolStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

struct PooledConn {
    stream: BufStream<PoolStream>,
    last_used: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    scheme: String,
    host: String,
    port: u16,
}

pub struct HttpClientPool {
    pub id: u64,
    idle: Mutex<HashMap<PoolKey, VecDeque<PooledConn>>>,
    max_idle_per_host: usize,
    idle_timeout: Duration,
    connect_timeout: Duration,
    read_timeout: Duration,
    insecure_tls: bool,
    ca_pem: Option<String>,
    hits: AtomicU64,
    misses: AtomicU64,
    retries: AtomicU64,
    stale_evictions: AtomicU64,
    connections_opened: AtomicU64,
    active: AtomicUsize,
}

struct ActivePoolGuard<'a>(&'a AtomicUsize);
impl<'a> Drop for ActivePoolGuard<'a> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl HttpClientPool {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            idle: Mutex::new(HashMap::new()),
            max_idle_per_host: 16,
            idle_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(10),
            insecure_tls: false,
            ca_pem: None,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            stale_evictions: AtomicU64::new(0),
            connections_opened: AtomicU64::new(0),
            active: AtomicUsize::new(0),
        }
    }

    fn open_fresh(&self, key: &PoolKey, insecure: bool) -> Result<BufStream<PoolStream>, String> {
        use std::net::ToSocketAddrs;
        let addr_str = format_socket_addr(&key.host, key.port);
        let addr = addr_str
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolution failed for '{}': {}", addr_str, e))?
            .next()
            .ok_or_else(|| format!("No socket addresses resolved for '{}'", addr_str))?;

        let tcp = TcpStream::connect_timeout(&addr, self.connect_timeout)
            .map_err(|e| format!("TCP connect failed to '{}': {}", addr_str, e))?;
        let _ = tcp.set_nodelay(true);
        let _ = tcp.set_read_timeout(Some(self.read_timeout));
        let _ = tcp.set_write_timeout(Some(self.read_timeout));
        self.connections_opened.fetch_add(1, Ordering::Relaxed);

        let ps = if key.scheme == "https" {
            let cfg = build_tls_client_config(
                self.ca_pem.as_deref(),
                insecure || self.insecure_tls,
                vec![b"http/1.1".to_vec()],
            )?;
            let sni = ServerName::try_from(key.host.clone())
                .map_err(|_| format!("Invalid SNI host '{}'", key.host))?;
            let conn = rustls::ClientConnection::new(cfg, sni)
                .map_err(|e| format!("TLS ClientConnection error: {}", e))?;
            PoolStream::Tls(rustls::StreamOwned::new(conn, tcp))
        } else {
            PoolStream::Plain(tcp)
        };

        Ok(BufStream::new(ps))
    }

    fn checkout(&self, key: &PoolKey, insecure: bool) -> Result<(BufStream<PoolStream>, bool), String> {
        loop {
            let candidate = {
                let mut guard = self.idle.lock().unwrap();
                guard.get_mut(key).and_then(|q| q.pop_front())
            };
            let Some(conn) = candidate else {
                self.misses.fetch_add(1, Ordering::Relaxed);
                let fresh = self.open_fresh(key, insecure)?;
                return Ok((fresh, false));
            };
            if conn.last_used.elapsed() > self.idle_timeout || !conn.stream.inner.is_conn_alive() {
                self.stale_evictions.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok((conn.stream, true));
        }
    }

    fn checkin(&self, key: PoolKey, conn: BufStream<PoolStream>) {
        let mut guard = self.idle.lock().unwrap();
        let q = guard.entry(key).or_default();
        if q.len() < self.max_idle_per_host {
            q.push_back(PooledConn {
                stream: conn,
                last_used: Instant::now(),
            });
        }
    }

    pub fn request(
        &self,
        method: &str,
        url: &str,
        headers: &IndexMap<String, Vec<String>>,
        body: &[u8],
        retry_on_stale: bool,
        insecure: bool,
    ) -> Result<H1Response, String> {
        let parsed = parse_url(url)?;
        let key = PoolKey {
            scheme: parsed.scheme.clone(),
            host: parsed.host.clone(),
            port: parsed.port,
        };
        let method_upper = method.to_ascii_uppercase();
        let wire = serialize_h1_request(
            &method_upper,
            &parsed.path_and_query,
            &parsed.authority,
            headers,
            body,
            true,
        )
        .map_err(|e| e.to_string())?;

        self.active.fetch_add(1, Ordering::Relaxed);
        let _guard = ActivePoolGuard(&self.active);

        let (mut stream, was_reused) = self.checkout(&key, insecure)?;
        let mut attempts = 0usize;

        loop {
            let write_err = stream.write_all(&wire).and_then(|_| stream.flush());
            if let Err(e) = write_err {
                if attempts == 0 && was_reused {
                    attempts += 1;
                    self.retries.fetch_add(1, Ordering::Relaxed);
                    stream = self.open_fresh(&key, insecure)?;
                    continue;
                }
                return Err(format!("HTTP/1.1 write failed: {}", e));
            }

            match parse_h1_response(&mut stream, &method_upper, H1Limits::default()) {
                Ok(resp) => {
                    if resp.keep_alive && !stream.has_buffered_data() {
                        self.checkin(key, stream);
                    }
                    return Ok(resp);
                }
                Err(H1Error::Eof) | Err(H1Error::UnexpectedEof) | Err(H1Error::Io(_))
                    if attempts == 0
                        && was_reused
                        && (retry_on_stale
                            || matches!(
                                method_upper.as_str(),
                                "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
                            )) =>
                {
                    attempts += 1;
                    self.retries.fetch_add(1, Ordering::Relaxed);
                    stream = self.open_fresh(&key, insecure)?;
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    pub fn pipeline(
        &self,
        base_url: &str,
        reqs: &[(String, String, IndexMap<String, Vec<String>>, Vec<u8>)],
        insecure: bool,
    ) -> Result<Vec<H1Response>, String> {
        if reqs.is_empty() {
            return Ok(Vec::new());
        }
        let base = parse_url(base_url)?;
        let key = PoolKey {
            scheme: base.scheme.clone(),
            host: base.host.clone(),
            port: base.port,
        };

        self.active.fetch_add(1, Ordering::Relaxed);
        let _guard = ActivePoolGuard(&self.active);

        let (mut stream, _reused) = self.checkout(&key, insecure)?;
        let mut batch = Vec::new();
        let mut methods = Vec::with_capacity(reqs.len());
        for (m, path, hdrs, body) in reqs {
            let mu = m.to_ascii_uppercase();
            let target = if path.starts_with('/') {
                path.clone()
            } else {
                format!("/{}", path)
            };
            let w = serialize_h1_request(&mu, &target, &base.authority, hdrs, body, true)
                .map_err(|e| e.to_string())?;
            batch.extend_from_slice(&w);
            methods.push(mu);
        }

        stream
            .write_all(&batch)
            .and_then(|_| stream.flush())
            .map_err(|e| format!("Pipeline write failed: {}", e))?;

        let mut responses = Vec::with_capacity(reqs.len());
        let mut closed_early = false;
        for (i, mu) in methods.iter().enumerate() {
            let resp = parse_h1_response(&mut stream, mu, H1Limits::default())
                .map_err(|e| format!("Pipeline response {} failed: {}", i, e))?;
            let ka = resp.keep_alive;
            responses.push(resp);
            if !ka {
                closed_early = true;
                if i + 1 < methods.len() {
                    return Err(format!(
                        "Server closed connection mid-pipeline at response {}/{}",
                        i + 1,
                        methods.len()
                    ));
                }
            }
        }

        if !closed_early && !stream.has_buffered_data() {
            self.checkin(key, stream);
        }
        Ok(responses)
    }

    pub fn clear(&self) {
        self.idle.lock().unwrap().clear();
    }

    pub fn stats_map(&self) -> Value {
        let idle_count: usize = self.idle.lock().unwrap().values().map(|q| q.len()).sum();
        let mut map = IndexMap::new();
        map.insert("active".to_string(), Value::Int(self.active.load(Ordering::Relaxed) as i64));
        map.insert("idle".to_string(), Value::Int(idle_count as i64));
        map.insert("hits".to_string(), Value::Int(self.hits.load(Ordering::Relaxed) as i64));
        map.insert("misses".to_string(), Value::Int(self.misses.load(Ordering::Relaxed) as i64));
        map.insert("retries".to_string(), Value::Int(self.retries.load(Ordering::Relaxed) as i64));
        map.insert(
            "stale_evictions".to_string(),
            Value::Int(self.stale_evictions.load(Ordering::Relaxed) as i64),
        );
        map.insert(
            "connections_opened".to_string(),
            Value::Int(self.connections_opened.load(Ordering::Relaxed) as i64),
        );
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

// ============================================================================
// Part 2: HPACK (RFC 7541) & HTTP/2 Framing / Multiplexing (RFC 7540)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H2ErrorCode {
    NoError = 0x0,
    ProtocolError = 0x1,
    InternalError = 0x2,
    FlowControlError = 0x3,
    SettingsTimeout = 0x4,
    StreamClosed = 0x5,
    FrameSizeError = 0x6,
    RefusedStream = 0x7,
    Cancel = 0x8,
    CompressionError = 0x9,
    ConnectError = 0xa,
    EnhanceYourCalm = 0xb,
    InadequateSecurity = 0xc,
    Http11Required = 0xd,
}

impl H2ErrorCode {
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H2Error {
    pub code: H2ErrorCode,
    pub message: String,
    pub stream_id: Option<u32>,
}

impl H2Error {
    pub fn conn(code: H2ErrorCode, msg: impl Into<String>) -> Self {
        Self {
            code,
            message: msg.into(),
            stream_id: None,
        }
    }

    pub fn stream(code: H2ErrorCode, sid: u32, msg: impl Into<String>) -> Self {
        Self {
            code,
            message: msg.into(),
            stream_id: Some(sid),
        }
    }
}

impl std::fmt::Display for H2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP/2 {:?}: {}", self.code, self.message)
    }
}

// RFC 7541 Appendix A: 61-entry Static Table (1-based index 1..=61)
pub const HPACK_STATIC_TABLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

// RFC 7541 Appendix B: 256 byte symbols + EOS (index 256)
#[rustfmt::skip]
pub const HPACK_HUFFMAN_CODES: [(u32, u8); 257] = [
    (0x1ff8, 13),     (0x7fffd8, 23),   (0xfffffe2, 28),  (0xfffffe3, 28),
    (0xfffffe4, 28),  (0xfffffe5, 28),  (0xfffffe6, 28),  (0xfffffe7, 28),
    (0xfffffe8, 28),  (0xffffea, 24),   (0x3ffffffc, 30), (0xfffffe9, 28),
    (0xfffffea, 28),  (0x3ffffffd, 30), (0xfffffeb, 28),  (0xfffffec, 28),
    (0xfffffed, 28),  (0xfffffee, 28),  (0xfffffef, 28),  (0xffffff0, 28),
    (0xffffff1, 28),  (0xffffff2, 28),  (0x3ffffffe, 30), (0xffffff3, 28),
    (0xffffff4, 28),  (0xffffff5, 28),  (0xffffff6, 28),  (0xffffff7, 28),
    (0xffffff8, 28),  (0xffffff9, 28),  (0xffffffa, 28),  (0xffffffb, 28),
    (0x14, 6),        (0x3f8, 10),      (0x3f9, 10),      (0xffa, 12),
    (0x1ff9, 13),     (0x15, 6),        (0xf8, 8),        (0x7fa, 11),
    (0x3fa, 10),      (0x3fb, 10),      (0xf9, 8),        (0x7fb, 11),
    (0xfa, 8),        (0x16, 6),        (0x17, 6),        (0x18, 6),
    (0x0, 5),         (0x1, 5),         (0x2, 5),         (0x19, 6),
    (0x1a, 6),        (0x1b, 6),        (0x1c, 6),        (0x1d, 6),
    (0x1e, 6),        (0x1f, 6),        (0x5c, 7),        (0xfb, 8),
    (0x7ffc, 15),     (0x20, 6),        (0xffb, 12),      (0x3fc, 10),
    (0x1ffa, 13),     (0x21, 6),        (0x5d, 7),        (0x5e, 7),
    (0x5f, 7),        (0x60, 7),        (0x61, 7),        (0x62, 7),
    (0x63, 7),        (0x64, 7),        (0x65, 7),        (0x66, 7),
    (0x67, 7),        (0x68, 7),        (0x69, 7),        (0x6a, 7),
    (0x6b, 7),        (0x6c, 7),        (0x6d, 7),        (0x6e, 7),
    (0x6f, 7),        (0x70, 7),        (0x71, 7),        (0x72, 7),
    (0xfc, 8),        (0x73, 7),        (0xfd, 8),        (0x1ffb, 13),
    (0x7fff0, 19),    (0x1ffc, 13),     (0x3ffc, 14),     (0x22, 6),
    (0x7ffd, 15),     (0x3, 5),         (0x23, 6),        (0x4, 5),
    (0x24, 6),        (0x5, 5),         (0x25, 6),        (0x26, 6),
    (0x27, 6),        (0x6, 5),         (0x74, 7),        (0x75, 7),
    (0x28, 6),        (0x29, 6),        (0x2a, 6),        (0x7, 5),
    (0x2b, 6),        (0x76, 7),        (0x2c, 6),        (0x8, 5),
    (0x9, 5),         (0x2d, 6),        (0x77, 7),        (0x78, 7),
    (0x79, 7),        (0x7a, 7),        (0x7b, 7),        (0x7ffe, 15),
    (0x7fc, 11),      (0x3ffd, 14),     (0x1ffd, 13),     (0xffffffc, 28),
    (0xfffe6, 20),    (0x3fffd2, 22),   (0xfffe7, 20),    (0xfffe8, 20),
    (0x3fffd3, 22),   (0x3fffd4, 22),   (0x3fffd5, 22),   (0x7fffd9, 23),
    (0x3fffd6, 22),   (0x7fffda, 23),   (0x7fffdb, 23),   (0x7fffdc, 23),
    (0x7fffdd, 23),   (0x7fffde, 23),   (0xffffeb, 24),   (0x7fffdf, 23),
    (0xffffec, 24),   (0xffffed, 24),   (0x3fffd7, 22),   (0x7fffe0, 23),
    (0xffffee, 24),   (0x7fffe1, 23),   (0x7fffe2, 23),   (0x7fffe3, 23),
    (0x7fffe4, 23),   (0x1fffdc, 21),   (0x3fffd8, 22),   (0x7fffe5, 23),
    (0x3fffd9, 22),   (0x7fffe6, 23),   (0x7fffe7, 23),   (0xffffef, 24),
    (0x3fffda, 22),   (0x1fffdd, 21),   (0xfffe9, 20),    (0x3fffdb, 22),
    (0x3fffdc, 22),   (0x7fffe8, 23),   (0x7fffe9, 23),   (0x1fffde, 21),
    (0x7fffea, 23),   (0x3fffdd, 22),   (0x3fffde, 22),   (0xfffff0, 24),
    (0x1fffdf, 21),   (0x3fffdf, 22),   (0x7fffeb, 23),   (0x7fffec, 23),
    (0x1fffe0, 21),   (0x1fffe1, 21),   (0x3fffe0, 22),   (0x1fffe2, 21),
    (0x7fffed, 23),   (0x3fffe1, 22),   (0x7fffee, 23),   (0x7fffef, 23),
    (0xfffea, 20),    (0x3fffe2, 22),   (0x3fffe3, 22),   (0x3fffe4, 22),
    (0x7ffff0, 23),   (0x3fffe5, 22),   (0x3fffe6, 22),   (0x7ffff1, 23),
    (0x3ffffe0, 26),  (0x3ffffe1, 26),  (0xfffeb, 20),    (0x7fff1, 19),
    (0x3fffe7, 22),   (0x7ffff2, 23),   (0x3fffe8, 22),   (0x1ffffec, 25),
    (0x3ffffe2, 26),  (0x3ffffe3, 26),  (0x3ffffe4, 26),  (0x7ffffde, 27),
    (0x7ffffdf, 27),  (0x3ffffe5, 26),  (0xfffff1, 24),   (0x1ffffed, 25),
    (0x7fff2, 19),    (0x1fffe3, 21),   (0x3ffffe6, 26),  (0x7ffffe0, 27),
    (0x7ffffe1, 27),  (0x3ffffe7, 26),  (0x7ffffe2, 27),  (0xfffff2, 24),
    (0x1fffe4, 21),   (0x1fffe5, 21),   (0x3ffffe8, 26),  (0x3ffffe9, 26),
    (0xffffffd, 28),  (0x7ffffe3, 27),  (0x7ffffe4, 27),  (0x7ffffe5, 27),
    (0xfffec, 20),    (0xfffff3, 24),   (0xfffed, 20),    (0x1fffe6, 21),
    (0x3fffe9, 22),   (0x1fffe7, 21),   (0x1fffe8, 21),   (0x7ffff3, 23),
    (0x3fffea, 22),   (0x3fffeb, 22),   (0x1ffffee, 25),  (0x1ffffef, 25),
    (0xfffff4, 24),   (0xfffff5, 24),   (0x3ffffea, 26),  (0x7ffff4, 23),
    (0x3ffffeb, 26),  (0x7ffffe6, 27),  (0x3ffffec, 26),  (0x3ffffed, 26),
    (0x7ffffe7, 27),  (0x7ffffe8, 27),  (0x7ffffe9, 27),  (0x7ffffea, 27),
    (0x7ffffeb, 27),  (0xffffffe, 28),  (0x7ffffec, 27),  (0x7ffffed, 27),
    (0x7ffffee, 27),  (0x7ffffef, 27),  (0x7fffff0, 27),  (0x3ffffee, 26),
    (0x3fffffff, 30),
];

#[derive(Clone, Copy, Default)]
struct HuffNode {
    children: [u16; 2], // 0 = unset
    symbol: Option<u16>,
}

static HUFF_TRIE: OnceLock<Vec<HuffNode>> = OnceLock::new();

fn huff_trie() -> &'static [HuffNode] {
    HUFF_TRIE.get_or_init(|| {
        let mut nodes = vec![HuffNode::default()];
        for (sym, &(code, len)) in HPACK_HUFFMAN_CODES.iter().enumerate() {
            let mut cur = 0usize;
            for shift in (0..len).rev() {
                let bit = ((code >> shift) & 1) as usize;
                if nodes[cur].children[bit] == 0 {
                    let next_idx = nodes.len() as u16;
                    nodes.push(HuffNode::default());
                    nodes[cur].children[bit] = next_idx;
                }
                cur = nodes[cur].children[bit] as usize;
            }
            nodes[cur].symbol = Some(sym as u16);
        }
        nodes
    })
}

pub fn hpack_huffman_encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut acc: u64 = 0;
    let mut bits: u8 = 0;
    for &b in input {
        let (code, len) = HPACK_HUFFMAN_CODES[b as usize];
        acc = (acc << len) | (code as u64);
        bits += len;
        while bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1u64 << bits) - 1;
        }
    }
    if bits > 0 {
        let rem = 8 - bits;
        let last = ((acc << rem) | ((1u64 << rem) - 1)) as u8;
        out.push(last);
    }
    out
}

pub fn hpack_huffman_decode(input: &[u8]) -> Result<Vec<u8>, H2Error> {
    let trie = huff_trie();
    let mut out = Vec::with_capacity(input.len() * 2);
    let mut node = 0usize;
    let mut pending_bits = 0u8;
    let mut all_ones = true;

    for &byte in input {
        for shift in (0..8).rev() {
            let bit = ((byte >> shift) & 1) as usize;
            if bit == 0 {
                all_ones = false;
            }
            pending_bits += 1;
            let next = trie[node].children[bit] as usize;
            if next == 0 {
                return Err(H2Error::conn(
                    H2ErrorCode::CompressionError,
                    "Invalid Huffman prefix code",
                ));
            }
            node = next;
            if let Some(sym) = trie[node].symbol {
                if sym == 256 {
                    return Err(H2Error::conn(
                        H2ErrorCode::CompressionError,
                        "Literal EOS symbol in Huffman stream",
                    ));
                }
                out.push(sym as u8);
                node = 0;
                pending_bits = 0;
                all_ones = true;
            }
        }
    }

    if pending_bits > 0 && (pending_bits > 7 || !all_ones) {
        return Err(H2Error::conn(
            H2ErrorCode::CompressionError,
            "Invalid Huffman EOS padding",
        ));
    }

    Ok(out)
}

pub fn hpack_encode_int(out: &mut Vec<u8>, prefix_bits: u8, prefix_byte: u8, mut val: u64) {
    let mask = ((1u16 << prefix_bits) - 1) as u64;
    if val < mask {
        out.push(prefix_byte | (val as u8));
        return;
    }
    out.push(prefix_byte | (mask as u8));
    val -= mask;
    while val >= 128 {
        out.push(((val & 0x7f) as u8) | 0x80);
        val >>= 7;
    }
    out.push(val as u8);
}

pub fn hpack_decode_int(
    buf: &[u8],
    pos: &mut usize,
    prefix_bits: u8,
) -> Result<u64, H2Error> {
    if *pos >= buf.len() {
        return Err(H2Error::conn(
            H2ErrorCode::CompressionError,
            "Unexpected end of HPACK integer",
        ));
    }
    let mask = ((1u16 << prefix_bits) - 1) as u8;
    let first = buf[*pos] & mask;
    *pos += 1;
    if first < mask {
        return Ok(first as u64);
    }
    let mut val = mask as u64;
    let mut m: u32 = 0;
    loop {
        if *pos >= buf.len() {
            return Err(H2Error::conn(
                H2ErrorCode::CompressionError,
                "Truncated HPACK integer",
            ));
        }
        let b = buf[*pos];
        *pos += 1;
        if m >= 63 {
            return Err(H2Error::conn(
                H2ErrorCode::CompressionError,
                "HPACK integer overflow",
            ));
        }
        let add_part = ((b & 0x7f) as u64)
            .checked_shl(m)
            .ok_or_else(|| H2Error::conn(H2ErrorCode::CompressionError, "HPACK int shift overflow"))?;
        val = val
            .checked_add(add_part)
            .ok_or_else(|| H2Error::conn(H2ErrorCode::CompressionError, "HPACK int add overflow"))?;
        m += 7;
        if (b & 0x80) == 0 {
            break;
        }
    }
    Ok(val)
}

fn hpack_encode_str(out: &mut Vec<u8>, s: &[u8], use_huffman: bool) {
    if use_huffman {
        let huff = hpack_huffman_encode(s);
        if huff.len() < s.len() {
            hpack_encode_int(out, 7, 0x80, huff.len() as u64);
            out.extend_from_slice(&huff);
            return;
        }
    }
    hpack_encode_int(out, 7, 0x00, s.len() as u64);
    out.extend_from_slice(s);
}

fn hpack_decode_str(buf: &[u8], pos: &mut usize) -> Result<String, H2Error> {
    if *pos >= buf.len() {
        return Err(H2Error::conn(
            H2ErrorCode::CompressionError,
            "Unexpected end of HPACK string",
        ));
    }
    let is_huff = (buf[*pos] & 0x80) != 0;
    let len = hpack_decode_int(buf, pos, 7)? as usize;
    if pos.checked_add(len).map_or(true, |end| end > buf.len()) {
        return Err(H2Error::conn(
            H2ErrorCode::CompressionError,
            "Truncated HPACK string payload",
        ));
    }
    let raw = &buf[*pos..*pos + len];
    *pos += len;
    let bytes = if is_huff {
        hpack_huffman_decode(raw)?
    } else {
        raw.to_vec()
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Debug, Clone)]
pub struct HpackTable {
    dynamic: VecDeque<(String, String)>,
    current_size: usize,
    max_size: usize,
    protocol_max_size: usize,
}

impl Default for HpackTable {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl HpackTable {
    pub fn new(max_size: usize) -> Self {
        Self {
            dynamic: VecDeque::new(),
            current_size: 0,
            max_size,
            protocol_max_size: max_size,
        }
    }

    pub fn set_protocol_max_size(&mut self, max_size: usize) {
        self.protocol_max_size = max_size;
        if self.max_size > max_size {
            self.resize(max_size);
        }
    }

    pub fn resize(&mut self, new_size: usize) {
        self.max_size = new_size;
        while self.current_size > self.max_size {
            if let Some((k, v)) = self.dynamic.pop_back() {
                self.current_size -= k.len() + v.len() + 32;
            } else {
                break;
            }
        }
    }

    pub fn insert(&mut self, name: String, value: String) {
        let es = name.len() + value.len() + 32;
        if es > self.max_size {
            self.dynamic.clear();
            self.current_size = 0;
            return;
        }
        while !self.dynamic.is_empty() && self.current_size + es > self.max_size {
            if let Some((k, v)) = self.dynamic.pop_back() {
                self.current_size -= k.len() + v.len() + 32;
            }
        }
        self.current_size += es;
        self.dynamic.push_front((name, value));
    }

    pub fn get(&self, index: usize) -> Result<(&str, &str), H2Error> {
        if index == 0 {
            return Err(H2Error::conn(
                H2ErrorCode::CompressionError,
                "HPACK index 0 is invalid",
            ));
        }
        if index <= HPACK_STATIC_TABLE.len() {
            return Ok(HPACK_STATIC_TABLE[index - 1]);
        }
        let dyn_idx = index - 1 - HPACK_STATIC_TABLE.len();
        self.dynamic
            .get(dyn_idx)
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .ok_or_else(|| {
                H2Error::conn(
                    H2ErrorCode::CompressionError,
                    format!("HPACK dynamic index {} out of bounds", index),
                )
            })
    }

    pub fn find(&self, name: &str, value: &str) -> (Option<usize>, Option<usize>) {
        let mut name_match: Option<usize> = None;
        for (i, &(k, v)) in HPACK_STATIC_TABLE.iter().enumerate() {
            if k == name {
                if v == value {
                    return (Some(i + 1), Some(i + 1));
                }
                if name_match.is_none() {
                    name_match = Some(i + 1);
                }
            }
        }
        for (i, (k, v)) in self.dynamic.iter().enumerate() {
            let idx = 1 + HPACK_STATIC_TABLE.len() + i;
            if k == name {
                if v == value {
                    return (Some(idx), Some(idx));
                }
                if name_match.is_none() {
                    name_match = Some(idx);
                }
            }
        }
        (None, name_match)
    }

    pub fn dynamic_len(&self) -> usize {
        self.dynamic.len()
    }

    pub fn current_size(&self) -> usize {
        self.current_size
    }
}

#[derive(Debug, Clone)]
pub struct HpackCodec {
    pub table: HpackTable,
    pub use_huffman: bool,
}

impl Default for HpackCodec {
    fn default() -> Self {
        Self {
            table: HpackTable::default(),
            use_huffman: true,
        }
    }
}

impl HpackCodec {
    pub fn new(max_table_size: usize, use_huffman: bool) -> Self {
        Self {
            table: HpackTable::new(max_table_size),
            use_huffman,
        }
    }

    pub fn encode(&mut self, headers: &[(String, String)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, value) in headers {
            let lname = name.to_ascii_lowercase();
            let (exact, name_idx) = self.table.find(&lname, value);
            if let Some(idx) = exact {
                hpack_encode_int(&mut out, 7, 0x80, idx as u64);
            } else {
                let should_index = !matches!(lname.as_str(), "authorization" | "cookie" | "set-cookie")
                    && (lname.len() + value.len() + 32) <= self.table.max_size;
                if should_index {
                    if let Some(nidx) = name_idx {
                        hpack_encode_int(&mut out, 6, 0x40, nidx as u64);
                    } else {
                        out.push(0x40);
                        hpack_encode_str(&mut out, lname.as_bytes(), self.use_huffman);
                    }
                    hpack_encode_str(&mut out, value.as_bytes(), self.use_huffman);
                    self.table.insert(lname, value.clone());
                } else {
                    if let Some(nidx) = name_idx {
                        hpack_encode_int(&mut out, 4, 0x00, nidx as u64);
                    } else {
                        out.push(0x00);
                        hpack_encode_str(&mut out, lname.as_bytes(), self.use_huffman);
                    }
                    hpack_encode_str(&mut out, value.as_bytes(), self.use_huffman);
                }
            }
        }
        out
    }

    pub fn decode(&mut self, block: &[u8]) -> Result<Vec<(String, String)>, H2Error> {
        let mut pos = 0usize;
        let mut out = Vec::new();
        let mut seen_header_rep = false;

        while pos < block.len() {
            let b = block[pos];
            if (b & 0x80) != 0 {
                // Indexed Header Field (1xxxxxxx)
                seen_header_rep = true;
                let idx = hpack_decode_int(block, &mut pos, 7)? as usize;
                let (k, v) = self.table.get(idx)?;
                out.push((k.to_string(), v.to_string()));
            } else if (b & 0xc0) == 0x40 {
                // Literal Header Field with Incremental Indexing (01xxxxxx)
                seen_header_rep = true;
                let nidx = hpack_decode_int(block, &mut pos, 6)? as usize;
                let name = if nidx == 0 {
                    hpack_decode_str(block, &mut pos)?
                } else {
                    self.table.get(nidx)?.0.to_string()
                };
                let val = hpack_decode_str(block, &mut pos)?;
                self.table.insert(name.clone(), val.clone());
                out.push((name, val));
            } else if (b & 0xe0) == 0x20 {
                // Dynamic Table Size Update (001xxxxx)
                if seen_header_rep {
                    return Err(H2Error::conn(
                        H2ErrorCode::CompressionError,
                        "Dynamic table size update after header representation",
                    ));
                }
                let new_size = hpack_decode_int(block, &mut pos, 5)? as usize;
                if new_size > self.table.protocol_max_size {
                    return Err(H2Error::conn(
                        H2ErrorCode::CompressionError,
                        "Dynamic table size update exceeds protocol max",
                    ));
                }
                self.table.resize(new_size);
            } else {
                // Without Indexing (0000xxxx) or Never Indexed (0001xxxx)
                seen_header_rep = true;
                let nidx = hpack_decode_int(block, &mut pos, 4)? as usize;
                let name = if nidx == 0 {
                    hpack_decode_str(block, &mut pos)?
                } else {
                    self.table.get(nidx)?.0.to_string()
                };
                let val = hpack_decode_str(block, &mut pos)?;
                out.push((name, val));
            }
        }
        Ok(out)
    }
}

pub const H2_PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub const H2_FLAG_ACK: u8 = 0x01;
pub const H2_FLAG_END_STREAM: u8 = 0x01;
pub const H2_FLAG_END_HEADERS: u8 = 0x04;
pub const H2_FLAG_PADDED: u8 = 0x08;
pub const H2_FLAG_PRIORITY: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct H2FrameHeader {
    pub length: u32,
    pub frame_type: u8,
    pub flags: u8,
    pub stream_id: u32,
}

impl H2FrameHeader {
    pub fn encode(&self) -> [u8; 9] {
        let len_b = self.length.to_be_bytes();
        let sid_b = (self.stream_id & 0x7FFF_FFFF).to_be_bytes();
        [
            len_b[1],
            len_b[2],
            len_b[3],
            self.frame_type,
            self.flags,
            sid_b[0],
            sid_b[1],
            sid_b[2],
            sid_b[3],
        ]
    }

    pub fn decode(b: &[u8; 9]) -> Self {
        let length = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        let frame_type = b[3];
        let flags = b[4];
        let stream_id = u32::from_be_bytes([b[5], b[6], b[7], b[8]]) & 0x7FFF_FFFF;
        Self {
            length,
            frame_type,
            flags,
            stream_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H2Frame {
    Data {
        stream_id: u32,
        end_stream: bool,
        data: Vec<u8>,
        pad_len: usize,
    },
    Headers {
        stream_id: u32,
        end_stream: bool,
        end_headers: bool,
        priority: Option<(bool, u32, u8)>,
        header_block: Vec<u8>,
        pad_len: usize,
    },
    Priority {
        stream_id: u32,
        exclusive: bool,
        stream_dep: u32,
        weight: u8,
    },
    RstStream {
        stream_id: u32,
        error_code: u32,
    },
    Settings {
        ack: bool,
        params: Vec<(u16, u32)>,
    },
    PushPromise {
        stream_id: u32,
        end_headers: bool,
        promised_stream_id: u32,
        header_block: Vec<u8>,
        pad_len: usize,
    },
    Ping {
        ack: bool,
        opaque: [u8; 8],
    },
    Goaway {
        last_stream_id: u32,
        error_code: u32,
        debug_data: Vec<u8>,
    },
    WindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    Continuation {
        stream_id: u32,
        end_headers: bool,
        header_block: Vec<u8>,
    },
    Unknown {
        header: H2FrameHeader,
        payload: Vec<u8>,
    },
}

impl H2Frame {
    pub fn encode(&self) -> Vec<u8> {
        let (frame_type, flags, stream_id, payload) = match self {
            Self::Data {
                stream_id,
                end_stream,
                data,
                pad_len,
            } => {
                let mut f = if *end_stream { H2_FLAG_END_STREAM } else { 0 };
                let mut p = Vec::new();
                if *pad_len > 0 {
                    f |= H2_FLAG_PADDED;
                    p.push(*pad_len as u8);
                    p.extend_from_slice(data);
                    p.resize(p.len() + *pad_len, 0);
                } else {
                    p.extend_from_slice(data);
                }
                (0x0u8, f, *stream_id, p)
            }
            Self::Headers {
                stream_id,
                end_stream,
                end_headers,
                priority,
                header_block,
                pad_len,
            } => {
                let mut f = 0u8;
                if *end_stream {
                    f |= H2_FLAG_END_STREAM;
                }
                if *end_headers {
                    f |= H2_FLAG_END_HEADERS;
                }
                let mut p = Vec::new();
                if *pad_len > 0 {
                    f |= H2_FLAG_PADDED;
                    p.push(*pad_len as u8);
                }
                if let Some((excl, dep, weight)) = priority {
                    f |= H2_FLAG_PRIORITY;
                    let mut raw_dep = dep & 0x7FFF_FFFF;
                    if *excl {
                        raw_dep |= 0x8000_0000;
                    }
                    p.extend_from_slice(&raw_dep.to_be_bytes());
                    p.push(*weight);
                }
                p.extend_from_slice(header_block);
                if *pad_len > 0 {
                    p.resize(p.len() + *pad_len, 0);
                }
                (0x1u8, f, *stream_id, p)
            }
            Self::Priority {
                stream_id,
                exclusive,
                stream_dep,
                weight,
            } => {
                let mut raw = stream_dep & 0x7FFF_FFFF;
                if *exclusive {
                    raw |= 0x8000_0000;
                }
                let mut p = raw.to_be_bytes().to_vec();
                p.push(*weight);
                (0x2u8, 0u8, *stream_id, p)
            }
            Self::RstStream {
                stream_id,
                error_code,
            } => (0x3u8, 0u8, *stream_id, error_code.to_be_bytes().to_vec()),
            Self::Settings { ack, params } => {
                let mut p = Vec::with_capacity(params.len() * 6);
                if !*ack {
                    for (id, val) in params {
                        p.extend_from_slice(&id.to_be_bytes());
                        p.extend_from_slice(&val.to_be_bytes());
                    }
                }
                (0x4u8, if *ack { H2_FLAG_ACK } else { 0 }, 0u32, p)
            }
            Self::PushPromise {
                stream_id,
                end_headers,
                promised_stream_id,
                header_block,
                pad_len,
            } => {
                let mut f = if *end_headers { H2_FLAG_END_HEADERS } else { 0 };
                let mut p = Vec::new();
                if *pad_len > 0 {
                    f |= H2_FLAG_PADDED;
                    p.push(*pad_len as u8);
                }
                p.extend_from_slice(&(promised_stream_id & 0x7FFF_FFFF).to_be_bytes());
                p.extend_from_slice(header_block);
                if *pad_len > 0 {
                    p.resize(p.len() + *pad_len, 0);
                }
                (0x5u8, f, *stream_id, p)
            }
            Self::Ping { ack, opaque } => {
                (0x6u8, if *ack { H2_FLAG_ACK } else { 0 }, 0u32, opaque.to_vec())
            }
            Self::Goaway {
                last_stream_id,
                error_code,
                debug_data,
            } => {
                let mut p = Vec::with_capacity(8 + debug_data.len());
                p.extend_from_slice(&(last_stream_id & 0x7FFF_FFFF).to_be_bytes());
                p.extend_from_slice(&error_code.to_be_bytes());
                p.extend_from_slice(debug_data);
                (0x7u8, 0u8, 0u32, p)
            }
            Self::WindowUpdate {
                stream_id,
                increment,
            } => (
                0x8u8,
                0u8,
                *stream_id,
                (increment & 0x7FFF_FFFF).to_be_bytes().to_vec(),
            ),
            Self::Continuation {
                stream_id,
                end_headers,
                header_block,
            } => (
                0x9u8,
                if *end_headers { H2_FLAG_END_HEADERS } else { 0 },
                *stream_id,
                header_block.clone(),
            ),
            Self::Unknown { header, payload } => {
                (header.frame_type, header.flags, header.stream_id, payload.clone())
            }
        };

        let hdr = H2FrameHeader {
            length: payload.len() as u32,
            frame_type,
            flags,
            stream_id,
        };
        let mut out = Vec::with_capacity(9 + payload.len());
        out.extend_from_slice(&hdr.encode());
        out.extend_from_slice(&payload);
        out
    }

    pub fn decode(hdr: H2FrameHeader, payload: &[u8], max_frame_size: u32) -> Result<Self, H2Error> {
        if hdr.length > max_frame_size || payload.len() != hdr.length as usize {
            return Err(H2Error::conn(
                H2ErrorCode::FrameSizeError,
                "Frame exceeds max_frame_size or length mismatch",
            ));
        }

        match hdr.frame_type {
            0x0 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "DATA frame on stream 0",
                    ));
                }
                let end_stream = (hdr.flags & H2_FLAG_END_STREAM) != 0;
                let (pad_len, data) = if (hdr.flags & H2_FLAG_PADDED) != 0 {
                    if payload.is_empty() {
                        return Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "Padded DATA frame with empty payload",
                        ));
                    }
                    let pl = payload[0] as usize;
                    if 1 + pl > payload.len() {
                        return Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "DATA padding exceeds payload length",
                        ));
                    }
                    (pl, payload[1..payload.len() - pl].to_vec())
                } else {
                    (0, payload.to_vec())
                };
                Ok(Self::Data {
                    stream_id: hdr.stream_id,
                    end_stream,
                    data,
                    pad_len,
                })
            }
            0x1 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "HEADERS frame on stream 0",
                    ));
                }
                let end_stream = (hdr.flags & H2_FLAG_END_STREAM) != 0;
                let end_headers = (hdr.flags & H2_FLAG_END_HEADERS) != 0;
                let mut idx = 0usize;
                let mut pad_len = 0usize;
                if (hdr.flags & H2_FLAG_PADDED) != 0 {
                    if payload.is_empty() {
                        return Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "Padded HEADERS frame empty",
                        ));
                    }
                    pad_len = payload[0] as usize;
                    idx += 1;
                }
                let priority = if (hdr.flags & H2_FLAG_PRIORITY) != 0 {
                    if idx + 5 > payload.len() {
                        return Err(H2Error::conn(
                            H2ErrorCode::FrameSizeError,
                            "Truncated HEADERS priority block",
                        ));
                    }
                    let raw_dep = u32::from_be_bytes([
                        payload[idx],
                        payload[idx + 1],
                        payload[idx + 2],
                        payload[idx + 3],
                    ]);
                    let excl = (raw_dep & 0x8000_0000) != 0;
                    let dep = raw_dep & 0x7FFF_FFFF;
                    if dep == hdr.stream_id {
                        return Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "HEADERS frame depends on itself",
                        ));
                    }
                    let weight = payload[idx + 4];
                    idx += 5;
                    Some((excl, dep, weight))
                } else {
                    None
                };
                if idx + pad_len > payload.len() {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "HEADERS padding exceeds payload length",
                    ));
                }
                let header_block = payload[idx..payload.len() - pad_len].to_vec();
                Ok(Self::Headers {
                    stream_id: hdr.stream_id,
                    end_stream,
                    end_headers,
                    priority,
                    header_block,
                    pad_len,
                })
            }
            0x2 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "PRIORITY frame on stream 0",
                    ));
                }
                if payload.len() != 5 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "PRIORITY frame must be 5 bytes",
                    ));
                }
                let raw = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let exclusive = (raw & 0x8000_0000) != 0;
                let stream_dep = raw & 0x7FFF_FFFF;
                if stream_dep == hdr.stream_id {
                    return Err(H2Error::stream(
                        H2ErrorCode::ProtocolError,
                        hdr.stream_id,
                        "PRIORITY self-dependency",
                    ));
                }
                Ok(Self::Priority {
                    stream_id: hdr.stream_id,
                    exclusive,
                    stream_dep,
                    weight: payload[4],
                })
            }
            0x3 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "RST_STREAM on stream 0",
                    ));
                }
                if payload.len() != 4 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "RST_STREAM must be 4 bytes",
                    ));
                }
                Ok(Self::RstStream {
                    stream_id: hdr.stream_id,
                    error_code: u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]),
                })
            }
            0x4 => {
                if hdr.stream_id != 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "SETTINGS frame with non-zero stream_id",
                    ));
                }
                let ack = (hdr.flags & H2_FLAG_ACK) != 0;
                if ack {
                    if !payload.is_empty() {
                        return Err(H2Error::conn(
                            H2ErrorCode::FrameSizeError,
                            "SETTINGS ACK must have empty payload",
                        ));
                    }
                    return Ok(Self::Settings {
                        ack: true,
                        params: Vec::new(),
                    });
                }
                if payload.len() % 6 != 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "SETTINGS payload must be multiple of 6 bytes",
                    ));
                }
                let mut params = Vec::with_capacity(payload.len() / 6);
                for chunk in payload.chunks_exact(6) {
                    let id = u16::from_be_bytes([chunk[0], chunk[1]]);
                    let val = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
                    match id {
                        0x2 if val > 1 => {
                            return Err(H2Error::conn(
                                H2ErrorCode::ProtocolError,
                                "SETTINGS_ENABLE_PUSH must be 0 or 1",
                            ));
                        }
                        0x4 if val > 0x7FFF_FFFF => {
                            return Err(H2Error::conn(
                                H2ErrorCode::FlowControlError,
                                "SETTINGS_INITIAL_WINDOW_SIZE exceeds 2^31-1",
                            ));
                        }
                        0x5 if !(16384..=16_777_215).contains(&val) => {
                            return Err(H2Error::conn(
                                H2ErrorCode::ProtocolError,
                                "SETTINGS_MAX_FRAME_SIZE out of range",
                            ));
                        }
                        _ => {}
                    }
                    params.push((id, val));
                }
                Ok(Self::Settings { ack: false, params })
            }
            0x5 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "PUSH_PROMISE on stream 0",
                    ));
                }
                let end_headers = (hdr.flags & H2_FLAG_END_HEADERS) != 0;
                let mut idx = 0usize;
                let mut pad_len = 0usize;
                if (hdr.flags & H2_FLAG_PADDED) != 0 {
                    if payload.is_empty() {
                        return Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "Padded PUSH_PROMISE empty",
                        ));
                    }
                    pad_len = payload[0] as usize;
                    idx += 1;
                }
                if idx + 4 + pad_len > payload.len() {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "Malformed PUSH_PROMISE",
                    ));
                }
                let promised_stream_id = u32::from_be_bytes([
                    payload[idx],
                    payload[idx + 1],
                    payload[idx + 2],
                    payload[idx + 3],
                ]) & 0x7FFF_FFFF;
                if promised_stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "PUSH_PROMISE promised stream ID cannot be 0",
                    ));
                }
                idx += 4;
                let header_block = payload[idx..payload.len() - pad_len].to_vec();
                Ok(Self::PushPromise {
                    stream_id: hdr.stream_id,
                    end_headers,
                    promised_stream_id,
                    header_block,
                    pad_len,
                })
            }
            0x6 => {
                if hdr.stream_id != 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "PING on non-zero stream",
                    ));
                }
                if payload.len() != 8 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "PING payload must be 8 bytes",
                    ));
                }
                let mut opaque = [0u8; 8];
                opaque.copy_from_slice(payload);
                Ok(Self::Ping {
                    ack: (hdr.flags & H2_FLAG_ACK) != 0,
                    opaque,
                })
            }
            0x7 => {
                if hdr.stream_id != 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "GOAWAY on non-zero stream",
                    ));
                }
                if payload.len() < 8 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "GOAWAY payload < 8 bytes",
                    ));
                }
                let last_stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
                        & 0x7FFF_FFFF;
                let error_code =
                    u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let debug_data = payload[8..].to_vec();
                Ok(Self::Goaway {
                    last_stream_id,
                    error_code,
                    debug_data,
                })
            }
            0x8 => {
                if payload.len() != 4 {
                    return Err(H2Error::conn(
                        H2ErrorCode::FrameSizeError,
                        "WINDOW_UPDATE must be 4 bytes",
                    ));
                }
                let increment =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
                        & 0x7FFF_FFFF;
                if increment == 0 {
                    return if hdr.stream_id == 0 {
                        Err(H2Error::conn(
                            H2ErrorCode::ProtocolError,
                            "WINDOW_UPDATE with 0 increment on stream 0",
                        ))
                    } else {
                        Err(H2Error::stream(
                            H2ErrorCode::ProtocolError,
                            hdr.stream_id,
                            "WINDOW_UPDATE with 0 increment",
                        ))
                    };
                }
                Ok(Self::WindowUpdate {
                    stream_id: hdr.stream_id,
                    increment,
                })
            }
            0x9 => {
                if hdr.stream_id == 0 {
                    return Err(H2Error::conn(
                        H2ErrorCode::ProtocolError,
                        "CONTINUATION on stream 0",
                    ));
                }
                Ok(Self::Continuation {
                    stream_id: hdr.stream_id,
                    end_headers: (hdr.flags & H2_FLAG_END_HEADERS) != 0,
                    header_block: payload.to_vec(),
                })
            }
            _ => Ok(Self::Unknown {
                header: hdr,
                payload: payload.to_vec(),
            }),
        }
    }
}

#[derive(Default)]
pub struct H2ContinuationAssembler {
    pending: Option<(u32, bool, Vec<u8>)>,
}

impl H2ContinuationAssembler {
    pub fn process_frame(
        &mut self,
        frame: H2Frame,
    ) -> Result<Option<(u32, bool, Vec<u8>)>, H2Error> {
        if let Some((expected_sid, end_stream, mut buf)) = self.pending.take() {
            match frame {
                H2Frame::Continuation {
                    stream_id,
                    end_headers,
                    header_block,
                } if stream_id == expected_sid => {
                    buf.extend_from_slice(&header_block);
                    if end_headers {
                        Ok(Some((expected_sid, end_stream, buf)))
                    } else {
                        self.pending = Some((expected_sid, end_stream, buf));
                        Ok(None)
                    }
                }
                _ => Err(H2Error::conn(
                    H2ErrorCode::ProtocolError,
                    "Expected CONTINUATION frame for active header block",
                )),
            }
        } else {
            match frame {
                H2Frame::Headers {
                    stream_id,
                    end_stream,
                    end_headers,
                    header_block,
                    ..
                } => {
                    if end_headers {
                        Ok(Some((stream_id, end_stream, header_block)))
                    } else {
                        self.pending = Some((stream_id, end_stream, header_block));
                        Ok(None)
                    }
                }
                H2Frame::Continuation { .. } => Err(H2Error::conn(
                    H2ErrorCode::ProtocolError,
                    "Unexpected CONTINUATION frame without active HEADERS",
                )),
                _ => Ok(None),
            }
        }
    }

    pub fn is_awaiting_continuation(&self) -> bool {
        self.pending.is_some()
    }
}

fn read_exact_resilient<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    running: &AtomicBool,
    allow_idle_before_first_byte: bool,
) -> io::Result<bool> {
    let mut off = 0usize;
    while off < buf.len() {
        match reader.read(&mut buf[off..]) {
            Ok(0) => return Err(io::Error::new(ErrorKind::UnexpectedEof, "EOF")),
            Ok(n) => off += n,
            Err(ref e)
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                if off == 0 && allow_idle_before_first_byte {
                    return Ok(false);
                }
                if !running.load(Ordering::Relaxed) {
                    return Err(io::Error::new(ErrorKind::Interrupted, "Stopped"));
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn read_h2_frame_resilient<R: Read>(
    reader: &mut R,
    max_frame_size: u32,
    running: &AtomicBool,
    allow_idle: bool,
) -> Result<Option<H2Frame>, H2Error> {
    let mut hdr_buf = [0u8; 9];
    match read_exact_resilient(reader, &mut hdr_buf, running, allow_idle) {
        Ok(false) => return Ok(None),
        Ok(true) => {}
        Err(e) => {
            return Err(H2Error::conn(
                H2ErrorCode::ConnectError,
                format!("H2 read error: {}", e),
            ));
        }
    }
    let hdr = H2FrameHeader::decode(&hdr_buf);
    if hdr.length > max_frame_size {
        return Err(H2Error::conn(
            H2ErrorCode::FrameSizeError,
            format!("Frame size {} exceeds {}", hdr.length, max_frame_size),
        ));
    }
    let mut payload = vec![0u8; hdr.length as usize];
    if !payload.is_empty() {
        read_exact_resilient(reader, &mut payload, running, false).map_err(|e| {
            H2Error::conn(
                H2ErrorCode::ConnectError,
                format!("H2 payload read error: {}", e),
            )
        })?;
    }
    H2Frame::decode(hdr, &payload, max_frame_size).map(Some)
}

#[derive(Debug, Clone)]
pub struct H2Response {
    pub stream_id: u32,
    pub status: u16,
    pub headers: IndexMap<String, Vec<String>>,
    pub body: Vec<u8>,
}

enum H2MuxMsg {
    SendRequest {
        stream_id: u32,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        done_tx: mpsc::Sender<Result<H2Response, String>>,
    },
    Ping {
        opaque: [u8; 8],
        done_tx: mpsc::Sender<Result<(), String>>,
    },
    WindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    RstStream {
        stream_id: u32,
        error_code: u32,
    },
    Goaway {
        error_code: u32,
    },
    Close,
}

struct H2ClientStreamState {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    recv_consumed: u32,
    send_window: i64,
    pending_send: VecDeque<u8>,
    end_stream_to_send: bool,
    done_tx: Option<mpsc::Sender<Result<H2Response, String>>>,
}

pub struct H2ClientConn {
    pub id: u64,
    pub authority: String,
    pub scheme: String,
    pub negotiated_alpn: Option<String>,
    next_stream_id: AtomicU64,
    cmd_tx: mpsc::Sender<H2MuxMsg>,
    running: Arc<AtomicBool>,
    driver_handle: Mutex<Option<JoinHandle<()>>>,
    requests_sent: AtomicU64,
    responses_recv: AtomicU64,
    pings_acked: AtomicU64,
}

impl H2ClientConn {
    pub fn connect(url_or_addr: &str, insecure: bool, ca_pem: Option<&str>) -> Result<Self, String> {
        let parsed = parse_url(url_or_addr)?;
        let use_tls = parsed.scheme == "https" || parsed.scheme == "h2";
        let addr_str = format_socket_addr(&parsed.host, parsed.port);
        let tcp = TcpStream::connect(&addr_str)
            .map_err(|e| format!("H2 TCP connect failed '{}': {}", addr_str, e))?;
        let _ = tcp.set_nodelay(true);
        let _ = tcp.set_read_timeout(Some(Duration::from_millis(2)));

        let (mut io_stream, alpn) = if use_tls {
            let cfg = build_tls_client_config(ca_pem, insecure, vec![b"h2".to_vec()])?;
            let sni = ServerName::try_from(parsed.host.clone())
                .map_err(|_| format!("Invalid SNI host '{}'", parsed.host))?;
            let mut conn = rustls::ClientConnection::new(cfg, sni)
                .map_err(|e| format!("H2 TLS init failed: {}", e))?;
            let mut sock = tcp;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
            conn.complete_io(&mut sock)
                .map_err(|e| format!("H2 TLS handshake failed: {}", e))?;
            let alpn = conn
                .alpn_protocol()
                .map(|p| String::from_utf8_lossy(p).into_owned());
            if alpn.as_deref() != Some("h2") {
                return Err(format!("TLS ALPN did not negotiate 'h2', got {:?}", alpn));
            }
            let _ = sock.set_read_timeout(Some(Duration::from_millis(2)));
            (H2ClientIo::Tls(rustls::StreamOwned::new(conn, sock)), alpn)
        } else {
            (H2ClientIo::Plain(tcp), None)
        };

        // Send HTTP/2 connection preface + initial SETTINGS
        io_stream
            .write_all(H2_PREFACE)
            .and_then(|_| io_stream.write_all(&H2Frame::Settings {
                ack: false,
                params: vec![(0x4, 65535), (0x5, 16384)],
            }.encode()))
            .and_then(|_| io_stream.flush())
            .map_err(|e| format!("Failed sending H2 preface: {}", e))?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<H2MuxMsg>();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let id = next_handle_id();
        let pings_acked = Arc::new(AtomicU64::new(0));
        let pings_clone = pings_acked.clone();

        let jh = thread::spawn(move || {
            run_h2_client_mux(io_stream, cmd_rx, running_clone, pings_clone);
        });

        Ok(Self {
            id,
            authority: parsed.authority,
            scheme: if use_tls { "https".to_string() } else { "http".to_string() },
            negotiated_alpn: alpn,
            next_stream_id: AtomicU64::new(1),
            cmd_tx,
            running,
            driver_handle: Mutex::new(Some(jh)),
            requests_sent: AtomicU64::new(0),
            responses_recv: AtomicU64::new(0),
            pings_acked: AtomicU64::new(0), // synced via mux or ping_wait
        })
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        headers: &IndexMap<String, Vec<String>>,
        body: &[u8],
    ) -> Result<H2Response, String> {
        let rx = self.submit_request(method, path, headers, body)?;
        let resp = rx
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "HTTP/2 request timed out".to_string())??;
        self.responses_recv.fetch_add(1, Ordering::Relaxed);
        Ok(resp)
    }

    pub fn submit_request(
        &self,
        method: &str,
        path: &str,
        headers: &IndexMap<String, Vec<String>>,
        body: &[u8],
    ) -> Result<mpsc::Receiver<Result<H2Response, String>>, String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("HTTP/2 connection is closed".to_string());
        }
        let sid = self.next_stream_id.fetch_add(2, Ordering::SeqCst) as u32;
        let mut hdr_list = vec![
            (":method".to_string(), method.to_ascii_uppercase()),
            (":path".to_string(), if path.is_empty() { "/".to_string() } else { path.to_string() }),
            (":scheme".to_string(), self.scheme.clone()),
            (":authority".to_string(), self.authority.clone()),
        ];
        for (k, vals) in headers {
            let lk = k.to_ascii_lowercase();
            if lk == "connection" || lk == "host" || lk == "transfer-encoding" {
                continue;
            }
            for v in vals {
                hdr_list.push((lk.clone(), v.clone()));
            }
        }
        let (done_tx, done_rx) = mpsc::channel();
        self.requests_sent.fetch_add(1, Ordering::Relaxed);
        self.cmd_tx
            .send(H2MuxMsg::SendRequest {
                stream_id: sid,
                headers: hdr_list,
                body: body.to_vec(),
                done_tx,
            })
            .map_err(|_| "HTTP/2 driver stopped".to_string())?;
        Ok(done_rx)
    }

    pub fn multiplex(
        &self,
        reqs: &[(String, String, IndexMap<String, Vec<String>>, Vec<u8>)],
    ) -> Result<Vec<H2Response>, String> {
        let mut rxs = Vec::with_capacity(reqs.len());
        for (m, p, h, b) in reqs {
            rxs.push(self.submit_request(m, p, h, b)?);
        }
        let mut out = Vec::with_capacity(reqs.len());
        for rx in rxs {
            let resp = rx
                .recv_timeout(Duration::from_secs(15))
                .map_err(|_| "HTTP/2 multiplex stream timed out".to_string())??;
            self.responses_recv.fetch_add(1, Ordering::Relaxed);
            out.push(resp);
        }
        Ok(out)
    }

    pub fn ping(&self, opaque: [u8; 8]) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(H2MuxMsg::Ping { opaque, done_tx: tx })
            .map_err(|_| "HTTP/2 driver stopped".to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "HTTP/2 PING ACK timed out".to_string())??;
        self.pings_acked.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn window_update(&self, stream_id: u32, increment: u32) -> Result<(), String> {
        self.cmd_tx
            .send(H2MuxMsg::WindowUpdate {
                stream_id,
                increment,
            })
            .map_err(|_| "HTTP/2 driver stopped".to_string())
    }

    pub fn rst_stream(&self, stream_id: u32, error_code: u32) -> Result<(), String> {
        self.cmd_tx
            .send(H2MuxMsg::RstStream {
                stream_id,
                error_code,
            })
            .map_err(|_| "HTTP/2 driver stopped".to_string())
    }

    pub fn goaway(&self, error_code: u32) -> Result<(), String> {
        self.cmd_tx
            .send(H2MuxMsg::Goaway { error_code })
            .map_err(|_| "HTTP/2 driver stopped".to_string())
    }

    pub fn close(&self) {
        if self.running.swap(false, Ordering::SeqCst) {
            let _ = self.cmd_tx.send(H2MuxMsg::Close);
        }
        if let Some(jh) = self.driver_handle.lock().unwrap().take() {
            let _ = jh.join();
        }
    }

    pub fn stats_map(&self) -> Value {
        let mut m = IndexMap::new();
        m.insert(
            "requests_sent".to_string(),
            Value::Int(self.requests_sent.load(Ordering::Relaxed) as i64),
        );
        m.insert(
            "responses_recv".to_string(),
            Value::Int(self.responses_recv.load(Ordering::Relaxed) as i64),
        );
        m.insert(
            "pings_acked".to_string(),
            Value::Int(self.pings_acked.load(Ordering::Relaxed) as i64),
        );
        m.insert(
            "open".to_string(),
            Value::Bool(self.running.load(Ordering::Relaxed)),
        );
        if let Some(ref alpn) = self.negotiated_alpn {
            m.insert("alpn".to_string(), Value::String(alpn.clone()));
        }
        Value::Map(Arc::new(RwLock::new(m)))
    }
}

impl Drop for H2ClientConn {
    fn drop(&mut self) {
        self.close();
    }
}

enum H2ClientIo {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl Read for H2ClientIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for H2ClientIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

fn flush_h2_stream_data<W: Write>(
    io: &mut W,
    streams: &mut HashMap<u32, H2ClientStreamState>,
    conn_send_window: &mut i64,
    max_frame_size: usize,
) -> io::Result<()> {
    let sids: Vec<u32> = streams.keys().copied().collect();
    for sid in sids {
        if *conn_send_window <= 0 {
            break;
        }
        let Some(st) = streams.get_mut(&sid) else {
            continue;
        };
        while !st.pending_send.is_empty() && st.send_window > 0 && *conn_send_window > 0 {
            let chunk_cap = (st.send_window as usize)
                .min(*conn_send_window as usize)
                .min(max_frame_size)
                .min(st.pending_send.len());
            if chunk_cap == 0 {
                break;
            }
            let chunk: Vec<u8> = st.pending_send.drain(..chunk_cap).collect();
            st.send_window -= chunk.len() as i64;
            *conn_send_window -= chunk.len() as i64;
            let end_stream = st.pending_send.is_empty() && st.end_stream_to_send;
            if end_stream {
                st.end_stream_to_send = false;
            }
            let frame = H2Frame::Data {
                stream_id: sid,
                end_stream,
                data: chunk,
                pad_len: 0,
            };
            io.write_all(&frame.encode())?;
        }
        if st.pending_send.is_empty() && st.end_stream_to_send {
            st.end_stream_to_send = false;
            let frame = H2Frame::Data {
                stream_id: sid,
                end_stream: true,
                data: Vec::new(),
                pad_len: 0,
            };
            io.write_all(&frame.encode())?;
        }
    }
    io.flush()
}

fn run_h2_client_mux(
    mut io: H2ClientIo,
    cmd_rx: mpsc::Receiver<H2MuxMsg>,
    running: Arc<AtomicBool>,
    pings_acked: Arc<AtomicU64>,
) {
    let mut hpack_enc = HpackCodec::default();
    let mut hpack_dec = HpackCodec::default();
    let mut assembler = H2ContinuationAssembler::default();
    let mut streams: HashMap<u32, H2ClientStreamState> = HashMap::new();
    let mut pending_pings: HashMap<[u8; 8], mpsc::Sender<Result<(), String>>> = HashMap::new();
    let mut peer_max_frame_size: u32 = 16384;
    let mut peer_initial_window: i64 = 65535;
    let mut conn_send_window: i64 = 65535;
    let mut conn_recv_consumed: u32 = 0;

    while running.load(Ordering::Acquire) {
        // 1. Drain outbound commands
        let mut wrote = false;
        while let Ok(msg) = cmd_rx.try_recv() {
            match msg {
                H2MuxMsg::SendRequest {
                    stream_id,
                    headers,
                    body,
                    done_tx,
                } => {
                    let block = hpack_enc.encode(&headers);
                    let has_body = !body.is_empty();
                    // Split HEADERS if larger than peer_max_frame_size
                    let max_fs = peer_max_frame_size as usize;
                    if block.len() <= max_fs {
                        let f = H2Frame::Headers {
                            stream_id,
                            end_stream: !has_body,
                            end_headers: true,
                            priority: None,
                            header_block: block,
                            pad_len: 0,
                        };
                        if io.write_all(&f.encode()).is_err() {
                            let _ = done_tx.send(Err("H2 write failed".to_string()));
                            running.store(false, Ordering::SeqCst);
                            break;
                        }
                    } else {
                        let mut offset = 0usize;
                        let first = &block[..max_fs];
                        offset += max_fs;
                        let f = H2Frame::Headers {
                            stream_id,
                            end_stream: !has_body,
                            end_headers: false,
                            priority: None,
                            header_block: first.to_vec(),
                            pad_len: 0,
                        };
                        let _ = io.write_all(&f.encode());
                        while offset < block.len() {
                            let end = (offset + max_fs).min(block.len());
                            let cf = H2Frame::Continuation {
                                stream_id,
                                end_headers: end == block.len(),
                                header_block: block[offset..end].to_vec(),
                            };
                            let _ = io.write_all(&cf.encode());
                            offset = end;
                        }
                    }

                    streams.insert(
                        stream_id,
                        H2ClientStreamState {
                            headers: Vec::new(),
                            body: Vec::new(),
                            recv_consumed: 0,
                            send_window: peer_initial_window,
                            pending_send: body.into(),
                            end_stream_to_send: has_body,
                            done_tx: Some(done_tx),
                        },
                    );
                    wrote = true;
                }
                H2MuxMsg::Ping { opaque, done_tx } => {
                    pending_pings.insert(opaque, done_tx);
                    let _ = io.write_all(&H2Frame::Ping { ack: false, opaque }.encode());
                    wrote = true;
                }
                H2MuxMsg::WindowUpdate {
                    stream_id,
                    increment,
                } => {
                    let _ = io.write_all(
                        &H2Frame::WindowUpdate {
                            stream_id,
                            increment,
                        }
                        .encode(),
                    );
                    wrote = true;
                }
                H2MuxMsg::RstStream {
                    stream_id,
                    error_code,
                } => {
                    let _ = io.write_all(
                        &H2Frame::RstStream {
                            stream_id,
                            error_code,
                        }
                        .encode(),
                    );
                    if let Some(mut st) = streams.remove(&stream_id) {
                        if let Some(tx) = st.done_tx.take() {
                            let _ = tx.send(Err("Stream reset by client".to_string()));
                        }
                    }
                    wrote = true;
                }
                H2MuxMsg::Goaway { error_code } => {
                    let _ = io.write_all(
                        &H2Frame::Goaway {
                            last_stream_id: 0,
                            error_code,
                            debug_data: Vec::new(),
                        }
                        .encode(),
                    );
                    let _ = io.flush();
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                H2MuxMsg::Close => {
                    let _ = io.write_all(
                        &H2Frame::Goaway {
                            last_stream_id: 0,
                            error_code: 0,
                            debug_data: Vec::new(),
                        }
                        .encode(),
                    );
                    let _ = io.flush();
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
        if !running.load(Ordering::Acquire) {
            break;
        }
        if wrote || !streams.is_empty() {
            if flush_h2_stream_data(&mut io, &mut streams, &mut conn_send_window, peer_max_frame_size as usize).is_err() {
                break;
            }
        }

        // 2. Read next inbound frame (non-blocking / 2ms timeout)
        match read_h2_frame_resilient(&mut io, 16384, &running, true) {
            Ok(None) => continue,
            Ok(Some(frame)) => {
                if assembler.is_awaiting_continuation() || matches!(frame, H2Frame::Headers { .. } | H2Frame::Continuation { .. }) {
                    match assembler.process_frame(frame.clone()) {
                        Ok(Some((sid, end_stream, block))) => {
                            match hpack_dec.decode(&block) {
                                Ok(decoded) => {
                                    if let Some(st) = streams.get_mut(&sid) {
                                        st.headers.extend(decoded);
                                        if end_stream {
                                            if let Some(mut finished) = streams.remove(&sid) {
                                                finish_client_h2_stream(sid, &mut finished);
                                            }
                                        }
                                    }
                                    continue;
                                }
                                Err(e) => {
                                    let _ = io.write_all(
                                        &H2Frame::Goaway {
                                            last_stream_id: 0,
                                            error_code: e.code.as_u32(),
                                            debug_data: e.message.clone().into_bytes(),
                                        }
                                        .encode(),
                                    );
                                    let _ = io.flush();
                                    let msg = e.to_string();
                                    for (_, mut st) in streams.drain() {
                                        if let Some(tx) = st.done_tx.take() {
                                            let _ = tx.send(Err(msg.clone()));
                                        }
                                    }
                                    for (_, tx) in pending_pings.drain() {
                                        let _ = tx.send(Err(msg.clone()));
                                    }
                                    running.store(false, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            let _ = io.write_all(
                                &H2Frame::Goaway {
                                    last_stream_id: 0,
                                    error_code: e.code.as_u32(),
                                    debug_data: e.message.clone().into_bytes(),
                                }
                                .encode(),
                            );
                            let _ = io.flush();
                            let msg = e.to_string();
                            for (_, mut st) in streams.drain() {
                                if let Some(tx) = st.done_tx.take() {
                                    let _ = tx.send(Err(msg.clone()));
                                }
                            }
                            for (_, tx) in pending_pings.drain() {
                                let _ = tx.send(Err(msg.clone()));
                            }
                            running.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }

                match frame {
                    H2Frame::Data {
                        stream_id,
                        end_stream,
                        data,
                        ..
                    } => {
                        let dlen = data.len() as u32;
                        if let Some(st) = streams.get_mut(&stream_id) {
                            st.body.extend_from_slice(&data);
                            st.recv_consumed += dlen;
                            conn_recv_consumed += dlen;
                            if conn_recv_consumed >= 32768 {
                                let _ = io.write_all(
                                    &H2Frame::WindowUpdate {
                                        stream_id: 0,
                                        increment: conn_recv_consumed,
                                    }
                                    .encode(),
                                );
                                conn_recv_consumed = 0;
                            }
                            if !end_stream && st.recv_consumed >= 32768 {
                                let _ = io.write_all(
                                    &H2Frame::WindowUpdate {
                                        stream_id,
                                        increment: st.recv_consumed,
                                    }
                                    .encode(),
                                );
                                st.recv_consumed = 0;
                            }
                            let _ = io.flush();
                            if end_stream {
                                if let Some(mut finished) = streams.remove(&stream_id) {
                                    finish_client_h2_stream(stream_id, &mut finished);
                                }
                            }
                        }
                    }
                    H2Frame::Settings { ack, params } => {
                        if !ack {
                            for (id, val) in params {
                                match id {
                                    0x1 => hpack_enc.table.set_protocol_max_size(val as usize),
                                    0x4 => {
                                        let delta = val as i64 - peer_initial_window;
                                        peer_initial_window = val as i64;
                                        for st in streams.values_mut() {
                                            st.send_window += delta;
                                        }
                                    }
                                    0x5 => peer_max_frame_size = val.max(16384),
                                    _ => {}
                                }
                            }
                            let _ = io.write_all(
                                &H2Frame::Settings {
                                    ack: true,
                                    params: Vec::new(),
                                }
                                .encode(),
                            );
                            let _ = io.flush();
                        }
                    }
                    H2Frame::WindowUpdate {
                        stream_id,
                        increment,
                    } => {
                        if stream_id == 0 {
                            conn_send_window += increment as i64;
                        } else if let Some(st) = streams.get_mut(&stream_id) {
                            st.send_window += increment as i64;
                        }
                        let _ = flush_h2_stream_data(
                            &mut io,
                            &mut streams,
                            &mut conn_send_window,
                            peer_max_frame_size as usize,
                        );
                    }
                    H2Frame::Ping { ack, opaque } => {
                        if ack {
                            if let Some(tx) = pending_pings.remove(&opaque) {
                                pings_acked.fetch_add(1, Ordering::Relaxed);
                                let _ = tx.send(Ok(()));
                            }
                        } else {
                            let _ = io.write_all(&H2Frame::Ping { ack: true, opaque }.encode());
                            let _ = io.flush();
                        }
                    }
                    H2Frame::RstStream {
                        stream_id,
                        error_code,
                    } => {
                        if let Some(mut st) = streams.remove(&stream_id) {
                            if let Some(tx) = st.done_tx.take() {
                                let _ = tx.send(Err(format!("RST_STREAM error {}", error_code)));
                            }
                        }
                    }
                    H2Frame::Goaway { error_code, .. } => {
                        for (_, mut st) in streams.drain() {
                            if let Some(tx) = st.done_tx.take() {
                                let _ = tx.send(Err(format!("Peer GOAWAY ({})", error_code)));
                            }
                        }
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
            }
            Err(_) => break,
        }
    }

    for (_, mut st) in streams.drain() {
        if let Some(tx) = st.done_tx.take() {
            let _ = tx.send(Err("HTTP/2 connection closed".to_string()));
        }
    }
    for (_, tx) in pending_pings.drain() {
        let _ = tx.send(Err("HTTP/2 connection closed".to_string()));
    }
}

fn finish_client_h2_stream(sid: u32, st: &mut H2ClientStreamState) {
    let mut status: u16 = 200;
    let mut hdr_map: IndexMap<String, Vec<String>> = IndexMap::new();
    for (k, v) in &st.headers {
        if k == ":status" {
            status = v.parse::<u16>().unwrap_or(200);
        } else if !k.starts_with(':') {
            hdr_map.entry(k.clone()).or_default().push(v.clone());
        }
    }
    if let Some(tx) = st.done_tx.take() {
        let _ = tx.send(Ok(H2Response {
            stream_id: sid,
            status,
            headers: hdr_map,
            body: std::mem::take(&mut st.body),
        }));
    }
}

// --- HTTP/2 Server Connection Handler (h2c + TLS ALPN "h2") ---

struct H2ServerStream {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    recv_consumed: u32,
}

enum H2ServerOutMsg {
    Response {
        stream_id: u32,
        resp: WireResponse,
    },
}

fn serve_h2_connection(
    tcp: TcpStream,
    tls_cfg: Option<Arc<ServerConfig>>,
    router: &Arc<RwLock<RouterState>>,
    ctx: &WorkerEnvContext,
    running: &Arc<AtomicBool>,
    stats: &Arc<ServerStats>,
) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));

    let mut io = if let Some(cfg) = tls_cfg {
        match rustls::ServerConnection::new(cfg) {
            Ok(mut conn) => {
                let mut sock = tcp;
                if conn.complete_io(&mut sock).is_err() {
                    return;
                }
                let _ = sock.set_read_timeout(Some(Duration::from_millis(2)));
                ServerIo::Tls(rustls::StreamOwned::new(conn, sock))
            }
            Err(_) => return,
        }
    } else {
        ServerIo::Plain(tcp)
    };

    let mut preface = [0u8; 24];
    if read_exact_resilient(&mut io, &mut preface, running, false).unwrap_or(false) == false {
        return;
    }
    if &preface != H2_PREFACE {
        return;
    }

    if let ServerIo::Plain(ref s) = io {
        let _ = s.set_read_timeout(Some(Duration::from_millis(2)));
    }

    // Send server SETTINGS
    if io
        .write_all(
            &H2Frame::Settings {
                ack: false,
                params: vec![(0x3, 128), (0x4, 65535), (0x5, 16384)],
            }
            .encode(),
        )
        .is_err()
    {
        return;
    }
    let _ = io.flush();

    let mut hpack_dec = HpackCodec::default();
    let mut hpack_enc = HpackCodec::default();
    let mut assembler = H2ContinuationAssembler::default();
    let mut streams: HashMap<u32, H2ServerStream> = HashMap::new();
    let mut outbound_queues: HashMap<u32, (i64, VecDeque<u8>)> = HashMap::new();
    let mut conn_send_window: i64 = 65535;
    let mut peer_initial_window: i64 = 65535;
    let mut peer_max_frame_size: usize = 16384;
    let mut conn_recv_consumed: u32 = 0;

    let (out_tx, out_rx) = mpsc::channel::<H2ServerOutMsg>();

    while running.load(Ordering::Acquire) {
        // 1. Drain completed responses from worker pool
        while let Ok(H2ServerOutMsg::Response { stream_id, resp }) = out_rx.try_recv() {
            let mut hdrs = vec![(":status".to_string(), resp.status.to_string())];
            for (k, vals) in &resp.headers {
                let lk = k.to_ascii_lowercase();
                if lk == "connection" || lk == "transfer-encoding" {
                    continue;
                }
                for v in vals {
                    hdrs.push((lk.clone(), v.clone()));
                }
            }
            let block = hpack_enc.encode(&hdrs);
            let has_body = !resp.body.is_empty();
            let max_fs = peer_max_frame_size.max(16384);
            if block.len() <= max_fs {
                let f = H2Frame::Headers {
                    stream_id,
                    end_stream: !has_body,
                    end_headers: true,
                    priority: None,
                    header_block: block,
                    pad_len: 0,
                };
                if io.write_all(&f.encode()).is_err() {
                    return;
                }
            } else {
                let mut offset = max_fs;
                let f = H2Frame::Headers {
                    stream_id,
                    end_stream: !has_body,
                    end_headers: false,
                    priority: None,
                    header_block: block[..max_fs].to_vec(),
                    pad_len: 0,
                };
                if io.write_all(&f.encode()).is_err() {
                    return;
                }
                while offset < block.len() {
                    let end = (offset + max_fs).min(block.len());
                    let cf = H2Frame::Continuation {
                        stream_id,
                        end_headers: end == block.len(),
                        header_block: block[offset..end].to_vec(),
                    };
                    if io.write_all(&cf.encode()).is_err() {
                        return;
                    }
                    offset = end;
                }
            }
            if has_body {
                outbound_queues.insert(stream_id, (peer_initial_window, resp.body.into()));
            }
        }

        // Flush pending server DATA chunks respecting send windows
        let out_sids: Vec<u32> = outbound_queues.keys().copied().collect();
        for sid in out_sids {
            if conn_send_window <= 0 {
                break;
            }
            let mut done = false;
            if let Some((swin, q)) = outbound_queues.get_mut(&sid) {
                while !q.is_empty() && *swin > 0 && conn_send_window > 0 {
                    let take = (*swin as usize)
                        .min(conn_send_window as usize)
                        .min(peer_max_frame_size)
                        .min(q.len());
                    if take == 0 {
                        break;
                    }
                    let chunk: Vec<u8> = q.drain(..take).collect();
                    *swin -= chunk.len() as i64;
                    conn_send_window -= chunk.len() as i64;
                    let end_stream = q.is_empty();
                    done = end_stream;
                    if io
                        .write_all(
                            &H2Frame::Data {
                                stream_id: sid,
                                end_stream,
                                data: chunk,
                                pad_len: 0,
                            }
                            .encode(),
                        )
                        .is_err()
                    {
                        return;
                    }
                }
            }
            if done {
                outbound_queues.remove(&sid);
            }
        }
        let _ = io.flush();

        // 2. Read next inbound H2 frame
        match read_h2_frame_resilient(&mut io, 16384, running, true) {
            Ok(None) => continue,
            Ok(Some(frame)) => {
                if assembler.is_awaiting_continuation()
                    || matches!(frame, H2Frame::Headers { .. } | H2Frame::Continuation { .. })
                {
                    match assembler.process_frame(frame) {
                        Ok(Some((sid, end_stream, block))) => {
                            match hpack_dec.decode(&block) {
                                Ok(hdrs) => {
                                    let st = streams.entry(sid).or_insert_with(|| H2ServerStream {
                                        headers: Vec::new(),
                                        body: Vec::new(),
                                        recv_consumed: 0,
                                    });
                                    st.headers.extend(hdrs);
                                    if end_stream {
                                        if let Some(done_st) = streams.remove(&sid) {
                                            spawn_h2_server_stream(
                                                sid,
                                                done_st,
                                                router,
                                                ctx,
                                                stats,
                                                &out_tx,
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = io.write_all(
                                        &H2Frame::Goaway {
                                            last_stream_id: sid,
                                            error_code: e.code.as_u32(),
                                            debug_data: e.message.into_bytes(),
                                        }
                                        .encode(),
                                    );
                                    let _ = io.flush();
                                    return;
                                }
                            }
                            continue;
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            let _ = io.write_all(
                                &H2Frame::Goaway {
                                    last_stream_id: 0,
                                    error_code: e.code.as_u32(),
                                    debug_data: e.message.into_bytes(),
                                }
                                .encode(),
                            );
                            let _ = io.flush();
                            return;
                        }
                    }
                }

                match frame {
                    H2Frame::Data {
                        stream_id,
                        end_stream,
                        data,
                        ..
                    } => {
                        const MAX_H2_BODY_BYTES: usize = 16 * 1024 * 1024;
                        let dlen = data.len() as u32;
                        let exceeded = streams
                            .get(&stream_id)
                            .map(|st| st.body.len().saturating_add(data.len()) > MAX_H2_BODY_BYTES)
                            .unwrap_or(false);
                        if exceeded {
                            streams.remove(&stream_id);
                            conn_recv_consumed += dlen;
                            let _ = io.write_all(
                                &H2Frame::RstStream {
                                    stream_id,
                                    error_code: H2ErrorCode::Cancel.as_u32(),
                                }
                                .encode(),
                            );
                            let _ = io.flush();
                            continue;
                        }
                        if let Some(st) = streams.get_mut(&stream_id) {
                            st.body.extend_from_slice(&data);
                            st.recv_consumed += dlen;
                            conn_recv_consumed += dlen;
                            if conn_recv_consumed >= 32768 {
                                let _ = io.write_all(
                                    &H2Frame::WindowUpdate {
                                        stream_id: 0,
                                        increment: conn_recv_consumed,
                                    }
                                    .encode(),
                                );
                                conn_recv_consumed = 0;
                            }
                            if !end_stream && st.recv_consumed >= 32768 {
                                let _ = io.write_all(
                                    &H2Frame::WindowUpdate {
                                        stream_id,
                                        increment: st.recv_consumed,
                                    }
                                    .encode(),
                                );
                                st.recv_consumed = 0;
                            }
                            let _ = io.flush();
                            if end_stream {
                                if let Some(done_st) = streams.remove(&stream_id) {
                                    spawn_h2_server_stream(
                                        stream_id,
                                        done_st,
                                        router,
                                        ctx,
                                        stats,
                                        &out_tx,
                                    );
                                }
                            }
                        }
                    }
                    H2Frame::Settings { ack, params } => {
                        if !ack {
                            for (id, val) in params {
                                match id {
                                    0x1 => hpack_enc.table.set_protocol_max_size(val as usize),
                                    0x4 => {
                                        let delta = val as i64 - peer_initial_window;
                                        peer_initial_window = val as i64;
                                        for (swin, _) in outbound_queues.values_mut() {
                                            *swin += delta;
                                        }
                                    }
                                    0x5 => peer_max_frame_size = (val as usize).max(16384),
                                    _ => {}
                                }
                            }
                            let _ = io.write_all(
                                &H2Frame::Settings {
                                    ack: true,
                                    params: Vec::new(),
                                }
                                .encode(),
                            );
                            let _ = io.flush();
                        }
                    }
                    H2Frame::WindowUpdate {
                        stream_id,
                        increment,
                    } => {
                        if stream_id == 0 {
                            conn_send_window += increment as i64;
                        } else if let Some((swin, _)) = outbound_queues.get_mut(&stream_id) {
                            *swin += increment as i64;
                        }
                    }
                    H2Frame::Ping { ack, opaque } => {
                        if !ack {
                            let _ = io.write_all(&H2Frame::Ping { ack: true, opaque }.encode());
                            let _ = io.flush();
                        }
                    }
                    H2Frame::RstStream { stream_id, .. } => {
                        streams.remove(&stream_id);
                        outbound_queues.remove(&stream_id);
                    }
                    H2Frame::Goaway { .. } => return,
                    _ => {}
                }
            }
            Err(e) => {
                if !matches!(e.code, H2ErrorCode::ConnectError) {
                    let _ = io.write_all(
                        &H2Frame::Goaway {
                            last_stream_id: 0,
                            error_code: e.code.as_u32(),
                            debug_data: e.message.into_bytes(),
                        }
                        .encode(),
                    );
                    let _ = io.flush();
                }
                return;
            }
        }
    }
}

fn spawn_h2_server_stream(
    stream_id: u32,
    st: H2ServerStream,
    router: &Arc<RwLock<RouterState>>,
    ctx: &WorkerEnvContext,
    stats: &Arc<ServerStats>,
    out_tx: &mpsc::Sender<H2ServerOutMsg>,
) {
    stats.total_requests.fetch_add(1, Ordering::Relaxed);
    let router_snap = router.read().unwrap().clone();
    let ctx_clone = ctx.clone();
    let tx_clone = out_tx.clone();
    crate::async_rt::global_pool().execute(move || {
        let mut method = "GET".to_string();
        let mut path = "/".to_string();
        let mut hdrs: IndexMap<String, Vec<String>> = IndexMap::new();
        for (k, v) in st.headers {
            if k == ":method" {
                method = v;
            } else if k == ":path" {
                path = v;
            } else if k == ":authority" {
                hdrs.entry("host".to_string()).or_default().push(v);
            } else if !k.starts_with(':') {
                hdrs.entry(k).or_default().push(v);
            }
        }
        let empty_trailers = IndexMap::new();
        let resp = dispatch_shae_handler(
            &router_snap,
            &ctx_clone,
            &method,
            &path,
            &hdrs,
            &st.body,
            &empty_trailers,
            "HTTP/2",
            stream_id as u64,
        );
        let _ = tx_clone.send(H2ServerOutMsg::Response { stream_id, resp });
    });
}

// ============================================================================
// Part 3: QUIC (RFC 9000 / 9001), QPACK (RFC 9204), & HTTP/3 (RFC 9114)
// ============================================================================

pub const QUIC_V1_VERSION: u32 = 0x0000_0001;
pub const QUIC_VARINT_MAX: u64 = (1u64 << 62) - 1;

pub const QUIC_V1_INITIAL_SALT: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];

pub fn varint_encode(val: u64) -> Result<Vec<u8>, String> {
    if val <= 63 {
        Ok(vec![val as u8])
    } else if val <= 16_383 {
        Ok(((val as u16) | 0x4000).to_be_bytes().to_vec())
    } else if val <= 1_073_741_823 {
        Ok(((val as u32) | 0x8000_0000).to_be_bytes().to_vec())
    } else if val <= QUIC_VARINT_MAX {
        Ok((val | 0xC000_0000_0000_0000).to_be_bytes().to_vec())
    } else {
        Err(format!("QUIC varint overflow: {}", val))
    }
}

pub fn varint_decode(buf: &[u8]) -> Result<(u64, usize), String> {
    if buf.is_empty() {
        return Err("Truncated QUIC varint (empty)".to_string());
    }
    let prefix = buf[0] >> 6;
    let len = 1usize << prefix;
    if buf.len() < len {
        return Err(format!(
            "Truncated QUIC varint: need {} bytes, have {}",
            len,
            buf.len()
        ));
    }
    let mut val = (buf[0] & 0x3f) as u64;
    for &b in &buf[1..len] {
        val = (val << 8) | (b as u64);
    }
    Ok((val, len))
}

pub fn varint_decode_cursor(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let (v, n) = varint_decode(&buf[*pos..])?;
    *pos += n;
    Ok(v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicPacketType {
    Initial = 0,
    ZeroRtt = 1,
    Handshake = 2,
    Retry = 3,
    OneRtt = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuicHeader {
    pub packet_type: QuicPacketType,
    pub first_byte: u8,
    pub version: u32,
    pub dcid: Vec<u8>,
    pub scid: Vec<u8>,
    pub token: Vec<u8>,
    pub payload_length: usize, // includes pn_len + ciphertext + tag for Long Header
    pub pn_offset: usize,
    pub spin: bool,
}

pub fn parse_quic_header_to_pn(
    buf: &[u8],
    short_dcid_len: usize,
) -> Result<QuicHeader, String> {
    if buf.is_empty() {
        return Err("Empty QUIC packet".to_string());
    }
    let b0 = buf[0];
    if (b0 & 0x80) != 0 {
        if buf.len() < 7 {
            return Err("Truncated QUIC Long Header".to_string());
        }
        let version = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        if version != 0 && (b0 & 0x40) == 0 {
            return Err("QUIC Long Header missing fixed bit".to_string());
        }
        let pkt_type = match (b0 >> 4) & 0x03 {
            0 => QuicPacketType::Initial,
            1 => QuicPacketType::ZeroRtt,
            2 => QuicPacketType::Handshake,
            _ => QuicPacketType::Retry,
        };
        let mut pos = 5usize;
        let dcid_len = buf[pos] as usize;
        pos += 1;
        if dcid_len > 20 || pos + dcid_len >= buf.len() {
            return Err("Invalid or truncated QUIC DCID".to_string());
        }
        let dcid = buf[pos..pos + dcid_len].to_vec();
        pos += dcid_len;

        let scid_len = buf[pos] as usize;
        pos += 1;
        if scid_len > 20 || pos + scid_len > buf.len() {
            return Err("Invalid or truncated QUIC SCID".to_string());
        }
        let scid = buf[pos..pos + scid_len].to_vec();
        pos += scid_len;

        let mut token = Vec::new();
        if pkt_type == QuicPacketType::Initial {
            let tlen = varint_decode_cursor(buf, &mut pos)? as usize;
            if pos + tlen > buf.len() {
                return Err("Truncated QUIC Initial token".to_string());
            }
            token = buf[pos..pos + tlen].to_vec();
            pos += tlen;
        }
        let payload_length = if pkt_type == QuicPacketType::Retry {
            buf.len().saturating_sub(pos)
        } else {
            varint_decode_cursor(buf, &mut pos)? as usize
        };

        Ok(QuicHeader {
            packet_type: pkt_type,
            first_byte: b0,
            version,
            dcid,
            scid,
            token,
            payload_length,
            pn_offset: pos,
            spin: false,
        })
    } else {
        if short_dcid_len > 20 || 1 + short_dcid_len > buf.len() {
            return Err("Invalid or truncated QUIC Short Header".to_string());
        }
        if (b0 & 0x40) == 0 {
            return Err("QUIC Short Header missing fixed bit".to_string());
        }
        let dcid = buf[1..1 + short_dcid_len].to_vec();
        let pn_offset = 1 + short_dcid_len;
        Ok(QuicHeader {
            packet_type: QuicPacketType::OneRtt,
            first_byte: b0,
            version: QUIC_V1_VERSION,
            dcid,
            scid: Vec::new(),
            token: Vec::new(),
            payload_length: buf.len() - pn_offset,
            pn_offset,
            spin: (b0 & 0x20) != 0,
        })
    }
}

pub fn encode_pn(full_pn: u64, largest_acked: Option<u64>) -> (usize, Vec<u8>) {
    let num_unacked = match largest_acked {
        Some(la) => full_pn.saturating_sub(la),
        None => full_pn + 1,
    };
    let min_bits = if num_unacked == 0 {
        1
    } else {
        64 - (num_unacked * 2).leading_zeros()
    };
    let pn_len = ((min_bits as usize + 7) / 8).clamp(1, 4);
    let be = full_pn.to_be_bytes();
    (pn_len, be[8 - pn_len..].to_vec())
}

// RFC 9000 Appendix A.3 Packet Number Decoding
pub fn reconstruct_pn(largest_pn: i64, truncated_pn: u64, pn_len: usize) -> u64 {
    let clamped_len = pn_len.clamp(1, 4);
    let expected_pn = (largest_pn + 1) as u64;
    let pn_nbits = (clamped_len * 8) as u32;
    let pn_win = 1u64 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;
    let candidate_pn = (expected_pn & !pn_mask) | (truncated_pn & pn_mask);
    if candidate_pn + pn_hwin <= expected_pn && candidate_pn < (1u64 << 62) - pn_win {
        candidate_pn + pn_win
    } else if candidate_pn > expected_pn + pn_hwin && candidate_pn >= pn_win {
        candidate_pn - pn_win
    } else {
        candidate_pn
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuicFrame {
    Padding(usize),
    Ping,
    Ack {
        largest: u64,
        delay: u64,
        first_range: u64,
        ranges: Vec<(u64, u64)>,
        ecn: Option<(u64, u64, u64)>,
    },
    ResetStream {
        stream_id: u64,
        error_code: u64,
        final_size: u64,
    },
    StopSending {
        stream_id: u64,
        error_code: u64,
    },
    Crypto {
        offset: u64,
        data: Vec<u8>,
    },
    Stream {
        stream_id: u64,
        offset: Option<u64>,
        fin: bool,
        has_len: bool,
        data: Vec<u8>,
    },
    MaxData {
        max_data: u64,
    },
    MaxStreamData {
        stream_id: u64,
        max_stream_data: u64,
    },
    ConnectionClose {
        is_app: bool,
        error_code: u64,
        frame_type: Option<u64>,
        reason: String,
    },
}

impl QuicFrame {
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        match self {
            Self::Padding(count) => out.resize(*count, 0),
            Self::Ping => out.push(0x01),
            Self::Ack {
                largest,
                delay,
                first_range,
                ranges,
                ecn,
            } => {
                out.push(if ecn.is_some() { 0x03 } else { 0x02 });
                out.extend_from_slice(&varint_encode(*largest)?);
                out.extend_from_slice(&varint_encode(*delay)?);
                out.extend_from_slice(&varint_encode(ranges.len() as u64)?);
                out.extend_from_slice(&varint_encode(*first_range)?);
                for &(gap, ack_range) in ranges {
                    out.extend_from_slice(&varint_encode(gap)?);
                    out.extend_from_slice(&varint_encode(ack_range)?);
                }
                if let Some((ect0, ect1, ce)) = ecn {
                    out.extend_from_slice(&varint_encode(*ect0)?);
                    out.extend_from_slice(&varint_encode(*ect1)?);
                    out.extend_from_slice(&varint_encode(*ce)?);
                }
            }
            Self::ResetStream {
                stream_id,
                error_code,
                final_size,
            } => {
                out.push(0x04);
                out.extend_from_slice(&varint_encode(*stream_id)?);
                out.extend_from_slice(&varint_encode(*error_code)?);
                out.extend_from_slice(&varint_encode(*final_size)?);
            }
            Self::StopSending {
                stream_id,
                error_code,
            } => {
                out.push(0x05);
                out.extend_from_slice(&varint_encode(*stream_id)?);
                out.extend_from_slice(&varint_encode(*error_code)?);
            }
            Self::Crypto { offset, data } => {
                out.push(0x06);
                out.extend_from_slice(&varint_encode(*offset)?);
                out.extend_from_slice(&varint_encode(data.len() as u64)?);
                out.extend_from_slice(data);
            }
            Self::Stream {
                stream_id,
                offset,
                fin,
                has_len,
                data,
            } => {
                let mut ftype = 0x08u8;
                if offset.is_some() {
                    ftype |= 0x04;
                }
                if *has_len {
                    ftype |= 0x02;
                }
                if *fin {
                    ftype |= 0x01;
                }
                out.push(ftype);
                out.extend_from_slice(&varint_encode(*stream_id)?);
                if let Some(off) = offset {
                    out.extend_from_slice(&varint_encode(*off)?);
                }
                if *has_len {
                    out.extend_from_slice(&varint_encode(data.len() as u64)?);
                }
                out.extend_from_slice(data);
            }
            Self::MaxData { max_data } => {
                out.push(0x10);
                out.extend_from_slice(&varint_encode(*max_data)?);
            }
            Self::MaxStreamData {
                stream_id,
                max_stream_data,
            } => {
                out.push(0x11);
                out.extend_from_slice(&varint_encode(*stream_id)?);
                out.extend_from_slice(&varint_encode(*max_stream_data)?);
            }
            Self::ConnectionClose {
                is_app,
                error_code,
                frame_type,
                reason,
            } => {
                out.push(if *is_app { 0x1d } else { 0x1c });
                out.extend_from_slice(&varint_encode(*error_code)?);
                if !*is_app {
                    out.extend_from_slice(&varint_encode(frame_type.unwrap_or(0))?);
                }
                let rb = reason.as_bytes();
                out.extend_from_slice(&varint_encode(rb.len() as u64)?);
                out.extend_from_slice(rb);
            }
        }
        Ok(out)
    }

    pub fn decode_all(buf: &[u8]) -> Result<Vec<Self>, String> {
        let mut pos = 0usize;
        let mut frames = Vec::new();
        while pos < buf.len() {
            let ftype = varint_decode_cursor(buf, &mut pos)?;
            match ftype {
                0x00 => {
                    let mut count = 1usize;
                    while pos < buf.len() && buf[pos] == 0x00 {
                        pos += 1;
                        count += 1;
                    }
                    frames.push(Self::Padding(count));
                }
                0x01 => frames.push(Self::Ping),
                0x02 | 0x03 => {
                    let largest = varint_decode_cursor(buf, &mut pos)?;
                    let delay = varint_decode_cursor(buf, &mut pos)?;
                    let range_count = varint_decode_cursor(buf, &mut pos)? as usize;
                    let first_range = varint_decode_cursor(buf, &mut pos)?;
                    let mut ranges = Vec::with_capacity(range_count.min(256));
                    for _ in 0..range_count {
                        let gap = varint_decode_cursor(buf, &mut pos)?;
                        let ack_len = varint_decode_cursor(buf, &mut pos)?;
                        ranges.push((gap, ack_len));
                    }
                    let ecn = if ftype == 0x03 {
                        let e0 = varint_decode_cursor(buf, &mut pos)?;
                        let e1 = varint_decode_cursor(buf, &mut pos)?;
                        let ce = varint_decode_cursor(buf, &mut pos)?;
                        Some((e0, e1, ce))
                    } else {
                        None
                    };
                    frames.push(Self::Ack {
                        largest,
                        delay,
                        first_range,
                        ranges,
                        ecn,
                    });
                }
                0x04 => {
                    let stream_id = varint_decode_cursor(buf, &mut pos)?;
                    let error_code = varint_decode_cursor(buf, &mut pos)?;
                    let final_size = varint_decode_cursor(buf, &mut pos)?;
                    frames.push(Self::ResetStream {
                        stream_id,
                        error_code,
                        final_size,
                    });
                }
                0x05 => {
                    let stream_id = varint_decode_cursor(buf, &mut pos)?;
                    let error_code = varint_decode_cursor(buf, &mut pos)?;
                    frames.push(Self::StopSending {
                        stream_id,
                        error_code,
                    });
                }
                0x06 => {
                    let offset = varint_decode_cursor(buf, &mut pos)?;
                    let len = varint_decode_cursor(buf, &mut pos)? as usize;
                    if pos + len > buf.len() {
                        return Err("Truncated QUIC CRYPTO frame".to_string());
                    }
                    let data = buf[pos..pos + len].to_vec();
                    pos += len;
                    frames.push(Self::Crypto { offset, data });
                }
                0x08..=0x0f => {
                    let has_off = (ftype & 0x04) != 0;
                    let has_len = (ftype & 0x02) != 0;
                    let fin = (ftype & 0x01) != 0;
                    let stream_id = varint_decode_cursor(buf, &mut pos)?;
                    let offset = if has_off {
                        Some(varint_decode_cursor(buf, &mut pos)?)
                    } else {
                        None
                    };
                    let data = if has_len {
                        let len = varint_decode_cursor(buf, &mut pos)? as usize;
                        if pos + len > buf.len() {
                            return Err("Truncated QUIC STREAM frame".to_string());
                        }
                        let d = buf[pos..pos + len].to_vec();
                        pos += len;
                        d
                    } else {
                        let d = buf[pos..].to_vec();
                        pos = buf.len();
                        d
                    };
                    frames.push(Self::Stream {
                        stream_id,
                        offset,
                        fin,
                        has_len,
                        data,
                    });
                }
                0x10 => {
                    let max_data = varint_decode_cursor(buf, &mut pos)?;
                    frames.push(Self::MaxData { max_data });
                }
                0x11 => {
                    let stream_id = varint_decode_cursor(buf, &mut pos)?;
                    let max_stream_data = varint_decode_cursor(buf, &mut pos)?;
                    frames.push(Self::MaxStreamData {
                        stream_id,
                        max_stream_data,
                    });
                }
                0x1c | 0x1d => {
                    let is_app = ftype == 0x1d;
                    let error_code = varint_decode_cursor(buf, &mut pos)?;
                    let frame_type = if !is_app {
                        Some(varint_decode_cursor(buf, &mut pos)?)
                    } else {
                        None
                    };
                    let rlen = varint_decode_cursor(buf, &mut pos)? as usize;
                    if pos + rlen > buf.len() {
                        return Err("Truncated CONNECTION_CLOSE reason".to_string());
                    }
                    let reason = String::from_utf8_lossy(&buf[pos..pos + rlen]).into_owned();
                    pos += rlen;
                    frames.push(Self::ConnectionClose {
                        is_app,
                        error_code,
                        frame_type,
                        reason,
                    });
                }
                other => {
                    return Err(format!("Unsupported QUIC frame type 0x{:x}", other));
                }
            }
        }
        Ok(frames)
    }
}

// --- RFC 9001 Packet Protection via `ring` ---

struct HkdfOutputLen(usize);
impl hkdf::KeyType for HkdfOutputLen {
    fn len(&self) -> usize {
        self.0
    }
}

pub fn hkdf_expand_label(
    prk: &hkdf::Prk,
    label: &[u8],
    context: &[u8],
    out_len: usize,
) -> Result<Vec<u8>, String> {
    let full_label_len = 6 + label.len();
    let mut info = Vec::with_capacity(2 + 1 + full_label_len + 1 + context.len());
    info.extend_from_slice(&(out_len as u16).to_be_bytes());
    info.push(full_label_len as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);

    let mut out = vec![0u8; out_len];
    prk.expand(&[&info], HkdfOutputLen(out_len))
        .map_err(|_| "HKDF-Expand-Label failed".to_string())?
        .fill(&mut out)
        .map_err(|_| "HKDF-Expand-Label fill failed".to_string())?;
    Ok(out)
}

pub struct QuicKeys {
    pub secret: Vec<u8>,
    pub key: Vec<u8>,
    pub iv: [u8; 12],
    pub hp: Vec<u8>,
    aead_key: LessSafeKey,
    hp_key: aead::quic::HeaderProtectionKey,
}

impl QuicKeys {
    pub fn from_secret(secret: &[u8]) -> Result<Self, String> {
        let prk = hkdf::Prk::new_less_safe(HKDF_SHA256, secret);
        let key = hkdf_expand_label(&prk, b"quic key", b"", 16)?;
        let iv_vec = hkdf_expand_label(&prk, b"quic iv", b"", 12)?;
        let hp = hkdf_expand_label(&prk, b"quic hp", b"", 16)?;
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&iv_vec);

        let unbound = UnboundKey::new(&AES_128_GCM, &key)
            .map_err(|_| "Invalid AES-128-GCM key".to_string())?;
        let aead_key = LessSafeKey::new(unbound);
        let hp_key = aead::quic::HeaderProtectionKey::new(&aead::quic::AES_128, &hp)
            .map_err(|_| "Invalid QUIC HeaderProtectionKey".to_string())?;

        Ok(Self {
            secret: secret.to_vec(),
            key,
            iv,
            hp,
            aead_key,
            hp_key,
        })
    }

    fn make_nonce(&self, pn: u64) -> Nonce {
        let mut nonce = self.iv;
        let pn_be = pn.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= pn_be[i];
        }
        Nonce::assume_unique_for_key(nonce)
    }
}

pub fn derive_initial_secrets(dcid: &[u8]) -> Result<(Vec<u8>, QuicKeys, QuicKeys), String> {
    if dcid.len() > 20 {
        return Err("QUIC DCID exceeds 20 bytes".to_string());
    }
    let salt = hkdf::Salt::new(HKDF_SHA256, &QUIC_V1_INITIAL_SALT);
    let initial_prk = salt.extract(dcid);

    let mut initial_secret = vec![0u8; 32];
    initial_prk
        .expand(&[], HkdfOutputLen(32))
        .ok(); // Note: PRK bytes aren't directly exposed, so derive from initial_prk directly:
    // Re-derive client/server secrets directly from initial_prk:
    let client_secret = hkdf_expand_label(&initial_prk, b"client in", b"", 32)?;
    let server_secret = hkdf_expand_label(&initial_prk, b"server in", b"", 32)?;
    let _ = &mut initial_secret;
    let client_keys = QuicKeys::from_secret(&client_secret)?;
    let server_keys = QuicKeys::from_secret(&server_secret)?;
    // Combine client+server secrets to seed 1-RTT HKDF when operating in pure-ring handshake mode
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(&client_secret);
    seed.extend_from_slice(&server_secret);
    Ok((seed, client_keys, server_keys))
}

pub fn derive_hkdf_traffic_keys(
    seed: &[u8],
    client_scid: &[u8],
    server_scid: &[u8],
) -> Result<(QuicKeys, QuicKeys), String> {
    let mut context = Vec::with_capacity(client_scid.len() + server_scid.len());
    context.extend_from_slice(client_scid);
    context.extend_from_slice(server_scid);
    let prk = hkdf::Salt::new(HKDF_SHA256, &context).extract(seed);
    let c_sec = hkdf_expand_label(&prk, b"shae c ap", &context, 32)?;
    let s_sec = hkdf_expand_label(&prk, b"shae s ap", &context, 32)?;
    Ok((
        QuicKeys::from_secret(&c_sec)?,
        QuicKeys::from_secret(&s_sec)?,
    ))
}

pub enum QuicCipher {
    Ring(QuicKeys),
    Rustls(rustls::quic::DirectionalKeys),
}

impl QuicCipher {
    fn seal_payload(&self, pn: u64, aad: &[u8], buf: &mut Vec<u8>) -> Result<(), String> {
        match self {
            Self::Ring(k) => {
                let nonce = k.make_nonce(pn);
                k.aead_key
                    .seal_in_place_append_tag(nonce, Aad::from(aad), buf)
                    .map_err(|_| "QUIC AEAD seal failed".to_string())
            }
            Self::Rustls(dk) => {
                let tag = dk
                    .packet
                    .encrypt_in_place(pn, aad, buf)
                    .map_err(|e| format!("rustls QUIC seal failed: {}", e))?;
                buf.extend_from_slice(tag.as_ref());
                Ok(())
            }
        }
    }

    fn open_payload<'a>(
        &self,
        pn: u64,
        aad: &[u8],
        buf: &'a mut [u8],
    ) -> Result<&'a [u8], String> {
        match self {
            Self::Ring(k) => {
                let nonce = k.make_nonce(pn);
                k.aead_key
                    .open_in_place(nonce, Aad::from(aad), buf)
                    .map(|slice| &*slice)
                    .map_err(|_| "QUIC AEAD authentication failed".to_string())
            }
            Self::Rustls(dk) => dk
                .packet
                .decrypt_in_place(pn, aad, buf)
                .map_err(|e| format!("rustls QUIC open failed: {}", e)),
        }
    }

    fn apply_hp(&self, sample: &[u8], first: &mut u8, pn_bytes: &mut [u8], is_long: bool) -> Result<(), String> {
        match self {
            Self::Ring(k) => {
                let mask = k
                    .hp_key
                    .new_mask(sample)
                    .map_err(|_| "Header protection sample invalid".to_string())?;
                *first ^= mask[0] & if is_long { 0x0f } else { 0x1f };
                for i in 0..pn_bytes.len() {
                    pn_bytes[i] ^= mask[1 + i];
                }
                Ok(())
            }
            Self::Rustls(dk) => dk
                .header
                .encrypt_in_place(sample, first, pn_bytes)
                .map_err(|e| format!("rustls HP encrypt failed: {}", e)),
        }
    }

    fn remove_hp(&self, sample: &[u8], first: &mut u8, pn_bytes: &mut [u8], is_long: bool) -> Result<(), String> {
        match self {
            Self::Ring(k) => {
                let mask = k
                    .hp_key
                    .new_mask(sample)
                    .map_err(|_| "Header protection sample invalid".to_string())?;
                *first ^= mask[0] & if is_long { 0x0f } else { 0x1f };
                let actual_pn_len = ((*first & 0x03) as usize) + 1;
                for i in 0..actual_pn_len.min(pn_bytes.len()) {
                    pn_bytes[i] ^= mask[1 + i];
                }
                Ok(())
            }
            Self::Rustls(dk) => {
                // Note: rustls HeaderProtectionKey::decrypt_in_place unmasks `first` and exact `pn_bytes` slice
                let mask_sample = sample;
                // Unmask first byte by calling with 4-byte slice? rustls expects exact pn length!
                // We first unmask `first` using a dummy or inspect rustls decrypt_in_place:
                // Actually rustls decrypt_in_place unmasks `first`, reads `(first & 0x03) + 1`, then unmasks pn[..pn_len]!
                dk.header
                    .decrypt_in_place(mask_sample, first, pn_bytes)
                    .map_err(|e| format!("rustls HP decrypt failed: {}", e))
            }
        }
    }
}

pub fn seal_quic_packet(
    pkt_type: QuicPacketType,
    dcid: &[u8],
    scid: &[u8],
    token: &[u8],
    pn: u64,
    largest_acked: Option<u64>,
    frames: &[QuicFrame],
    cipher: &QuicCipher,
    min_total_size: usize,
) -> Result<Vec<u8>, String> {
    if dcid.len() > 20 || scid.len() > 20 {
        return Err("QUIC CID length exceeds 20 bytes".to_string());
    }
    let (pn_len, pn_bytes) = encode_pn(pn, largest_acked);
    let mut plaintext = Vec::new();
    for f in frames {
        plaintext.extend_from_slice(&f.encode()?);
    }
    // Guarantee at least 4 bytes of (pn_len + plaintext) so ciphertext + tag >= 20 bytes for HP sample!
    while pn_len + plaintext.len() < 4 {
        plaintext.push(0x00);
    }

    let is_long = pkt_type != QuicPacketType::OneRtt;
    let mut header = Vec::new();
    if is_long {
        let pt_bits = (pkt_type as u8) << 4;
        header.push(0xc0 | pt_bits | ((pn_len as u8) - 1));
        header.extend_from_slice(&QUIC_V1_VERSION.to_be_bytes());
        header.push(dcid.len() as u8);
        header.extend_from_slice(dcid);
        header.push(scid.len() as u8);
        header.extend_from_slice(scid);
        if pkt_type == QuicPacketType::Initial {
            header.extend_from_slice(&varint_encode(token.len() as u64)?);
            header.extend_from_slice(token);
            let pre_len = header.len() + 2 + pn_len + plaintext.len() + 16;
            if pre_len < min_total_size {
                plaintext.resize(plaintext.len() + (min_total_size - pre_len), 0x00);
            }
        }
        let payload_len = pn_len + plaintext.len() + 16;
        header.extend_from_slice(&varint_encode(payload_len as u64)?);
    } else {
        header.push(0x40 | ((pn_len as u8) - 1));
        header.extend_from_slice(dcid);
    }

    let pn_offset = header.len();
    header.extend_from_slice(&pn_bytes);

    let mut encrypted = plaintext;
    cipher.seal_payload(pn, &header, &mut encrypted)?;

    let mut packet = header;
    packet.extend_from_slice(&encrypted);

    let sample_start = pn_offset + 4;
    if packet.len() < sample_start + 16 {
        return Err("Packet too short for 16-byte header protection sample".to_string());
    }
    let sample = packet[sample_start..sample_start + 16].to_vec();
    let (head_slice, pn_slice) = packet.split_at_mut(pn_offset);
    cipher.apply_hp(&sample, &mut head_slice[0], &mut pn_slice[..pn_len], is_long)?;
    Ok(packet)
}

#[derive(Debug, Clone)]
pub struct QuicOpenedPacket {
    pub header: QuicHeader,
    pub pn: u64,
    pub frames: Vec<QuicFrame>,
}

pub fn open_quic_packet(
    raw: &[u8],
    short_dcid_len: usize,
    largest_pn: i64,
    cipher: &QuicCipher,
) -> Result<(QuicOpenedPacket, usize), String> {
    let hdr = parse_quic_header_to_pn(raw, short_dcid_len)?;
    let is_long = hdr.packet_type != QuicPacketType::OneRtt;
    let total_pkt_len = if is_long {
        hdr.pn_offset + hdr.payload_length
    } else {
        raw.len()
    };
    if raw.len() < total_pkt_len || total_pkt_len < hdr.pn_offset + 20 {
        return Err("QUIC packet too short for AEAD + HP sample".to_string());
    }

    let mut pkt = raw[..total_pkt_len].to_vec();
    let sample = pkt[hdr.pn_offset + 4..hdr.pn_offset + 20].to_vec();

    let (first_part, rest_part) = pkt.split_at_mut(hdr.pn_offset);
    let max_pn = 4usize.min(rest_part.len());
    // For Ring we unmask first byte then exact pn_len; for Rustls we inspect first byte via Ring-compatible mask or pass slice
    match cipher {
        QuicCipher::Ring(_) => {
            cipher.remove_hp(&sample, &mut first_part[0], &mut rest_part[..max_pn], is_long)?;
        }
        QuicCipher::Rustls(dk) => {
            // Note: rustls::quic::HeaderProtectionKey::decrypt_in_place expects exact pn slice length,
            // so pass a 4-byte scratch first to learn pn_len? No, decrypt_in_place unmasks `first` first,
            // reads `pn_len = (first & 0x03) + 1`, and only unmasks `packet_number[..pn_len]`!
            // Verify: rustls decrypt_in_place returns Ok(()) when passed &mut rest_part[..4] if pn_len <= 4,
            // or wait: in rustls, `packet_number` slice length MUST match? Let's pass `rest_part[..4]`?
            // Actually let's check: in rustls `ring.rs`, `decrypt_in_place` checks `packet_number.len()`!
            // If we copy 4 bytes into `[u8;4]`, decrypt_in_place unmasks `first` and `tmp[..4]` safely if slice matches,
            // OR let's unmask `first` safely without relying on slice-len panic!
            let mut tmp = [0u8; 4];
            tmp[..max_pn].copy_from_slice(&rest_part[..max_pn]);
            let mut first_copy = first_part[0];
            if dk.header.decrypt_in_place(&sample, &mut first_copy, &mut tmp).is_ok() {
                first_part[0] = first_copy;
                let pn_len = ((first_copy & 0x03) as usize) + 1;
                rest_part[..pn_len].copy_from_slice(&tmp[..pn_len]);
            } else {
                // Retry with exact pn_len 1..=4
                let mut ok = false;
                for try_len in 1..=max_pn {
                    let mut fc = first_part[0];
                    let mut slice = rest_part[..try_len].to_vec();
                    if dk.header.decrypt_in_place(&sample, &mut fc, &mut slice).is_ok() {
                        if ((fc & 0x03) as usize) + 1 == try_len {
                            first_part[0] = fc;
                            rest_part[..try_len].copy_from_slice(&slice);
                            ok = true;
                            break;
                        }
                    }
                }
                if !ok {
                    return Err("rustls HP decrypt failed".to_string());
                }
            }
        }
    }

    let pn_len = ((pkt[0] & 0x03) as usize) + 1;
    let mut trunc_pn = 0u64;
    for &b in &pkt[hdr.pn_offset..hdr.pn_offset + pn_len] {
        trunc_pn = (trunc_pn << 8) | (b as u64);
    }
    let full_pn = reconstruct_pn(largest_pn, trunc_pn, pn_len);
    let aad_end = hdr.pn_offset + pn_len;
    let (aad, ciphertext) = pkt.split_at_mut(aad_end);
    let plaintext = cipher.open_payload(full_pn, aad, ciphertext)?;
    let frames = QuicFrame::decode_all(plaintext)?;

    Ok((
        QuicOpenedPacket {
            header: hdr,
            pn: full_pn,
            frames,
        },
        total_pkt_len,
    ))
}

// --- RFC 9204 QPACK Static Table (99 Entries, indices 0..=98) & Codec ---

pub const QPACK_STATIC_TABLE: [(&str, &str); 99] = [
    (":authority", ""),
    (":path", "/"),
    ("age", "0"),
    ("content-disposition", ""),
    ("content-length", "0"),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("referer", ""),
    ("set-cookie", ""),
    (":method", "CONNECT"),
    (":method", "DELETE"),
    (":method", "GET"),
    (":method", "HEAD"),
    (":method", "OPTIONS"),
    (":method", "POST"),
    (":method", "PUT"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "103"),
    (":status", "200"),
    (":status", "304"),
    (":status", "404"),
    (":status", "503"),
    ("accept", "*/*"),
    ("accept", "application/dns-message"),
    ("accept-encoding", "gzip, deflate, br"),
    ("accept-ranges", "bytes"),
    ("access-control-allow-headers", "cache-control"),
    ("access-control-allow-headers", "content-type"),
    ("access-control-allow-origin", "*"),
    ("cache-control", "max-age=0"),
    ("cache-control", "max-age=2592000"),
    ("cache-control", "max-age=604800"),
    ("cache-control", "no-cache"),
    ("cache-control", "no-store"),
    ("cache-control", "public, max-age=31536000"),
    ("content-encoding", "br"),
    ("content-encoding", "gzip"),
    ("content-type", "application/dns-message"),
    ("content-type", "application/javascript"),
    ("content-type", "application/json"),
    ("content-type", "application/x-www-form-urlencoded"),
    ("content-type", "image/gif"),
    ("content-type", "image/jpeg"),
    ("content-type", "image/png"),
    ("content-type", "text/css"),
    ("content-type", "text/html; charset=utf-8"),
    ("content-type", "text/plain"),
    ("content-type", "text/plain;charset=utf-8"),
    ("range", "bytes=0-"),
    ("strict-transport-security", "max-age=31536000"),
    ("strict-transport-security", "max-age=31536000; includesubdomains"),
    ("strict-transport-security", "max-age=31536000; includesubdomains; preload"),
    ("vary", "accept-encoding"),
    ("vary", "origin"),
    ("x-content-type-options", "nosniff"),
    ("x-xss-protection", "1; mode=block"),
    (":status", "100"),
    (":status", "204"),
    (":status", "206"),
    (":status", "302"),
    (":status", "400"),
    (":status", "403"),
    (":status", "421"),
    (":status", "425"),
    (":status", "500"),
    ("accept-language", ""),
    ("access-control-allow-credentials", "FALSE"),
    ("access-control-allow-credentials", "TRUE"),
    ("access-control-allow-headers", "*"),
    ("access-control-allow-methods", "get"),
    ("access-control-allow-methods", "get, post, options"),
    ("access-control-allow-methods", "options"),
    ("access-control-expose-headers", "content-length"),
    ("access-control-request-headers", "content-type"),
    ("access-control-request-method", "get"),
    ("access-control-request-method", "post"),
    ("alt-svc", "clear"),
    ("authorization", ""),
    ("content-security-policy", "script-src 'none'; object-src 'none'; base-uri 'none'"),
    ("early-data", "1"),
    ("expect-ct", ""),
    ("forwarded", ""),
    ("if-range", ""),
    ("origin", ""),
    ("purpose", "prefetch"),
    ("server", ""),
    ("timing-allow-origin", "*"),
    ("upgrade-insecure-requests", "1"),
    ("user-agent", ""),
    ("x-forwarded-for", ""),
    ("x-frame-options", "deny"),
    ("x-frame-options", "sameorigin"),
];

pub fn qpack_encode_headers(headers: &[(String, String)], use_huffman: bool) -> Vec<u8> {
    let mut out = Vec::new();
    // Field Section Prefix: Required Insert Count = 0, Delta Base = 0
    out.push(0x00);
    out.push(0x00);

    for (name, val) in headers {
        let lname = name.to_ascii_lowercase();
        let mut exact_idx = None;
        let mut name_idx = None;
        for (i, &(k, v)) in QPACK_STATIC_TABLE.iter().enumerate() {
            if k == lname {
                if v == val {
                    exact_idx = Some(i);
                    break;
                }
                if name_idx.is_none() {
                    name_idx = Some(i);
                }
            }
        }
        if let Some(idx) = exact_idx {
            // Indexed Field Line (T=1): 11xxxxxx (6-bit prefix)
            hpack_encode_int(&mut out, 6, 0xc0, idx as u64);
        } else if let Some(nidx) = name_idx {
            // Literal Field Line With Name Reference (N=0, T=1): 0101xxxx (4-bit prefix)
            hpack_encode_int(&mut out, 4, 0x50, nidx as u64);
            hpack_encode_str(&mut out, val.as_bytes(), use_huffman);
        } else {
            // Literal Field Line With Literal Name: 001 N H NameLen(3+)
            let name_bytes = lname.as_bytes();
            if use_huffman {
                let h = hpack_huffman_encode(name_bytes);
                if h.len() < name_bytes.len() {
                    hpack_encode_int(&mut out, 3, 0x28, h.len() as u64);
                    out.extend_from_slice(&h);
                } else {
                    hpack_encode_int(&mut out, 3, 0x20, name_bytes.len() as u64);
                    out.extend_from_slice(name_bytes);
                }
            } else {
                hpack_encode_int(&mut out, 3, 0x20, name_bytes.len() as u64);
                out.extend_from_slice(name_bytes);
            }
            hpack_encode_str(&mut out, val.as_bytes(), use_huffman);
        }
    }
    out
}

pub fn qpack_decode_headers(buf: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut pos = 0usize;
    let req_insert_count = hpack_decode_int(buf, &mut pos, 8).map_err(|e| e.message)?;
    if req_insert_count != 0 {
        return Err("QPACK dynamic table references not enabled (RIC != 0)".to_string());
    }
    let _delta_base = hpack_decode_int(buf, &mut pos, 7).map_err(|e| e.message)?;

    let mut out = Vec::new();
    while pos < buf.len() {
        let b = buf[pos];
        if (b & 0x80) != 0 {
            // Indexed Field Line
            let is_static = (b & 0x40) != 0;
            if !is_static {
                return Err("QPACK dynamic indexed entry not supported".to_string());
            }
            let idx = hpack_decode_int(buf, &mut pos, 6).map_err(|e| e.message)? as usize;
            let &(k, v) = QPACK_STATIC_TABLE
                .get(idx)
                .ok_or_else(|| format!("QPACK static index {} out of bounds", idx))?;
            out.push((k.to_string(), v.to_string()));
        } else if (b & 0xc0) == 0x40 {
            // Literal Field Line With Name Reference
            let is_static = (b & 0x10) != 0;
            if !is_static {
                return Err("QPACK dynamic name reference not supported".to_string());
            }
            let idx = hpack_decode_int(buf, &mut pos, 4).map_err(|e| e.message)? as usize;
            let &(k, _) = QPACK_STATIC_TABLE
                .get(idx)
                .ok_or_else(|| format!("QPACK static name index {} out of bounds", idx))?;
            let val = hpack_decode_str(buf, &mut pos).map_err(|e| e.message)?;
            out.push((k.to_string(), val));
        } else if (b & 0xe0) == 0x20 {
            // Literal Field Line With Literal Name: 001 N H NameLen(3+)
            let is_huff = (b & 0x08) != 0;
            let nlen_u64 = hpack_decode_int(buf, &mut pos, 3).map_err(|e| e.message)?;
            let nlen = usize::try_from(nlen_u64)
                .map_err(|_| "QPACK literal header name length overflow".to_string())?;
            let end = pos
                .checked_add(nlen)
                .ok_or_else(|| "QPACK literal header name length overflow".to_string())?;
            if end > buf.len() {
                return Err("Truncated QPACK literal header name".to_string());
            }
            let raw_name = &buf[pos..end];
            pos = end;
            let name_bytes = if is_huff {
                hpack_huffman_decode(raw_name).map_err(|e| e.message)?
            } else {
                raw_name.to_vec()
            };
            let name = String::from_utf8_lossy(&name_bytes).into_owned();
            let val = hpack_decode_str(buf, &mut pos).map_err(|e| e.message)?;
            out.push((name, val));
        } else {
            return Err(format!("Unsupported QPACK instruction byte 0x{:02x}", b));
        }
    }
    Ok(out)
}

// --- RFC 9114 HTTP/3 Stream Types & Frames ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H3StreamType {
    Control = 0x00,
    Push = 0x01,
    QpackEncoder = 0x02,
    QpackDecoder = 0x03,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H3Frame {
    Data(Vec<u8>),
    Headers(Vec<u8>),
    CancelPush(u64),
    Settings(Vec<(u64, u64)>),
    PushPromise {
        push_id: u64,
        encoded_field_section: Vec<u8>,
    },
    Goaway(u64),
    MaxPushId(u64),
    Unknown {
        frame_type: u64,
        payload: Vec<u8>,
    },
}

impl H3Frame {
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let (ftype, payload) = match self {
            Self::Data(d) => (0x00u64, d.clone()),
            Self::Headers(h) => (0x01u64, h.clone()),
            Self::CancelPush(id) => (0x03u64, varint_encode(*id)?),
            Self::Settings(params) => {
                let mut p = Vec::new();
                for (id, val) in params {
                    p.extend_from_slice(&varint_encode(*id)?);
                    p.extend_from_slice(&varint_encode(*val)?);
                }
                (0x04u64, p)
            }
            Self::PushPromise {
                push_id,
                encoded_field_section,
            } => {
                let mut p = varint_encode(*push_id)?;
                p.extend_from_slice(encoded_field_section);
                (0x05u64, p)
            }
            Self::Goaway(id) => (0x07u64, varint_encode(*id)?),
            Self::MaxPushId(id) => (0x0du64, varint_encode(*id)?),
            Self::Unknown {
                frame_type,
                payload,
            } => (*frame_type, payload.clone()),
        };

        let mut out = varint_encode(ftype)?;
        out.extend_from_slice(&varint_encode(payload.len() as u64)?);
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn decode_all(buf: &[u8]) -> Result<Vec<Self>, String> {
        let mut pos = 0usize;
        let mut out = Vec::new();
        while pos < buf.len() {
            let ftype = varint_decode_cursor(buf, &mut pos)?;
            let flen_u64 = varint_decode_cursor(buf, &mut pos)?;
            let flen = usize::try_from(flen_u64)
                .map_err(|_| "HTTP/3 frame length overflow".to_string())?;
            let end = pos
                .checked_add(flen)
                .ok_or_else(|| "HTTP/3 frame length overflow".to_string())?;
            if end > buf.len() {
                return Err("Truncated HTTP/3 frame payload".to_string());
            }
            let payload = &buf[pos..end];
            pos = end;
            let frame = match ftype {
                0x00 => Self::Data(payload.to_vec()),
                0x01 => Self::Headers(payload.to_vec()),
                0x03 => {
                    let mut p = 0;
                    let id = varint_decode_cursor(payload, &mut p)?;
                    Self::CancelPush(id)
                }
                0x04 => {
                    let mut p = 0;
                    let mut params = Vec::new();
                    while p < payload.len() {
                        let k = varint_decode_cursor(payload, &mut p)?;
                        let v = varint_decode_cursor(payload, &mut p)?;
                        params.push((k, v));
                    }
                    Self::Settings(params)
                }
                0x05 => {
                    let mut p = 0;
                    let push_id = varint_decode_cursor(payload, &mut p)?;
                    Self::PushPromise {
                        push_id,
                        encoded_field_section: payload[p..].to_vec(),
                    }
                }
                0x07 => {
                    let mut p = 0;
                    let id = varint_decode_cursor(payload, &mut p)?;
                    Self::Goaway(id)
                }
                0x0d => {
                    let mut p = 0;
                    let id = varint_decode_cursor(payload, &mut p)?;
                    Self::MaxPushId(id)
                }
                other => Self::Unknown {
                    frame_type: other,
                    payload: payload.to_vec(),
                },
            };
            out.push(frame);
        }
        Ok(out)
    }
}

// --- Real UDP Loopback HTTP/3 Server & Client ---

const MAX_H3_STREAM_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct H3StreamReassembler {
    chunks: HashMap<u64, Vec<u8>>,
    fin_offset: Option<u64>,
    buffered_bytes: usize,
    overflowed: bool,
}

impl H3StreamReassembler {
    fn push(&mut self, offset: u64, data: &[u8], fin: bool) -> Result<(), String> {
        if self.overflowed {
            return Err("H3 stream already exceeded max buffer size".to_string());
        }
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| {
                self.overflowed = true;
                "H3 stream offset overflow".to_string()
            })?;
        if end > MAX_H3_STREAM_BYTES as u64 {
            self.overflowed = true;
            return Err("H3 stream exceeds MAX_H3_STREAM_BYTES".to_string());
        }
        if !data.is_empty() {
            let new_buffered = self.buffered_bytes.saturating_add(data.len());
            if new_buffered > MAX_H3_STREAM_BYTES {
                self.overflowed = true;
                return Err("H3 stream buffered bytes exceed MAX_H3_STREAM_BYTES".to_string());
            }
            if let Some(prev) = self.chunks.insert(offset, data.to_vec()) {
                self.buffered_bytes = self
                    .buffered_bytes
                    .saturating_sub(prev.len())
                    .saturating_add(data.len());
            } else {
                self.buffered_bytes = new_buffered;
            }
        }
        if fin {
            self.fin_offset = Some(end);
        }
        Ok(())
    }

    fn take_if_complete(&mut self) -> Option<Vec<u8>> {
        if self.overflowed {
            return None;
        }
        let fin_off = self.fin_offset?;
        let total_len = usize::try_from(fin_off)
            .ok()
            .filter(|&n| n <= MAX_H3_STREAM_BYTES)?;
        let mut cursor = 0u64;
        let mut ordered_offsets = Vec::new();
        while cursor < fin_off {
            let chunk = self.chunks.get(&cursor)?;
            if chunk.is_empty() {
                return None;
            }
            ordered_offsets.push(cursor);
            cursor = cursor.checked_add(chunk.len() as u64)?;
        }
        if cursor != fin_off {
            return None;
        }
        let mut out = Vec::with_capacity(total_len);
        for off in ordered_offsets {
            out.extend_from_slice(&self.chunks[&off]);
        }
        self.chunks.clear();
        self.buffered_bytes = 0;
        Some(out)
    }
}

struct H3ServerPeer {
    peer_addr: SocketAddr,
    client_scid: Vec<u8>,
    app_read: Arc<QuicCipher>,
    app_write: Arc<QuicCipher>,
    next_pn: Arc<AtomicU64>,
    largest_pn: i64,
    streams: HashMap<u64, H3StreamReassembler>,
}

pub struct H3Server {
    pub id: u64,
    pub local_addr: SocketAddr,
    router: Arc<RwLock<RouterState>>,
    worker_ctx: Arc<RwLock<Option<WorkerEnvContext>>>,
    running: Arc<AtomicBool>,
    loop_handle: Mutex<Option<JoinHandle<()>>>,
    tls_server_cfg: Option<Arc<ServerConfig>>,
    requests_handled: Arc<AtomicU64>,
}

impl H3Server {
    pub(crate) fn bind_and_start(
        id: u64,
        addr: &str,
        router: RouterState,
        ctx: WorkerEnvContext,
        tls_cfg: Option<Arc<ServerConfig>>,
    ) -> Result<Self, String> {
        let sock = UdpSocket::bind(addr)
            .map_err(|e| format!("Failed to bind H3 UDP socket '{}': {}", addr, e))?;
        let local_addr = sock
            .local_addr()
            .map_err(|e| format!("Failed to get H3 local_addr: {}", e))?;
        let socket = Arc::new(sock);
        let router = Arc::new(RwLock::new(router));
        let worker_ctx = Arc::new(RwLock::new(Some(ctx)));
        let running = Arc::new(AtomicBool::new(true));
        let requests_handled = Arc::new(AtomicU64::new(0));

        let srv_sock = socket.clone();
        let srv_router = router.clone();
        let srv_ctx = worker_ctx.clone();
        let srv_running = running.clone();
        let srv_reqs = requests_handled.clone();
        let srv_tls = tls_cfg.clone();

        let jh = thread::spawn(move || {
            #[derive(Default)]
            struct CryptoReassembler {
                next_offset: u64,
                pending: std::collections::BTreeMap<u64, Vec<u8>>,
            }
            impl CryptoReassembler {
                fn push_and_drain(&mut self, offset: u64, data: &[u8]) -> Result<Vec<u8>, String> {
                    let end = offset
                        .checked_add(data.len() as u64)
                        .ok_or_else(|| "QUIC CRYPTO offset overflow".to_string())?;
                    if end > 1024 * 1024 {
                        return Err("QUIC CRYPTO buffer limit exceeded".to_string());
                    }
                    if !data.is_empty() && end > self.next_offset {
                        let trim = self.next_offset.saturating_sub(offset) as usize;
                        let start = offset.max(self.next_offset);
                        self.pending.insert(start, data[trim..].to_vec());
                    }
                    let mut out = Vec::new();
                    while let Some((&off, _)) = self.pending.first_key_value() {
                        if off > self.next_offset {
                            break;
                        }
                        let chunk = self.pending.remove(&off).unwrap();
                        let overlap = (self.next_offset - off) as usize;
                        if overlap < chunk.len() {
                            out.extend_from_slice(&chunk[overlap..]);
                            self.next_offset += (chunk.len() - overlap) as u64;
                        }
                    }
                    Ok(out)
                }
            }
            struct TlsHandshakeState {
                peer_addr: SocketAddr,
                client_scid: Vec<u8>,
                qconn: rustls::quic::ServerConnection,
                hs_read: Option<QuicCipher>,
                hs_write: Option<QuicCipher>,
                app_read: Option<Arc<QuicCipher>>,
                app_write: Option<Arc<QuicCipher>>,
                hs_pn_out: u64,
                hs_largest_pn: i64,
                hs_crypto_out_off: u64,
                hs_crypto: CryptoReassembler,
            }

            let mut peers_by_scid: HashMap<Vec<u8>, H3ServerPeer> = HashMap::new();
            let mut tls_handshakes: HashMap<Vec<u8>, TlsHandshakeState> = HashMap::new();
            let mut buf = [0u8; 65535];

            while srv_running.load(Ordering::Acquire) {
                let Ok((n, peer_addr)) = srv_sock.recv_from(&mut buf) else {
                    if !srv_running.load(Ordering::Acquire) {
                        break;
                    }
                    continue;
                };
                if !srv_running.load(Ordering::Acquire) {
                    break;
                }
                if n < 21 {
                    continue;
                }
                let pkt = &buf[..n];
                let is_long = (pkt[0] & 0x80) != 0;

                if is_long {
                    let Ok(hdr) = parse_quic_header_to_pn(pkt, 8) else {
                        continue;
                    };
                    if hdr.packet_type == QuicPacketType::Initial {
                        let Ok((seed, c_init, s_init)) = derive_initial_secrets(&hdr.dcid) else {
                            continue;
                        };
                        let c_cipher = QuicCipher::Ring(c_init);
                        let Ok((opened, _)) = open_quic_packet(pkt, hdr.dcid.len(), -1, &c_cipher)
                        else {
                            continue;
                        };

                        let mut server_scid = [0u8; 8];
                        fastrand::fill(&mut server_scid);
                        let server_scid_vec = server_scid.to_vec();

                        let mut hs_reply_frames = vec![QuicFrame::Ack {
                            largest: opened.pn,
                            delay: 0,
                            first_range: 0,
                            ranges: Vec::new(),
                            ecn: None,
                        }];

                        if let Some(ref cfg) = srv_tls {
                            let mut qconn = match rustls::quic::ServerConnection::new(
                                cfg.clone(),
                                rustls::quic::Version::V1,
                                Vec::new(),
                            ) {
                                Ok(c) => c,
                                Err(_) => continue,
                            };
                            let mut init_crypto = CryptoReassembler::default();
                            let mut hs_ok = true;
                            for f in &opened.frames {
                                if let QuicFrame::Crypto { offset, data } = f {
                                    match init_crypto.push_and_drain(*offset, data) {
                                        Ok(bytes) if !bytes.is_empty() => {
                                            if qconn.read_hs(&bytes).is_err() {
                                                hs_ok = false;
                                                break;
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(_) => {
                                            hs_ok = false;
                                            break;
                                        }
                                    }
                                }
                            }
                            if !hs_ok {
                                let s_cipher = QuicCipher::Ring(s_init);
                                if let Ok(close_pkt) = seal_quic_packet(
                                    QuicPacketType::Initial,
                                    &hdr.scid,
                                    &server_scid_vec,
                                    &[],
                                    0,
                                    None,
                                    &[QuicFrame::ConnectionClose {
                                        is_app: false,
                                        error_code: 0x10a,
                                        frame_type: None,
                                        reason: "TLS Initial handshake error".to_string(),
                                    }],
                                    &s_cipher,
                                    0,
                                ) {
                                    let _ = srv_sock.send_to(&close_pkt, peer_addr);
                                }
                                continue;
                            }

                            let mut state = TlsHandshakeState {
                                peer_addr,
                                client_scid: hdr.scid.clone(),
                                qconn,
                                hs_read: None,
                                hs_write: None,
                                app_read: None,
                                app_write: None,
                                hs_pn_out: 0,
                                hs_largest_pn: -1,
                                hs_crypto_out_off: 0,
                                hs_crypto: CryptoReassembler::default(),
                            };

                            let s_cipher = QuicCipher::Ring(s_init);
                            let mut init_pn_out = 0u64;
                            let mut sent_init_ack = false;
                            loop {
                                let mut hs_out = Vec::new();
                                let kc = state.qconn.write_hs(&mut hs_out);
                                if !hs_out.is_empty() {
                                    if let Some(ref hw) = state.hs_write {
                                        for chunk in hs_out.chunks(1150) {
                                            let off = state.hs_crypto_out_off;
                                            state.hs_crypto_out_off += chunk.len() as u64;
                                            if let Ok(hpkt) = seal_quic_packet(
                                                QuicPacketType::Handshake,
                                                &state.client_scid,
                                                &server_scid_vec,
                                                &[],
                                                state.hs_pn_out,
                                                None,
                                                &[QuicFrame::Crypto {
                                                    offset: off,
                                                    data: chunk.to_vec(),
                                                }],
                                                hw,
                                                0,
                                            ) {
                                                state.hs_pn_out += 1;
                                                let _ = srv_sock.send_to(&hpkt, peer_addr);
                                            }
                                        }
                                    } else {
                                        let mut frames = if !sent_init_ack {
                                            sent_init_ack = true;
                                            hs_reply_frames.clone()
                                        } else {
                                            Vec::new()
                                        };
                                        frames.push(QuicFrame::Crypto {
                                            offset: 0,
                                            data: hs_out,
                                        });
                                        if let Ok(ipkt) = seal_quic_packet(
                                            QuicPacketType::Initial,
                                            &state.client_scid,
                                            &server_scid_vec,
                                            &[],
                                            init_pn_out,
                                            None,
                                            &frames,
                                            &s_cipher,
                                            0,
                                        ) {
                                            init_pn_out += 1;
                                            let _ = srv_sock.send_to(&ipkt, peer_addr);
                                        }
                                    }
                                }
                                match kc {
                                    Some(rustls::quic::KeyChange::Handshake { keys }) => {
                                        state.hs_read = Some(QuicCipher::Rustls(keys.remote));
                                        state.hs_write = Some(QuicCipher::Rustls(keys.local));
                                    }
                                    Some(rustls::quic::KeyChange::OneRtt { keys, .. }) => {
                                        state.app_read = Some(Arc::new(QuicCipher::Rustls(keys.remote)));
                                        state.app_write = Some(Arc::new(QuicCipher::Rustls(keys.local)));
                                    }
                                    None => break,
                                }
                            }
                            if !sent_init_ack {
                                if let Ok(ipkt) = seal_quic_packet(
                                    QuicPacketType::Initial,
                                    &state.client_scid,
                                    &server_scid_vec,
                                    &[],
                                    init_pn_out,
                                    None,
                                    &hs_reply_frames,
                                    &s_cipher,
                                    0,
                                ) {
                                    let _ = srv_sock.send_to(&ipkt, peer_addr);
                                }
                            }
                            tls_handshakes.insert(server_scid_vec, state);
                        } else {
                            // Pure-ring HKDF 1-RTT key derivation
                            let Ok((c_app, s_app)) =
                                derive_hkdf_traffic_keys(&seed, &hdr.scid, &server_scid_vec)
                            else {
                                continue;
                            };

                            // Control stream SETTINGS frame inside CRYPTO handshake confirmation
                            let settings_frame = H3Frame::Settings(vec![
                                (0x01, 0),
                                (0x06, 65536),
                                (0x07, 0),
                            ])
                            .encode()
                            .unwrap_or_default();
                            let mut crypto_hello = Vec::new();
                            crypto_hello.push(server_scid_vec.len() as u8);
                            crypto_hello.extend_from_slice(&server_scid_vec);
                            crypto_hello.extend_from_slice(&settings_frame);
                            hs_reply_frames.push(QuicFrame::Crypto {
                                offset: 0,
                                data: crypto_hello,
                            });

                            let s_cipher = QuicCipher::Ring(s_init);
                            if let Ok(reply) = seal_quic_packet(
                                QuicPacketType::Initial,
                                &hdr.scid,
                                &server_scid_vec,
                                &[],
                                0,
                                None,
                                &hs_reply_frames,
                                &s_cipher,
                                0,
                            ) {
                                let _ = srv_sock.send_to(&reply, peer_addr);
                            }

                            peers_by_scid.insert(
                                server_scid_vec.clone(),
                                H3ServerPeer {
                                    peer_addr,
                                    client_scid: hdr.scid.clone(),
                                    app_read: Arc::new(QuicCipher::Ring(c_app)),
                                    app_write: Arc::new(QuicCipher::Ring(s_app)),
                                    next_pn: Arc::new(AtomicU64::new(0)),
                                    largest_pn: -1,
                                    streams: HashMap::new(),
                                },
                            );
                        }
                    } else if hdr.packet_type == QuicPacketType::Handshake {
                        let dcid_key = hdr.dcid.clone();
                        let Some(state) = tls_handshakes.get_mut(&dcid_key) else {
                            continue;
                        };
                        let Some(ref h_read) = state.hs_read else {
                            continue;
                        };
                        let Ok((opened, _)) =
                            open_quic_packet(pkt, hdr.dcid.len(), state.hs_largest_pn, h_read)
                        else {
                            continue;
                        };
                        if opened.pn as i64 > state.hs_largest_pn {
                            state.hs_largest_pn = opened.pn as i64;
                        }
                        state.peer_addr = peer_addr;
                        let mut hs_ok = true;
                        for f in &opened.frames {
                            if let QuicFrame::Crypto { offset, data } = f {
                                match state.hs_crypto.push_and_drain(*offset, data) {
                                    Ok(bytes) if !bytes.is_empty() => {
                                        if state.qconn.read_hs(&bytes).is_err() {
                                            hs_ok = false;
                                            break;
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(_) => {
                                        hs_ok = false;
                                        break;
                                    }
                                }
                            }
                        }
                        if !hs_ok {
                            tls_handshakes.remove(&dcid_key);
                            continue;
                        }
                        loop {
                            let mut hs_out = Vec::new();
                            let kc = state.qconn.write_hs(&mut hs_out);
                            if !hs_out.is_empty() {
                                if let Some(ref hw) = state.hs_write {
                                    for chunk in hs_out.chunks(1150) {
                                        let off = state.hs_crypto_out_off;
                                        state.hs_crypto_out_off += chunk.len() as u64;
                                        if let Ok(hpkt) = seal_quic_packet(
                                            QuicPacketType::Handshake,
                                            &state.client_scid,
                                            &dcid_key,
                                            &[],
                                            state.hs_pn_out,
                                            None,
                                            &[QuicFrame::Crypto {
                                                offset: off,
                                                data: chunk.to_vec(),
                                            }],
                                            hw,
                                            0,
                                        ) {
                                            state.hs_pn_out += 1;
                                            let _ = srv_sock.send_to(&hpkt, peer_addr);
                                        }
                                    }
                                }
                            }
                            match kc {
                                Some(rustls::quic::KeyChange::Handshake { keys }) => {
                                    state.hs_read = Some(QuicCipher::Rustls(keys.remote));
                                    state.hs_write = Some(QuicCipher::Rustls(keys.local));
                                }
                                Some(rustls::quic::KeyChange::OneRtt { keys, .. }) => {
                                    state.app_read = Some(Arc::new(QuicCipher::Rustls(keys.remote)));
                                    state.app_write = Some(Arc::new(QuicCipher::Rustls(keys.local)));
                                }
                                None => break,
                            }
                        }
                        if !state.qconn.is_handshaking() {
                            if let Some(done_state) = tls_handshakes.remove(&dcid_key) {
                                if let (Some(app_r), Some(app_w)) =
                                    (done_state.app_read, done_state.app_write)
                                {
                                    if let Ok(ack_pkt) = seal_quic_packet(
                                        QuicPacketType::OneRtt,
                                        &done_state.client_scid,
                                        &[],
                                        &[],
                                        0,
                                        None,
                                        &[QuicFrame::Ack {
                                            largest: opened.pn,
                                            delay: 0,
                                            first_range: 0,
                                            ranges: Vec::new(),
                                            ecn: None,
                                        }],
                                        &app_w,
                                        0,
                                    ) {
                                        let _ = srv_sock.send_to(&ack_pkt, done_state.peer_addr);
                                    }
                                    peers_by_scid.insert(
                                        dcid_key,
                                        H3ServerPeer {
                                            peer_addr: done_state.peer_addr,
                                            client_scid: done_state.client_scid,
                                            app_read: app_r,
                                            app_write: app_w,
                                            next_pn: Arc::new(AtomicU64::new(1)),
                                            largest_pn: -1,
                                            streams: HashMap::new(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                } else {
                    // Short Header (1-RTT): server_scid is 8 bytes at pkt[1..9]
                    if pkt.len() < 9 {
                        continue;
                    }
                    let dcid = &pkt[1..9];
                    let Some(peer) = peers_by_scid.get_mut(dcid) else {
                        continue;
                    };
                    let Ok((opened, _)) =
                        open_quic_packet(pkt, 8, peer.largest_pn, &peer.app_read)
                    else {
                        continue;
                    };
                    if opened.pn as i64 > peer.largest_pn {
                        peer.largest_pn = opened.pn as i64;
                    }
                    peer.peer_addr = peer_addr;

                    for frame in opened.frames {
                        match frame {
                            QuicFrame::Stream {
                                stream_id,
                                offset,
                                fin,
                                data,
                                ..
                            } => {
                                // Client-initiated bidirectional streams: stream_id % 4 == 0
                                if stream_id % 4 != 0 {
                                    continue;
                                }
                                let off = offset.unwrap_or(0);
                                let reasm = peer.streams.entry(stream_id).or_default();
                                if reasm.push(off, &data, fin).is_err() {
                                    peer.streams.remove(&stream_id);
                                    continue;
                                }
                                if let Some(stream_bytes) = reasm.take_if_complete() {
                                    peer.streams.remove(&stream_id);
                                    srv_reqs.fetch_add(1, Ordering::Relaxed);
                                    let r_snap = srv_router.read().unwrap().clone();
                                    let ctx_snap = srv_ctx
                                        .read()
                                        .unwrap()
                                        .clone()
                                        .unwrap_or(WorkerEnvContext {
                                            env_root: None,
                                            current_file: None,
                                            embedded_archive: None,
                                            embedded_bytecode: None,
                                        });
                                    let sock_out = srv_sock.clone();
                                    let c_scid = peer.client_scid.clone();
                                    let app_w = peer.app_write.clone();
                                    let pn_counter = peer.next_pn.clone();
                                    let target_addr = peer.peer_addr;

                                    crate::async_rt::global_pool().execute(move || {
                                        handle_h3_stream_request(
                                            stream_id,
                                            &stream_bytes,
                                            &r_snap,
                                            &ctx_snap,
                                            &sock_out,
                                            target_addr,
                                            &c_scid,
                                            &app_w,
                                            &pn_counter,
                                        );
                                    });
                                }
                            }
                            QuicFrame::ConnectionClose { .. } => {
                                peers_by_scid.remove(dcid);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        let server = Self {
            id,
            local_addr,
            router,
            worker_ctx,
            running,
            loop_handle: Mutex::new(Some(jh)),
            tls_server_cfg: tls_cfg,
            requests_handled,
        };
        Ok(server)
    }

    pub(crate) fn update_context(&self, router: RouterState, ctx: WorkerEnvContext) {
        *self.router.write().unwrap() = router;
        *self.worker_ctx.write().unwrap() = Some(ctx);
    }

    pub fn stop(&self) -> bool {
        if !self.running.swap(false, Ordering::SeqCst) {
            return false;
        }
        let wake = wake_addr_for(self.local_addr);
        if let Ok(s) = UdpSocket::bind("127.0.0.1:0") {
            let _ = s.send_to(b"\x00", wake);
        }
        if let Some(jh) = self.loop_handle.lock().unwrap().take() {
            let _ = jh.join();
        }
        true
    }

    pub fn stats_map(&self) -> Value {
        let mut m = IndexMap::new();
        m.insert("port".to_string(), Value::Int(self.local_addr.port() as i64));
        m.insert(
            "local_addr".to_string(),
            Value::String(self.local_addr.to_string()),
        );
        m.insert(
            "running".to_string(),
            Value::Bool(self.running.load(Ordering::Relaxed)),
        );
        m.insert(
            "requests".to_string(),
            Value::Int(self.requests_handled.load(Ordering::Relaxed) as i64),
        );
        m.insert(
            "tls_configured".to_string(),
            Value::Bool(self.tls_server_cfg.is_some()),
        );
        Value::Map(Arc::new(RwLock::new(m)))
    }
}

impl Drop for H3Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_h3_stream_request(
    stream_id: u64,
    stream_bytes: &[u8],
    router: &RouterState,
    ctx: &WorkerEnvContext,
    sock: &UdpSocket,
    peer_addr: SocketAddr,
    client_scid: &[u8],
    app_write: &QuicCipher,
    pn_counter: &AtomicU64,
) {
    let Ok(h3_frames) = H3Frame::decode_all(stream_bytes) else {
        return;
    };
    let mut method = "GET".to_string();
    let mut path = "/".to_string();
    let mut hdrs: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut body = Vec::new();

    for f in h3_frames {
        match f {
            H3Frame::Headers(qblock) => {
                if let Ok(decoded) = qpack_decode_headers(&qblock) {
                    for (k, v) in decoded {
                        if k == ":method" {
                            method = v;
                        } else if k == ":path" {
                            path = v;
                        } else if k == ":authority" {
                            hdrs.entry("host".to_string()).or_default().push(v);
                        } else if !k.starts_with(':') {
                            hdrs.entry(k).or_default().push(v);
                        }
                    }
                }
            }
            H3Frame::Data(d) => body.extend_from_slice(&d),
            _ => {}
        }
    }

    let empty_trailers = IndexMap::new();
    let resp = dispatch_shae_handler(
        router,
        ctx,
        &method,
        &path,
        &hdrs,
        &body,
        &empty_trailers,
        "HTTP/3",
        stream_id,
    );

    let mut resp_hdrs = vec![(":status".to_string(), resp.status.to_string())];
    for (k, vals) in &resp.headers {
        let lk = k.to_ascii_lowercase();
        for v in vals {
            resp_hdrs.push((lk.clone(), v.clone()));
        }
    }
    let qblock = qpack_encode_headers(&resp_hdrs, true);
    let mut payload = H3Frame::Headers(qblock).encode().unwrap_or_default();
    if !resp.body.is_empty() {
        payload.extend_from_slice(&H3Frame::Data(resp.body).encode().unwrap_or_default());
    }

    // Fragment across MTU-friendly QUIC 1-RTT STREAM packets if needed
    const CHUNK_SIZE: usize = 1150;
    let mut off = 0usize;
    if payload.is_empty() {
        let pn = pn_counter.fetch_add(1, Ordering::SeqCst);
        if let Ok(pkt) = seal_quic_packet(
            QuicPacketType::OneRtt,
            client_scid,
            &[],
            &[],
            pn,
            None,
            &[QuicFrame::Stream {
                stream_id,
                offset: Some(0),
                fin: true,
                has_len: true,
                data: Vec::new(),
            }],
            app_write,
            0,
        ) {
            let _ = sock.send_to(&pkt, peer_addr);
        }
        return;
    }

    while off < payload.len() {
        let end = (off + CHUNK_SIZE).min(payload.len());
        let fin = end == payload.len();
        let pn = pn_counter.fetch_add(1, Ordering::SeqCst);
        let sf = QuicFrame::Stream {
            stream_id,
            offset: Some(off as u64),
            fin,
            has_len: true,
            data: payload[off..end].to_vec(),
        };
        if let Ok(pkt) = seal_quic_packet(
            QuicPacketType::OneRtt,
            client_scid,
            &[],
            &[],
            pn,
            None,
            &[sf],
            app_write,
            0,
        ) {
            let _ = sock.send_to(&pkt, peer_addr);
        }
        off = end;
    }
}

pub struct H3Client {
    pub id: u64,
    socket: UdpSocket,
    pub authority: String,
    client_scid: Vec<u8>,
    server_scid: Vec<u8>,
    app_write: QuicCipher,
    app_read: QuicCipher,
    next_stream_id: AtomicU64,
    next_pn: AtomicU64,
    largest_pn: Mutex<i64>,
    requests_sent: AtomicU64,
    responses_recv: AtomicU64,
    io_lock: Mutex<()>,
}

impl H3Client {
    pub fn connect(addr_or_url: &str) -> Result<Self, String> {
        Self::connect_with_opts(addr_or_url, false, false, None, None)
    }

    pub fn connect_with_opts(
        addr_or_url: &str,
        use_tls: bool,
        insecure: bool,
        ca_pem: Option<&str>,
        server_name: Option<&str>,
    ) -> Result<Self, String> {
        use std::net::ToSocketAddrs;
        let parsed = parse_url(addr_or_url)?;
        let target = format_socket_addr(&parsed.host, parsed.port);
        let addrs: Vec<SocketAddr> = target
            .to_socket_addrs()
            .map_err(|e| format!("H3 DNS resolution failed for '{}': {}", target, e))?
            .collect();
        let remote_addr = addrs
            .iter()
            .find(|a| a.is_ipv4())
            .copied()
            .or_else(|| addrs.first().copied())
            .ok_or_else(|| format!("No socket address resolved for '{}'", target))?;
        let bind_addr = if remote_addr.is_ipv6() {
            "[::]:0"
        } else {
            "127.0.0.1:0"
        };
        let sock = UdpSocket::bind(bind_addr)
            .map_err(|e| format!("Failed to bind H3 client UDP socket: {}", e))?;
        sock.connect(remote_addr)
            .map_err(|e| format!("Failed to connect H3 UDP socket to '{}': {}", target, e))?;
        sock.set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set_read_timeout failed: {}", e))?;

        let mut client_dcid = [0u8; 8];
        let mut client_scid = [0u8; 8];
        fastrand::fill(&mut client_dcid);
        fastrand::fill(&mut client_scid);

        let (seed, c_init, s_init) = derive_initial_secrets(&client_dcid)?;
        let c_init_cipher = QuicCipher::Ring(c_init);
        let s_init_cipher = QuicCipher::Ring(s_init);

        if use_tls {
            #[derive(Default)]
            struct ClientCryptoReasm {
                next_offset: u64,
                pending: std::collections::BTreeMap<u64, Vec<u8>>,
            }
            impl ClientCryptoReasm {
                fn push_and_drain(&mut self, offset: u64, data: &[u8]) -> Result<Vec<u8>, String> {
                    let end = offset
                        .checked_add(data.len() as u64)
                        .ok_or_else(|| "QUIC CRYPTO offset overflow".to_string())?;
                    if end > 1024 * 1024 {
                        return Err("QUIC CRYPTO buffer limit exceeded".to_string());
                    }
                    if !data.is_empty() && end > self.next_offset {
                        let trim = self.next_offset.saturating_sub(offset) as usize;
                        let start = offset.max(self.next_offset);
                        self.pending.insert(start, data[trim..].to_vec());
                    }
                    let mut out = Vec::new();
                    while let Some((&off, _)) = self.pending.first_key_value() {
                        if off > self.next_offset {
                            break;
                        }
                        let chunk = self.pending.remove(&off).unwrap();
                        let overlap = (self.next_offset - off) as usize;
                        if overlap < chunk.len() {
                            out.extend_from_slice(&chunk[overlap..]);
                            self.next_offset += (chunk.len() - overlap) as u64;
                        }
                    }
                    Ok(out)
                }
            }

            let cfg = build_tls_client_config(ca_pem, insecure, vec![b"h3".to_vec()])?;
            let sni_host = server_name.unwrap_or(&parsed.host);
            let sni = ServerName::try_from(sni_host.to_string())
                .map_err(|_| format!("Invalid QUIC TLS server name '{}'", sni_host))?;
            let mut qconn = rustls::quic::ClientConnection::new(
                cfg,
                rustls::quic::Version::V1,
                sni,
                Vec::new(),
            )
            .map_err(|e| format!("rustls::quic::ClientConnection init failed: {}", e))?;

            let mut ch_buf = Vec::new();
            let _ = qconn.write_hs(&mut ch_buf);
            let init_pkt = seal_quic_packet(
                QuicPacketType::Initial,
                &client_dcid,
                &client_scid,
                &[],
                0,
                None,
                &[QuicFrame::Crypto {
                    offset: 0,
                    data: ch_buf,
                }],
                &c_init_cipher,
                1200,
            )?;
            sock.send(&init_pkt)
                .map_err(|e| format!("Failed sending QUIC TLS Initial: {}", e))?;

            let mut server_scid: Option<Vec<u8>> = None;
            let mut hs_read: Option<QuicCipher> = None;
            let mut hs_write: Option<QuicCipher> = None;
            let mut app_read: Option<QuicCipher> = None;
            let mut app_write: Option<QuicCipher> = None;
            let mut hs_pn_out = 0u64;
            let mut hs_crypto_out_off = 0u64;
            let mut hs_largest_pn = -1i64;
            let mut app_largest_pn = -1i64;
            let mut init_crypto = ClientCryptoReasm::default();
            let mut hs_crypto = ClientCryptoReasm::default();
            let mut sent_finished = false;
            let mut buf = [0u8; 65535];
            let start = Instant::now();

            loop {
                if start.elapsed() > Duration::from_secs(5) {
                    return Err("QUIC TLS 1.3 handshake timed out".to_string());
                }
                let n = sock
                    .recv(&mut buf)
                    .map_err(|e| format!("QUIC TLS handshake recv failed: {}", e))?;
                let hdr = parse_quic_header_to_pn(&buf[..n], client_scid.len())?;

                match hdr.packet_type {
                    QuicPacketType::Initial => {
                        let (opened, _) =
                            open_quic_packet(&buf[..n], client_scid.len(), -1, &s_init_cipher)?;
                        if server_scid.is_none() && !opened.header.scid.is_empty() {
                            server_scid = Some(opened.header.scid.clone());
                        }
                        for f in &opened.frames {
                            match f {
                                QuicFrame::ConnectionClose { reason, .. } => {
                                    return Err(format!("QUIC TLS handshake rejected: {}", reason));
                                }
                                QuicFrame::Crypto { offset, data } => {
                                    let bytes = init_crypto.push_and_drain(*offset, data)?;
                                    if !bytes.is_empty() {
                                        qconn
                                            .read_hs(&bytes)
                                            .map_err(|e| format!("QUIC TLS Initial read_hs failed: {}", e))?;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    QuicPacketType::Handshake => {
                        let Some(ref h_read) = hs_read else {
                            continue;
                        };
                        let (opened, _) =
                            open_quic_packet(&buf[..n], client_scid.len(), hs_largest_pn, h_read)?;
                        if opened.pn as i64 > hs_largest_pn {
                            hs_largest_pn = opened.pn as i64;
                        }
                        if server_scid.is_none() && !opened.header.scid.is_empty() {
                            server_scid = Some(opened.header.scid.clone());
                        }
                        for f in &opened.frames {
                            match f {
                                QuicFrame::ConnectionClose { reason, .. } => {
                                    return Err(format!("QUIC TLS handshake closed: {}", reason));
                                }
                                QuicFrame::Crypto { offset, data } => {
                                    let bytes = hs_crypto.push_and_drain(*offset, data)?;
                                    if !bytes.is_empty() {
                                        qconn
                                            .read_hs(&bytes)
                                            .map_err(|e| format!("QUIC TLS Handshake read_hs failed: {}", e))?;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    QuicPacketType::OneRtt => {
                        if sent_finished {
                            if let Some(ref a_read) = app_read {
                                let (opened, _) = open_quic_packet(
                                    &buf[..n],
                                    client_scid.len(),
                                    app_largest_pn,
                                    a_read,
                                )?;
                                if opened.pn as i64 > app_largest_pn {
                                    app_largest_pn = opened.pn as i64;
                                }
                                break;
                            }
                        }
                        continue;
                    }
                    _ => continue,
                }

                loop {
                    let mut hs_out = Vec::new();
                    let kc = qconn.write_hs(&mut hs_out);
                    if !hs_out.is_empty() {
                        if let (Some(hw), Some(scid)) = (&hs_write, &server_scid) {
                            for chunk in hs_out.chunks(1150) {
                                let off = hs_crypto_out_off;
                                hs_crypto_out_off += chunk.len() as u64;
                                let hpkt = seal_quic_packet(
                                    QuicPacketType::Handshake,
                                    scid,
                                    &client_scid,
                                    &[],
                                    hs_pn_out,
                                    None,
                                    &[QuicFrame::Crypto {
                                        offset: off,
                                        data: chunk.to_vec(),
                                    }],
                                    hw,
                                    0,
                                )?;
                                hs_pn_out += 1;
                                sock.send(&hpkt)
                                    .map_err(|e| format!("Failed sending QUIC TLS Handshake: {}", e))?;
                            }
                        }
                    }
                    match kc {
                        Some(rustls::quic::KeyChange::Handshake { keys }) => {
                            hs_read = Some(QuicCipher::Rustls(keys.remote));
                            hs_write = Some(QuicCipher::Rustls(keys.local));
                        }
                        Some(rustls::quic::KeyChange::OneRtt { keys, .. }) => {
                            app_read = Some(QuicCipher::Rustls(keys.remote));
                            app_write = Some(QuicCipher::Rustls(keys.local));
                            sent_finished = true;
                        }
                        None => break,
                    }
                }
            }

            let server_scid =
                server_scid.ok_or_else(|| "Server did not return SCID during QUIC TLS handshake".to_string())?;
            let app_write =
                app_write.ok_or_else(|| "QUIC TLS handshake missing 1-RTT write keys".to_string())?;
            let app_read =
                app_read.ok_or_else(|| "QUIC TLS handshake missing 1-RTT read keys".to_string())?;

            return Ok(Self {
                id: next_handle_id(),
                socket: sock,
                authority: parsed.authority,
                client_scid: client_scid.to_vec(),
                server_scid,
                app_write,
                app_read,
                next_stream_id: AtomicU64::new(0),
                next_pn: AtomicU64::new(1),
                largest_pn: Mutex::new(app_largest_pn),
                requests_sent: AtomicU64::new(0),
                responses_recv: AtomicU64::new(0),
                io_lock: Mutex::new(()),
            });
        }

        let settings = H3Frame::Settings(vec![(0x01, 0), (0x06, 65536), (0x07, 0)]).encode()?;
        let init_pkt = seal_quic_packet(
            QuicPacketType::Initial,
            &client_dcid,
            &client_scid,
            &[],
            0,
            None,
            &[QuicFrame::Crypto {
                offset: 0,
                data: settings,
            }],
            &c_init_cipher,
            1200,
        )?;
        sock.send(&init_pkt)
            .map_err(|e| format!("Failed sending QUIC Initial: {}", e))?;

        let mut buf = [0u8; 65535];
        let n = sock
            .recv(&mut buf)
            .map_err(|e| format!("QUIC Initial handshake recv failed: {}", e))?;
        let (opened, _) = open_quic_packet(&buf[..n], client_scid.len(), -1, &s_init_cipher)?;

        let mut server_scid = opened.header.scid.clone();
        for f in &opened.frames {
            match f {
                QuicFrame::ConnectionClose { reason, .. } => {
                    return Err(format!("Server closed QUIC connection: {}", reason));
                }
                QuicFrame::Crypto { data, .. } => {
                    if !data.is_empty() {
                        let slen = data[0] as usize;
                        if slen <= 20 && 1 + slen <= data.len() {
                            server_scid = data[1..1 + slen].to_vec();
                        }
                    }
                }
                _ => {}
            }
        }
        if server_scid.is_empty() {
            return Err("Server did not return a valid SCID in QUIC Initial".to_string());
        }

        let (c_app, s_app) = derive_hkdf_traffic_keys(&seed, &client_scid, &server_scid)?;

        Ok(Self {
            id: next_handle_id(),
            socket: sock,
            authority: parsed.authority,
            client_scid: client_scid.to_vec(),
            server_scid,
            app_write: QuicCipher::Ring(c_app),
            app_read: QuicCipher::Ring(s_app),
            next_stream_id: AtomicU64::new(0),
            next_pn: AtomicU64::new(1),
            largest_pn: Mutex::new(-1),
            requests_sent: AtomicU64::new(0),
            responses_recv: AtomicU64::new(0),
            io_lock: Mutex::new(()),
        })
    }

    fn send_stream_request(
        &self,
        stream_id: u64,
        method: &str,
        path: &str,
        headers: &IndexMap<String, Vec<String>>,
        body: &[u8],
    ) -> Result<(), String> {
        let mut hdr_list = vec![
            (":method".to_string(), method.to_ascii_uppercase()),
            (
                ":path".to_string(),
                if path.is_empty() {
                    "/".to_string()
                } else {
                    path.to_string()
                },
            ),
            (":scheme".to_string(), "https".to_string()),
            (":authority".to_string(), self.authority.clone()),
        ];
        for (k, vals) in headers {
            let lk = k.to_ascii_lowercase();
            if lk == "connection" || lk == "host" || lk == "transfer-encoding" {
                continue;
            }
            for v in vals {
                hdr_list.push((lk.clone(), v.clone()));
            }
        }

        let qblock = qpack_encode_headers(&hdr_list, true);
        let mut h3_bytes = H3Frame::Headers(qblock).encode()?;
        if !body.is_empty() {
            h3_bytes.extend_from_slice(&H3Frame::Data(body.to_vec()).encode()?);
        }

        const CHUNK: usize = 1150;
        let mut off = 0usize;
        while off < h3_bytes.len() {
            let end = (off + CHUNK).min(h3_bytes.len());
            let fin = end == h3_bytes.len();
            let pn = self.next_pn.fetch_add(1, Ordering::SeqCst);
            let pkt = seal_quic_packet(
                QuicPacketType::OneRtt,
                &self.server_scid,
                &[],
                &[],
                pn,
                None,
                &[QuicFrame::Stream {
                    stream_id,
                    offset: Some(off as u64),
                    fin,
                    has_len: true,
                    data: h3_bytes[off..end].to_vec(),
                }],
                &self.app_write,
                0,
            )?;
            self.socket
                .send(&pkt)
                .map_err(|e| format!("H3 UDP send failed: {}", e))?;
            off = end;
        }
        self.requests_sent.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        headers: &IndexMap<String, Vec<String>>,
        body: &[u8],
    ) -> Result<(u16, IndexMap<String, Vec<String>>, Vec<u8>, u64), String> {
        let _guard = self.io_lock.lock().unwrap();
        let sid = self.next_stream_id.fetch_add(4, Ordering::SeqCst);
        self.send_stream_request(sid, method, path, headers, body)?;

        let mut reasm = H3StreamReassembler::default();
        let mut buf = [0u8; 65535];
        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("HTTP/3 response timed out".to_string());
            }
            let n = self
                .socket
                .recv(&mut buf)
                .map_err(|e| format!("H3 UDP recv failed: {}", e))?;
            let lp = *self.largest_pn.lock().unwrap();
            let Ok((opened, _)) = open_quic_packet(&buf[..n], self.client_scid.len(), lp, &self.app_read) else {
                continue;
            };
            {
                let mut lp_g = self.largest_pn.lock().unwrap();
                if opened.pn as i64 > *lp_g {
                    *lp_g = opened.pn as i64;
                }
            }
            for f in opened.frames {
                if let QuicFrame::Stream {
                    stream_id,
                    offset,
                    fin,
                    data,
                    ..
                } = f
                {
                    if stream_id == sid {
                        reasm.push(offset.unwrap_or(0), &data, fin)?;
                        if let Some(bytes) = reasm.take_if_complete() {
                            self.responses_recv.fetch_add(1, Ordering::Relaxed);
                            return decode_h3_response_stream(sid, &bytes);
                        }
                    }
                }
            }
        }
    }

    pub fn multiplex(
        &self,
        reqs: &[(String, String, IndexMap<String, Vec<String>>, Vec<u8>)],
    ) -> Result<Vec<(u16, IndexMap<String, Vec<String>>, Vec<u8>, u64)>, String> {
        let _guard = self.io_lock.lock().unwrap();
        let mut sids = Vec::with_capacity(reqs.len());
        let mut reasms: HashMap<u64, H3StreamReassembler> = HashMap::new();
        let mut done: HashMap<u64, (u16, IndexMap<String, Vec<String>>, Vec<u8>, u64)> =
            HashMap::new();

        for (m, p, h, b) in reqs {
            let sid = self.next_stream_id.fetch_add(4, Ordering::SeqCst);
            self.send_stream_request(sid, m, p, h, b)?;
            sids.push(sid);
            reasms.insert(sid, H3StreamReassembler::default());
        }

        let mut buf = [0u8; 65535];
        let start = Instant::now();
        while done.len() < sids.len() {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("HTTP/3 multiplex timed out".to_string());
            }
            let n = self
                .socket
                .recv(&mut buf)
                .map_err(|e| format!("H3 multiplex recv failed: {}", e))?;
            let lp = *self.largest_pn.lock().unwrap();
            let Ok((opened, _)) = open_quic_packet(&buf[..n], self.client_scid.len(), lp, &self.app_read) else {
                continue;
            };
            {
                let mut lp_g = self.largest_pn.lock().unwrap();
                if opened.pn as i64 > *lp_g {
                    *lp_g = opened.pn as i64;
                }
            }
            for f in opened.frames {
                if let QuicFrame::Stream {
                    stream_id,
                    offset,
                    fin,
                    data,
                    ..
                } = f
                {
                    if let Some(r) = reasms.get_mut(&stream_id) {
                        r.push(offset.unwrap_or(0), &data, fin)?;
                        if let Some(bytes) = r.take_if_complete() {
                            reasms.remove(&stream_id);
                            self.responses_recv.fetch_add(1, Ordering::Relaxed);
                            done.insert(stream_id, decode_h3_response_stream(stream_id, &bytes)?);
                        }
                    }
                }
            }
        }

        Ok(sids.into_iter().map(|sid| done.remove(&sid).unwrap()).collect())
    }

    pub fn close(&self) {
        let pn = self.next_pn.fetch_add(1, Ordering::SeqCst);
        if let Ok(pkt) = seal_quic_packet(
            QuicPacketType::OneRtt,
            &self.server_scid,
            &[],
            &[],
            pn,
            None,
            &[QuicFrame::ConnectionClose {
                is_app: true,
                error_code: 0x100,
                frame_type: None,
                reason: "H3_NO_ERROR".to_string(),
            }],
            &self.app_write,
            0,
        ) {
            let _ = self.socket.send(&pkt);
        }
    }

    pub fn stats_map(&self) -> Value {
        let mut m = IndexMap::new();
        m.insert(
            "requests_sent".to_string(),
            Value::Int(self.requests_sent.load(Ordering::Relaxed) as i64),
        );
        m.insert(
            "responses_recv".to_string(),
            Value::Int(self.responses_recv.load(Ordering::Relaxed) as i64),
        );
        m.insert("authority".to_string(), Value::String(self.authority.clone()));
        Value::Map(Arc::new(RwLock::new(m)))
    }
}

fn decode_h3_response_stream(
    stream_id: u64,
    bytes: &[u8],
) -> Result<(u16, IndexMap<String, Vec<String>>, Vec<u8>, u64), String> {
    let frames = H3Frame::decode_all(bytes)?;
    let mut status: u16 = 200;
    let mut hdrs: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut body = Vec::new();
    for f in frames {
        match f {
            H3Frame::Headers(qblock) => {
                let decoded = qpack_decode_headers(&qblock)?;
                for (k, v) in decoded {
                    if k == ":status" {
                        status = v.parse::<u16>().unwrap_or(200);
                    } else if !k.starts_with(':') {
                        hdrs.entry(k).or_default().push(v);
                    }
                }
            }
            H3Frame::Data(d) => body.extend_from_slice(&d),
            _ => {}
        }
    }
    Ok((status, hdrs, body, stream_id))
}

// ============================================================================
// Part 4: Registries, BoundMethod Dispatchers, and Shae Module Exports
// ============================================================================

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);
static HTTP_SERVERS: OnceLock<RwLock<HashMap<u64, Arc<HttpServerHandle>>>> = OnceLock::new();
static HTTP_POOLS: OnceLock<RwLock<HashMap<u64, Arc<HttpClientPool>>>> = OnceLock::new();
static H2_CONNS: OnceLock<RwLock<HashMap<u64, Arc<H2ClientConn>>>> = OnceLock::new();
static H3_SERVERS: OnceLock<RwLock<HashMap<u64, Arc<H3ServerSlot>>>> = OnceLock::new();
static H3_CLIENTS: OnceLock<RwLock<HashMap<u64, Arc<H3Client>>>> = OnceLock::new();
static DEFAULT_POOL: OnceLock<Arc<HttpClientPool>> = OnceLock::new();

fn next_handle_id() -> u64 {
    NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst)
}

fn http_servers() -> &'static RwLock<HashMap<u64, Arc<HttpServerHandle>>> {
    HTTP_SERVERS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn http_pools() -> &'static RwLock<HashMap<u64, Arc<HttpClientPool>>> {
    HTTP_POOLS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn h2_conns() -> &'static RwLock<HashMap<u64, Arc<H2ClientConn>>> {
    H2_CONNS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn h3_servers() -> &'static RwLock<HashMap<u64, Arc<H3ServerSlot>>> {
    H3_SERVERS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn h3_clients() -> &'static RwLock<HashMap<u64, Arc<H3Client>>> {
    H3_CLIENTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn default_pool() -> &'static Arc<HttpClientPool> {
    DEFAULT_POOL.get_or_init(|| Arc::new(HttpClientPool::new(0)))
}

struct H3ServerSlot {
    id: u64,
    router: Arc<RwLock<RouterState>>,
    worker_ctx: Arc<RwLock<Option<WorkerEnvContext>>>,
    running_srv: Mutex<Option<H3Server>>,
}

impl H3ServerSlot {
    fn new(id: u64) -> Self {
        Self {
            id,
            router: Arc::new(RwLock::new(RouterState::default())),
            worker_ctx: Arc::new(RwLock::new(None)),
            running_srv: Mutex::new(None),
        }
    }

    fn bind_and_start(
        &self,
        addr: &str,
        ctx: WorkerEnvContext,
        tls_cfg: Option<Arc<ServerConfig>>,
    ) -> Result<SocketAddr, String> {
        let mut guard = self.running_srv.lock().unwrap();
        if guard.is_some() {
            return Err("HTTP/3 server is already running".to_string());
        }
        *self.worker_ctx.write().unwrap() = Some(ctx.clone());
        let router_snap = self.router.read().unwrap().clone();
        let srv = H3Server::bind_and_start(self.id, addr, router_snap, ctx, tls_cfg)?;
        let local = srv.local_addr;
        *guard = Some(srv);
        Ok(local)
    }

    fn stop(&self) -> bool {
        if let Some(srv) = self.running_srv.lock().unwrap().take() {
            srv.stop()
        } else {
            false
        }
    }

    fn port(&self) -> u16 {
        self.running_srv
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.local_addr.port())
            .unwrap_or(0)
    }

    fn local_addr_str(&self) -> String {
        self.running_srv
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.local_addr.to_string())
            .unwrap_or_default()
    }

    fn stats_map(&self) -> Value {
        if let Some(ref srv) = *self.running_srv.lock().unwrap() {
            srv.stats_map()
        } else {
            let mut m = IndexMap::new();
            m.insert("running".to_string(), Value::Bool(false));
            m.insert("port".to_string(), Value::Int(0));
            Value::Map(Arc::new(RwLock::new(m)))
        }
    }
}

fn make_http_server_value(srv: &HttpServerHandle) -> Value {
    let mut m = IndexMap::new();
    m.insert("_http_server_id".to_string(), Value::Int(srv.id as i64));
    m.insert("id".to_string(), Value::Int(srv.id as i64));
    m.insert("port".to_string(), Value::Int(srv.port() as i64));
    let addr = srv.local_addr_str();
    m.insert("addr".to_string(), Value::String(addr.clone()));
    m.insert("local_addr".to_string(), Value::String(addr.clone()));
    m.insert("localAddr".to_string(), Value::String(addr));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_http_pool_value(pool: &HttpClientPool) -> Value {
    let mut m = IndexMap::new();
    m.insert("_http_pool_id".to_string(), Value::Int(pool.id as i64));
    m.insert("id".to_string(), Value::Int(pool.id as i64));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_h2_conn_value(conn: &H2ClientConn) -> Value {
    let mut m = IndexMap::new();
    m.insert("_h2_conn_id".to_string(), Value::Int(conn.id as i64));
    m.insert("id".to_string(), Value::Int(conn.id as i64));
    m.insert("authority".to_string(), Value::String(conn.authority.clone()));
    m.insert(
        "alpn".to_string(),
        conn.negotiated_alpn
            .as_ref()
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null),
    );
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_h3_server_value(slot: &H3ServerSlot) -> Value {
    let mut m = IndexMap::new();
    m.insert("_h3_server_id".to_string(), Value::Int(slot.id as i64));
    m.insert("id".to_string(), Value::Int(slot.id as i64));
    m.insert("port".to_string(), Value::Int(slot.port() as i64));
    let addr = slot.local_addr_str();
    m.insert("addr".to_string(), Value::String(addr.clone()));
    m.insert("local_addr".to_string(), Value::String(addr.clone()));
    m.insert("localAddr".to_string(), Value::String(addr));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_h3_client_value(client: &H3Client) -> Value {
    let mut m = IndexMap::new();
    m.insert("_h3_client_id".to_string(), Value::Int(client.id as i64));
    m.insert("id".to_string(), Value::Int(client.id as i64));
    m.insert(
        "authority".to_string(),
        Value::String(client.authority.clone()),
    );
    Value::Map(Arc::new(RwLock::new(m)))
}

fn get_map_id(
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    key: &str,
    span: Span,
) -> Result<u64, RuntimeError> {
    let guard = map.read().unwrap();
    match guard.get(key) {
        Some(Value::Int(id)) if *id >= 0 => Ok(*id as u64),
        _ => Err(RuntimeError::new(format!("Missing valid '{}' handle ID", key)).at(span)),
    }
}

fn apply_handler_or_routes(router: &mut RouterState, handler_val: &Value, span: Span) -> Result<(), RuntimeError> {
    if handler_val.is_callable() {
        router.fallback = Some(handler_val.clone());
        return Ok(());
    }
    if let Value::Map(m) = handler_val {
        let guard = m.read().unwrap();
        for (k, v) in guard.iter() {
            if !v.is_callable() {
                return Err(RuntimeError::new(format!(
                    "Route '{}' handler must be callable, got {}",
                    k,
                    v.type_name()
                ))
                .at(span));
            }
            if let Some(space) = k.find(' ') {
                let method = k[..space].trim().to_ascii_uppercase();
                let path = k[space + 1..].trim().to_string();
                router.routes.push(RouteEntry {
                    method,
                    pattern: path,
                    handler: v.clone(),
                });
            } else {
                router.routes.push(RouteEntry {
                    method: "*".to_string(),
                    pattern: k.clone(),
                    handler: v.clone(),
                });
            }
        }
        return Ok(());
    }
    Err(RuntimeError::new(format!(
        "Handler must be a function or route map, got {}",
        handler_val.type_name()
    ))
    .at(span))
}

fn extract_request_opts(
    method_default: &str,
    args: &[Value],
    span: Span,
) -> Result<(String, String, IndexMap<String, Vec<String>>, Vec<u8>, bool, bool), RuntimeError> {
    // Supports:
    // - request({ method, url/path, headers, body, json, retry, insecure })
    // - get(url, [opts]) / head(url, [opts]) / delete(url, [opts])
    // - post(url, body, [opts]) / put(url, body, [opts]) / patch(url, body, [opts])
    if args.is_empty() {
        return Err(RuntimeError::new("Expected URL or request options map".into()).at(span));
    }

    let mut method = method_default.to_string();
    let mut url = String::new();
    let mut headers = IndexMap::new();
    let mut body = Vec::new();
    let mut retry = false;
    let mut insecure = false;

    if let Value::Map(m) = &args[0] {
        let g = m.read().unwrap();
        if let Some(v) = g.get("method") {
            method = v.to_display().to_ascii_uppercase();
        }
        if let Some(v) = g.get("url").or_else(|| g.get("path")) {
            url = v.to_display();
        }
        if let Some(v) = g.get("headers") {
            headers = extract_headers_from_shae_map(v);
        }
        if let Some(v) = g.get("bytes").or_else(|| g.get("body")) {
            body = value_to_bytes(v).map_err(|e| RuntimeError::new(e).at(span))?;
        } else if let Some(v) = g.get("json") {
            if !headers.contains_key("content-type") {
                headers.insert(
                    "content-type".to_string(),
                    vec!["application/json".to_string()],
                );
            }
            body = serde_json::to_vec(&shae_value_to_json(v)).unwrap_or_default();
        }
        if let Some(Value::Bool(b)) = g.get("retry").or_else(|| g.get("retry_on_stale")) {
            retry = *b;
        }
        if let Some(Value::Bool(b)) = g.get("insecure") {
            insecure = *b;
        }
        return Ok((method, url, headers, body, retry, insecure));
    }

    url = args[0].to_display();
    let opts_idx = if matches!(method_default, "POST" | "PUT" | "PATCH") {
        if let Some(bval) = args.get(1) {
            if let Value::Map(m) = bval {
                let g = m.read().unwrap();
                if g.contains_key("headers") || g.contains_key("body") || g.contains_key("json") {
                    if let Some(v) = g.get("headers") {
                        headers = extract_headers_from_shae_map(v);
                    }
                    if let Some(v) = g.get("bytes").or_else(|| g.get("body")) {
                        body = value_to_bytes(v).map_err(|e| RuntimeError::new(e).at(span))?;
                    } else if let Some(v) = g.get("json") {
                        if !headers.contains_key("content-type") {
                            headers.insert(
                                "content-type".to_string(),
                                vec!["application/json".to_string()],
                            );
                        }
                        body = serde_json::to_vec(&shae_value_to_json(v)).unwrap_or_default();
                    }
                    if let Some(Value::Bool(b)) = g.get("insecure") {
                        insecure = *b;
                    }
                    2
                } else {
                    if !headers.contains_key("content-type") {
                        headers.insert(
                            "content-type".to_string(),
                            vec!["application/json".to_string()],
                        );
                    }
                    body = serde_json::to_vec(&shae_value_to_json(bval)).unwrap_or_default();
                    2
                }
            } else {
                body = value_to_bytes(bval).map_err(|e| RuntimeError::new(e).at(span))?;
                2
            }
        } else {
            1
        }
    } else {
        1
    };

    if let Some(Value::Map(m)) = args.get(opts_idx) {
        let g = m.read().unwrap();
        if let Some(v) = g.get("headers") {
            for (k, vs) in extract_headers_from_shae_map(v) {
                headers.insert(k, vs);
            }
        }
        if let Some(v) = g.get("bytes").or_else(|| g.get("body")) {
            body = value_to_bytes(v).map_err(|e| RuntimeError::new(e).at(span))?;
        } else if let Some(v) = g.get("json") {
            if !headers.contains_key("content-type") {
                headers.insert(
                    "content-type".to_string(),
                    vec!["application/json".to_string()],
                );
            }
            body = serde_json::to_vec(&shae_value_to_json(v)).unwrap_or_default();
        }
        if let Some(Value::Bool(b)) = g.get("retry").or_else(|| g.get("retry_on_stale")) {
            retry = *b;
        }
        if let Some(Value::Bool(b)) = g.get("insecure") {
            insecure = *b;
        }
    }

    Ok((method, url, headers, body, retry, insecure))
}

fn extract_batch_requests(
    val: &Value,
    span: Span,
) -> Result<Vec<(String, String, IndexMap<String, Vec<String>>, Vec<u8>)>, RuntimeError> {
    let Value::Array(arr) = val else {
        return Err(RuntimeError::new("Expected an array of request objects".into()).at(span));
    };
    let items = arr.read().unwrap();
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        match item {
            Value::String(path) => {
                out.push(("GET".to_string(), path.clone(), IndexMap::new(), Vec::new()));
            }
            Value::Map(m) => {
                let g = m.read().unwrap();
                let method = g
                    .get("method")
                    .map(|v| v.to_display().to_ascii_uppercase())
                    .unwrap_or_else(|| "GET".to_string());
                let path = g
                    .get("path")
                    .or_else(|| g.get("url"))
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "/".to_string());
                let mut headers = g
                    .get("headers")
                    .map(extract_headers_from_shae_map)
                    .unwrap_or_default();
                let body = if let Some(v) = g.get("bytes").or_else(|| g.get("body")) {
                    value_to_bytes(v).map_err(|e| RuntimeError::new(e).at(span))?
                } else if let Some(v) = g.get("json") {
                    if !headers.contains_key("content-type") {
                        headers.insert(
                            "content-type".to_string(),
                            vec!["application/json".to_string()],
                        );
                    }
                    serde_json::to_vec(&shae_value_to_json(v)).unwrap_or_default()
                } else {
                    Vec::new()
                };
                out.push((method, path, headers, body));
            }
            other => {
                return Err(RuntimeError::new(format!(
                    "Request entry must be string or map, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        }
    }
    Ok(out)
}

fn update_server_map_addr(map: &Arc<RwLock<IndexMap<String, Value>>>, addr: SocketAddr) {
    let mut g = map.write().unwrap();
    g.insert("port".to_string(), Value::Int(addr.port() as i64));
    let s = addr.to_string();
    g.insert("addr".to_string(), Value::String(s.clone()));
    g.insert("local_addr".to_string(), Value::String(s.clone()));
    g.insert("localAddr".to_string(), Value::String(s));
}

pub fn eval_http_server_method(
    ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_map_id(map, "_http_server_id", span)?;
    let srv = http_servers()
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| RuntimeError::new("HTTP server handle not found".into()).at(span))?;

    match method {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "any" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(format!(
                    "server.{}() expects 2 arguments (path, handler)",
                    method
                ))
                .at(span));
            }
            let pattern = args[0].to_display();
            let handler = args[1].clone();
            if !handler.is_callable() {
                return Err(RuntimeError::new("Route handler must be callable".into()).at(span));
            }
            let http_method = if method == "any" {
                "*".to_string()
            } else {
                method.to_ascii_uppercase()
            };
            srv.router.write().unwrap().routes.push(RouteEntry {
                method: http_method,
                pattern,
                handler,
            });
            *srv.worker_ctx.write().unwrap() = Some(WorkerEnvContext::capture(ev));
            Ok(Value::Map(map.clone()))
        }
        "route" => {
            if args.len() != 3 {
                return Err(RuntimeError::new(
                    "server.route() expects 3 arguments (method, path, handler)".into(),
                )
                .at(span));
            }
            let http_method = args[0].to_display().to_ascii_uppercase();
            let pattern = args[1].to_display();
            let handler = args[2].clone();
            if !handler.is_callable() {
                return Err(RuntimeError::new("Route handler must be callable".into()).at(span));
            }
            srv.router.write().unwrap().routes.push(RouteEntry {
                method: http_method,
                pattern,
                handler,
            });
            *srv.worker_ctx.write().unwrap() = Some(WorkerEnvContext::capture(ev));
            Ok(Value::Map(map.clone()))
        }
        "listen" | "start" | "serve" => {
            if args.is_empty() {
                return Err(RuntimeError::new(
                    "server.listen() expects at least 1 argument (addr_or_port)".into(),
                )
                .at(span));
            }
            let addr = parse_bind_addr_val(&args[0], span)?;
            if let Some(arg1) = args.get(1) {
                if arg1.is_callable() {
                    let mut r = srv.router.write().unwrap();
                    r.fallback = Some(arg1.clone());
                } else if let Value::Map(opt_m) = arg1 {
                    let g = opt_m.read().unwrap();
                    if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
                        let proto = *srv.protocol.lock().unwrap();
                        let alpn = if proto == ServerProtocol::Http2 {
                            vec![b"h2".to_vec()]
                        } else {
                            vec![b"http/1.1".to_vec()]
                        };
                        let cfg = build_tls_server_config(
                            &cert.to_display(),
                            &key.to_display(),
                            alpn,
                        )
                        .map_err(|e| RuntimeError::new(e).at(span))?;
                        *srv.tls_config.lock().unwrap() = Some(cfg);
                    } else {
                        drop(g);
                        let mut r = srv.router.write().unwrap();
                        apply_handler_or_routes(&mut r, arg1, span)?;
                    }
                }
            }
            if let Some(Value::Map(opt_m)) = args.get(2) {
                let g = opt_m.read().unwrap();
                if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
                    let proto = *srv.protocol.lock().unwrap();
                    let alpn = if proto == ServerProtocol::Http2 {
                        vec![b"h2".to_vec()]
                    } else {
                        vec![b"http/1.1".to_vec()]
                    };
                    let cfg = build_tls_server_config(&cert.to_display(), &key.to_display(), alpn)
                        .map_err(|e| RuntimeError::new(e).at(span))?;
                    *srv.tls_config.lock().unwrap() = Some(cfg);
                }
            }

            // Refresh worker environment snapshot right before spawning listener
            *srv.worker_ctx.write().unwrap() = Some(WorkerEnvContext::capture(ev));
            let local = srv
                .bind_and_start(&addr)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            update_server_map_addr(map, local);
            Ok(Value::Map(map.clone()))
        }
        "stop" | "close" => Ok(Value::Bool(srv.stop())),
        "is_running" | "isRunning" => Ok(Value::Bool(srv.running.load(Ordering::Relaxed))),
        "stats" => {
            let mut m = IndexMap::new();
            m.insert(
                "active_connections".to_string(),
                Value::Int(srv.stats.active_connections.load(Ordering::Relaxed) as i64),
            );
            m.insert(
                "total_connections".to_string(),
                Value::Int(srv.stats.total_connections.load(Ordering::Relaxed) as i64),
            );
            m.insert(
                "total_requests".to_string(),
                Value::Int(srv.stats.total_requests.load(Ordering::Relaxed) as i64),
            );
            m.insert(
                "running".to_string(),
                Value::Bool(srv.running.load(Ordering::Relaxed)),
            );
            m.insert("port".to_string(), Value::Int(srv.port() as i64));
            Ok(Value::Map(Arc::new(RwLock::new(m))))
        }
        other => Err(RuntimeError::new(format!("Unknown HttpServer method '{}'", other)).at(span)),
    }
}

pub fn eval_http_pool_method(
    _ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_map_id(map, "_http_pool_id", span)?;
    let pool = http_pools()
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| RuntimeError::new("HTTP client pool not found".into()).at(span))?;

    match method {
        "request" | "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => {
            let default_m = if method == "request" {
                "GET"
            } else {
                method
            };
            let (m, url, hdrs, body, retry, insecure) =
                extract_request_opts(&default_m.to_ascii_uppercase(), &args, span)?;
            let resp = pool
                .request(&m, &url, &hdrs, &body, retry, insecure)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(wire_response_to_shae_value(
                resp.status,
                &resp.headers,
                &resp.body,
                &resp.trailers,
                &resp.version,
                0,
            ))
        }
        "pipeline" => {
            if args.len() < 2 {
                return Err(RuntimeError::new(
                    "pool.pipeline() expects 2 arguments (base_url, requests_array)".into(),
                )
                .at(span));
            }
            let base_url = args[0].to_display();
            let reqs = extract_batch_requests(&args[1], span)?;
            let insecure = matches!(args.get(2), Some(Value::Bool(true)));
            let resps = pool
                .pipeline(&base_url, &reqs, insecure)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            let shae_resps: Vec<Value> = resps
                .into_iter()
                .map(|r| {
                    wire_response_to_shae_value(
                        r.status,
                        &r.headers,
                        &r.body,
                        &r.trailers,
                        &r.version,
                        0,
                    )
                })
                .collect();
            Ok(Value::Array(Arc::new(RwLock::new(shae_resps))))
        }
        "stats" => Ok(pool.stats_map()),
        "clear" => {
            pool.clear();
            Ok(Value::Null)
        }
        "close" => {
            pool.clear();
            http_pools().write().unwrap().remove(&id);
            Ok(Value::Bool(true))
        }
        other => Err(RuntimeError::new(format!("Unknown HttpClientPool method '{}'", other)).at(span)),
    }
}

pub fn eval_h2_conn_method(
    _ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_map_id(map, "_h2_conn_id", span)?;
    let conn = h2_conns()
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| RuntimeError::new("HTTP/2 connection not found".into()).at(span))?;

    match method {
        "request" | "get" | "post" | "put" | "delete" | "head" => {
            let default_m = if method == "request" { "GET" } else { method };
            let (m, path, hdrs, body, _, _) =
                extract_request_opts(&default_m.to_ascii_uppercase(), &args, span)?;
            let r = conn
                .request(&m, &path, &hdrs, &body)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(wire_response_to_shae_value(
                r.status,
                &r.headers,
                &r.body,
                &IndexMap::new(),
                "HTTP/2",
                r.stream_id as u64,
            ))
        }
        "multiplex" => {
            if args.is_empty() {
                return Err(RuntimeError::new(
                    "h2.multiplex() expects an array of request objects".into(),
                )
                .at(span));
            }
            let reqs = extract_batch_requests(&args[0], span)?;
            let resps = conn
                .multiplex(&reqs)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            let vals: Vec<Value> = resps
                .into_iter()
                .map(|r| {
                    wire_response_to_shae_value(
                        r.status,
                        &r.headers,
                        &r.body,
                        &IndexMap::new(),
                        "HTTP/2",
                        r.stream_id as u64,
                    )
                })
                .collect();
            Ok(Value::Array(Arc::new(RwLock::new(vals))))
        }
        "ping" => {
            let mut opaque = *b"SHAE_H2!";
            if let Some(arg) = args.first() {
                let bytes = value_to_bytes(arg).map_err(|e| RuntimeError::new(e).at(span))?;
                for (i, b) in bytes.iter().take(8).enumerate() {
                    opaque[i] = *b;
                }
            }
            conn.ping(opaque)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Bool(true))
        }
        "window_update" | "windowUpdate" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(
                    "h2.window_update() expects (stream_id, increment)".into(),
                )
                .at(span));
            }
            let sid = match args[0] {
                Value::Int(n) if n >= 0 => n as u32,
                _ => return Err(RuntimeError::new("stream_id must be non-negative int".into()).at(span)),
            };
            let inc = match args[1] {
                Value::Int(n) if n > 0 => n as u32,
                _ => return Err(RuntimeError::new("increment must be positive int".into()).at(span)),
            };
            conn.window_update(sid, inc)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Bool(true))
        }
        "rst_stream" | "rstStream" => {
            if args.is_empty() {
                return Err(RuntimeError::new(
                    "h2.rst_stream() expects (stream_id, [error_code])".into(),
                )
                .at(span));
            }
            let sid = match args[0] {
                Value::Int(n) if n > 0 => n as u32,
                _ => return Err(RuntimeError::new("stream_id must be positive int".into()).at(span)),
            };
            let code = match args.get(1) {
                Some(Value::Int(c)) if *c >= 0 => *c as u32,
                _ => 0x8,
            };
            conn.rst_stream(sid, code)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Bool(true))
        }
        "goaway" => {
            let code = match args.first() {
                Some(Value::Int(c)) if *c >= 0 => *c as u32,
                _ => 0,
            };
            conn.goaway(code)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Bool(true))
        }
        "stats" => Ok(conn.stats_map()),
        "close" => {
            conn.close();
            h2_conns().write().unwrap().remove(&id);
            Ok(Value::Bool(true))
        }
        other => Err(RuntimeError::new(format!("Unknown H2Connection method '{}'", other)).at(span)),
    }
}

pub fn eval_h3_server_method(
    ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_map_id(map, "_h3_server_id", span)?;
    let slot = h3_servers()
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| RuntimeError::new("HTTP/3 server handle not found".into()).at(span))?;

    match method {
        "get" | "post" | "put" | "delete" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(format!(
                    "h3_server.{}() expects (path, handler)",
                    method
                ))
                .at(span));
            }
            let pattern = args[0].to_display();
            let handler = args[1].clone();
            if !handler.is_callable() {
                return Err(RuntimeError::new("Route handler must be callable".into()).at(span));
            }
            slot.router.write().unwrap().routes.push(RouteEntry {
                method: method.to_ascii_uppercase(),
                pattern,
                handler,
            });
            let ctx = WorkerEnvContext::capture(ev);
            *slot.worker_ctx.write().unwrap() = Some(ctx.clone());
            if let Some(ref srv) = *slot.running_srv.lock().unwrap() {
                srv.update_context(slot.router.read().unwrap().clone(), ctx);
            }
            Ok(Value::Map(map.clone()))
        }
        "route" => {
            if args.len() != 3 {
                return Err(RuntimeError::new(
                    "h3_server.route() expects (method, path, handler)".into(),
                )
                .at(span));
            }
            let http_method = args[0].to_display().to_ascii_uppercase();
            let pattern = args[1].to_display();
            let handler = args[2].clone();
            if !handler.is_callable() {
                return Err(RuntimeError::new("Route handler must be callable".into()).at(span));
            }
            slot.router.write().unwrap().routes.push(RouteEntry {
                method: http_method,
                pattern,
                handler,
            });
            let ctx = WorkerEnvContext::capture(ev);
            *slot.worker_ctx.write().unwrap() = Some(ctx.clone());
            if let Some(ref srv) = *slot.running_srv.lock().unwrap() {
                srv.update_context(slot.router.read().unwrap().clone(), ctx);
            }
            Ok(Value::Map(map.clone()))
        }
        "listen" | "start" | "serve" => {
            if args.is_empty() {
                return Err(RuntimeError::new(
                    "h3_server.listen() expects addr_or_port".into(),
                )
                .at(span));
            }
            let addr = parse_bind_addr_val(&args[0], span)?;
            let mut tls_cfg = None;
            if let Some(arg1) = args.get(1) {
                if arg1.is_callable() {
                    slot.router.write().unwrap().fallback = Some(arg1.clone());
                } else if let Value::Map(m) = arg1 {
                    let g = m.read().unwrap();
                    if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
                        tls_cfg = Some(
                            build_tls_server_config(
                                &cert.to_display(),
                                &key.to_display(),
                                vec![b"h3".to_vec()],
                            )
                            .map_err(|e| RuntimeError::new(e).at(span))?,
                        );
                    } else {
                        drop(g);
                        let mut r = slot.router.write().unwrap();
                        apply_handler_or_routes(&mut r, arg1, span)?;
                    }
                }
            }
            if let Some(Value::Map(m)) = args.get(2) {
                let g = m.read().unwrap();
                if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
                    tls_cfg = Some(
                        build_tls_server_config(
                            &cert.to_display(),
                            &key.to_display(),
                            vec![b"h3".to_vec()],
                        )
                        .map_err(|e| RuntimeError::new(e).at(span))?,
                    );
                }
            }
            let ctx = WorkerEnvContext::capture(ev);
            let local = slot
                .bind_and_start(&addr, ctx, tls_cfg)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            update_server_map_addr(map, local);
            Ok(Value::Map(map.clone()))
        }
        "stop" | "close" => Ok(Value::Bool(slot.stop())),
        "stats" => Ok(slot.stats_map()),
        other => Err(RuntimeError::new(format!("Unknown H3Server method '{}'", other)).at(span)),
    }
}

pub fn eval_h3_client_method(
    _ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_map_id(map, "_h3_client_id", span)?;
    let client = h3_clients()
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| RuntimeError::new("HTTP/3 client handle not found".into()).at(span))?;

    match method {
        "request" | "get" | "post" | "put" | "delete" | "head" => {
            let default_m = if method == "request" { "GET" } else { method };
            let (m, path, hdrs, body, _, _) =
                extract_request_opts(&default_m.to_ascii_uppercase(), &args, span)?;
            let (status, resp_hdrs, resp_body, sid) = client
                .request(&m, &path, &hdrs, &body)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(wire_response_to_shae_value(
                status,
                &resp_hdrs,
                &resp_body,
                &IndexMap::new(),
                "HTTP/3",
                sid,
            ))
        }
        "multiplex" => {
            if args.is_empty() {
                return Err(RuntimeError::new(
                    "h3.multiplex() expects an array of request objects".into(),
                )
                .at(span));
            }
            let reqs = extract_batch_requests(&args[0], span)?;
            let resps = client
                .multiplex(&reqs)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            let vals: Vec<Value> = resps
                .into_iter()
                .map(|(status, hdrs, body, sid)| {
                    wire_response_to_shae_value(
                        status,
                        &hdrs,
                        &body,
                        &IndexMap::new(),
                        "HTTP/3",
                        sid,
                    )
                })
                .collect();
            Ok(Value::Array(Arc::new(RwLock::new(vals))))
        }
        "stats" => Ok(client.stats_map()),
        "close" => {
            client.close();
            h3_clients().write().unwrap().remove(&id);
            Ok(Value::Bool(true))
        }
        other => Err(RuntimeError::new(format!("Unknown H3Client method '{}'", other)).at(span)),
    }
}

// --- Builtin Module Functions ---

fn fn_create_server(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let id = next_handle_id();
    let srv = Arc::new(HttpServerHandle::new(id, ServerProtocol::Http1));
    http_servers().write().unwrap().insert(id, srv.clone());
    Ok(make_http_server_value(&srv))
}

fn fn_listen(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "listen() expects (addr_or_port, handler_or_routes, [opts])".into(),
        )
        .at(span));
    }
    let addr = parse_bind_addr_val(&args[0], span)?;
    let id = next_handle_id();
    let srv = Arc::new(HttpServerHandle::new(id, ServerProtocol::Http1));
    {
        let mut r = srv.router.write().unwrap();
        apply_handler_or_routes(&mut r, &args[1], span)?;
    }
    if let Some(Value::Map(m)) = args.get(2) {
        let g = m.read().unwrap();
        if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
            let cfg = build_tls_server_config(
                &cert.to_display(),
                &key.to_display(),
                vec![b"http/1.1".to_vec()],
            )
            .map_err(|e| RuntimeError::new(e).at(span))?;
            *srv.tls_config.lock().unwrap() = Some(cfg);
        }
    }
    *srv.worker_ctx.write().unwrap() = Some(WorkerEnvContext::capture(ev));
    srv.bind_and_start(&addr)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    http_servers().write().unwrap().insert(id, srv.clone());
    Ok(make_http_server_value(&srv))
}

fn fn_create_pool(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let id = next_handle_id();
    let mut pool = HttpClientPool::new(id);
    if let Some(Value::Map(m)) = args.first() {
        let g = m.read().unwrap();
        if let Some(Value::Bool(b)) = g.get("insecure") {
            pool.insecure_tls = *b;
        }
        if let Some(ca) = g.get("ca") {
            pool.ca_pem = Some(ca.to_display());
        }
        if let Some(Value::Int(max_idle)) = g.get("max_idle") {
            if *max_idle > 0 {
                pool.max_idle_per_host = *max_idle as usize;
            }
        }
    }
    let arc_pool = Arc::new(pool);
    http_pools().write().unwrap().insert(id, arc_pool.clone());
    Ok(make_http_pool_value(&arc_pool))
}

fn pool_shortcut_req(
    method_default: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let (m, url, hdrs, body, retry, insecure) =
        extract_request_opts(method_default, &args, span)?;
    let resp = default_pool()
        .request(&m, &url, &hdrs, &body, retry, insecure)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(wire_response_to_shae_value(
        resp.status,
        &resp.headers,
        &resp.body,
        &resp.trailers,
        &resp.version,
        0,
    ))
}

fn fn_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("GET", args, span)
}
fn fn_post(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("POST", args, span)
}
fn fn_put(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("PUT", args, span)
}
fn fn_delete(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("DELETE", args, span)
}
fn fn_patch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("PATCH", args, span)
}
fn fn_head(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("HEAD", args, span)
}
fn fn_request(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    pool_shortcut_req("GET", args, span)
}

fn fn_pipeline(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "pipeline() expects (base_url, requests_array)".into(),
        )
        .at(span));
    }
    let base_url = args[0].to_display();
    let reqs = extract_batch_requests(&args[1], span)?;
    let insecure = matches!(args.get(2), Some(Value::Bool(true)));
    let resps = default_pool()
        .pipeline(&base_url, &reqs, insecure)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::Array(Arc::new(RwLock::new(
        resps
            .into_iter()
            .map(|r| {
                wire_response_to_shae_value(
                    r.status,
                    &r.headers,
                    &r.body,
                    &r.trailers,
                    &r.version,
                    0,
                )
            })
            .collect(),
    ))))
}

fn fn_h2_listen(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "h2_listen() expects (addr_or_port, handler_or_routes, [opts])".into(),
        )
        .at(span));
    }
    let addr = parse_bind_addr_val(&args[0], span)?;
    let id = next_handle_id();
    let srv = Arc::new(HttpServerHandle::new(id, ServerProtocol::Http2));
    {
        let mut r = srv.router.write().unwrap();
        apply_handler_or_routes(&mut r, &args[1], span)?;
    }
    if let Some(Value::Map(m)) = args.get(2) {
        let g = m.read().unwrap();
        if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
            let cfg = build_tls_server_config(
                &cert.to_display(),
                &key.to_display(),
                vec![b"h2".to_vec()],
            )
            .map_err(|e| RuntimeError::new(e).at(span))?;
            *srv.tls_config.lock().unwrap() = Some(cfg);
        }
    }
    *srv.worker_ctx.write().unwrap() = Some(WorkerEnvContext::capture(ev));
    srv.bind_and_start(&addr)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    http_servers().write().unwrap().insert(id, srv.clone());
    Ok(make_http_server_value(&srv))
}

fn fn_h2_connect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("h2_connect() expects url_or_addr".into()).at(span));
    }
    let url = args[0].to_display();
    let mut insecure = false;
    let mut ca_pem: Option<String> = None;
    if let Some(Value::Map(m)) = args.get(1) {
        let g = m.read().unwrap();
        if let Some(Value::Bool(b)) = g.get("insecure") {
            insecure = *b;
        }
        if let Some(Value::Bool(b)) = g.get("verify_peer") {
            insecure = !*b;
        }
        if let Some(ca) = g.get("ca") {
            ca_pem = Some(ca.to_display());
        }
    }
    let conn = Arc::new(
        H2ClientConn::connect(&url, insecure, ca_pem.as_deref())
            .map_err(|e| RuntimeError::new(e).at(span))?,
    );
    h2_conns().write().unwrap().insert(conn.id, conn.clone());
    Ok(make_h2_conn_value(&conn))
}

fn shae_pairs_from_val(val: &Value, span: Span) -> Result<Vec<(String, String)>, RuntimeError> {
    let mut pairs = Vec::new();
    match val {
        Value::Map(m) => {
            for (k, v) in m.read().unwrap().iter() {
                pairs.push((k.clone(), v.to_display()));
            }
        }
        Value::Array(arr) => {
            for item in arr.read().unwrap().iter() {
                if let Value::Array(pair) = item {
                    let g = pair.read().unwrap();
                    if g.len() >= 2 {
                        pairs.push((g[0].to_display(), g[1].to_display()));
                    }
                }
            }
        }
        other => {
            return Err(RuntimeError::new(format!(
                "Expected headers map or [k, v] array, got {}",
                other.type_name()
            ))
            .at(span));
        }
    }
    Ok(pairs)
}

fn fn_hpack_encode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("hpack_encode() expects headers map/array".into()).at(span));
    }
    let pairs = shae_pairs_from_val(&args[0], span)?;
    let use_huff = !matches!(args.get(1), Some(Value::Bool(false)));
    let mut codec = HpackCodec::new(4096, use_huff);
    Ok(bytes_to_value(codec.encode(&pairs)))
}

fn fn_hpack_decode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("hpack_decode() expects ByteBuffer or bytes".into()).at(span));
    }
    let bytes = value_to_bytes(&args[0]).map_err(|e| RuntimeError::new(e).at(span))?;
    let mut codec = HpackCodec::default();
    let pairs = codec
        .decode(&bytes)
        .map_err(|e| RuntimeError::new(e.to_string()).at(span))?;
    let mut map = IndexMap::new();
    for (k, v) in pairs {
        map.insert(k, Value::String(v));
    }
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn fn_h3_create_server(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let id = next_handle_id();
    let slot = Arc::new(H3ServerSlot::new(id));
    h3_servers().write().unwrap().insert(id, slot.clone());
    Ok(make_h3_server_value(&slot))
}

fn fn_h3_listen(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "h3_listen() expects (addr_or_port, handler_or_routes, [opts])".into(),
        )
        .at(span));
    }
    let addr = parse_bind_addr_val(&args[0], span)?;
    let id = next_handle_id();
    let slot = Arc::new(H3ServerSlot::new(id));
    {
        let mut r = slot.router.write().unwrap();
        apply_handler_or_routes(&mut r, &args[1], span)?;
    }
    let mut tls_cfg = None;
    if let Some(Value::Map(m)) = args.get(2) {
        let g = m.read().unwrap();
        if let (Some(cert), Some(key)) = (g.get("cert"), g.get("key")) {
            tls_cfg = Some(
                build_tls_server_config(&cert.to_display(), &key.to_display(), vec![b"h3".to_vec()])
                    .map_err(|e| RuntimeError::new(e).at(span))?,
            );
        }
    }
    let ctx = WorkerEnvContext::capture(ev);
    slot.bind_and_start(&addr, ctx, tls_cfg)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    h3_servers().write().unwrap().insert(id, slot.clone());
    Ok(make_h3_server_value(&slot))
}

fn fn_h3_connect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("h3_connect() expects addr_or_url".into()).at(span));
    }
    let mut use_tls = false;
    let mut insecure = false;
    let mut ca_pem: Option<String> = None;
    let mut server_name: Option<String> = None;
    if let Some(Value::Map(m)) = args.get(1) {
        let g = m.read().unwrap();
        if let Some(Value::Bool(b)) = g.get("insecure") {
            insecure = *b;
            if *b {
                use_tls = true;
            }
        }
        if let Some(ca) = g.get("ca").or_else(|| g.get("ca_cert")) {
            ca_pem = Some(ca.to_display());
            use_tls = true;
        }
        if let Some(sn) = g.get("server_name") {
            server_name = Some(sn.to_display());
        }
        if let Some(Value::Bool(b)) = g.get("tls") {
            use_tls = *b;
        }
    }
    let client = Arc::new(
        H3Client::connect_with_opts(
            &args[0].to_display(),
            use_tls,
            insecure,
            ca_pem.as_deref(),
            server_name.as_deref(),
        )
        .map_err(|e| RuntimeError::new(e).at(span))?,
    );
    h3_clients()
        .write()
        .unwrap()
        .insert(client.id, client.clone());
    Ok(make_h3_client_value(&client))
}

fn h3_one_shot(method_default: &str, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let (m, url, hdrs, body, _, _) = extract_request_opts(method_default, &args, span)?;
    let parsed = parse_url(&url).map_err(|e| RuntimeError::new(e).at(span))?;
    let client = H3Client::connect(&url).map_err(|e| RuntimeError::new(e).at(span))?;
    let (status, resp_hdrs, resp_body, sid) = client
        .request(&m, &parsed.path_and_query, &hdrs, &body)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    client.close();
    Ok(wire_response_to_shae_value(
        status,
        &resp_hdrs,
        &resp_body,
        &IndexMap::new(),
        "HTTP/3",
        sid,
    ))
}

fn fn_h3_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    h3_one_shot("GET", args, span)
}
fn fn_h3_post(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    h3_one_shot("POST", args, span)
}
fn fn_h3_request(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    h3_one_shot("GET", args, span)
}

fn fn_varint_encode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let Some(Value::Int(n)) = args.first() else {
        return Err(RuntimeError::new("varint_encode() expects non-negative int".into()).at(span));
    };
    if *n < 0 {
        return Err(RuntimeError::new("varint_encode() cannot encode negative int".into()).at(span));
    }
    let bytes = varint_encode(*n as u64).map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(bytes_to_value(bytes))
}

fn fn_varint_decode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("varint_decode() expects ByteBuffer or bytes".into()).at(span));
    }
    let bytes = value_to_bytes(&args[0]).map_err(|e| RuntimeError::new(e).at(span))?;
    let (val, len) = varint_decode(&bytes).map_err(|e| RuntimeError::new(e).at(span))?;
    let mut m = IndexMap::new();
    m.insert("value".to_string(), Value::Int(val as i64));
    m.insert("bytes_read".to_string(), Value::Int(len as i64));
    Ok(Value::Map(Arc::new(RwLock::new(m))))
}

fn fn_qpack_encode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("qpack_encode() expects headers map/array".into()).at(span));
    }
    let pairs = shae_pairs_from_val(&args[0], span)?;
    let use_huff = !matches!(args.get(1), Some(Value::Bool(false)));
    Ok(bytes_to_value(qpack_encode_headers(&pairs, use_huff)))
}

fn fn_qpack_decode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("qpack_decode() expects ByteBuffer or bytes".into()).at(span));
    }
    let bytes = value_to_bytes(&args[0]).map_err(|e| RuntimeError::new(e).at(span))?;
    let pairs = qpack_decode_headers(&bytes).map_err(|e| RuntimeError::new(e).at(span))?;
    let mut map = IndexMap::new();
    for (k, v) in pairs {
        map.insert(k, Value::String(v));
    }
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

pub fn build_web_http_module() -> Value {
    make_map(vec![
        ("create_server", fn_create_server),
        ("createServer", fn_create_server),
        ("server", fn_create_server),
        ("listen", fn_listen),
        ("serve", fn_listen),
        ("create_pool", fn_create_pool),
        ("createPool", fn_create_pool),
        ("pool", fn_create_pool),
        ("get", fn_get),
        ("post", fn_post),
        ("put", fn_put),
        ("delete", fn_delete),
        ("patch", fn_patch),
        ("head", fn_head),
        ("request", fn_request),
        ("pipeline", fn_pipeline),
        ("h2_listen", fn_h2_listen),
        ("h2Listen", fn_h2_listen),
        ("h2_connect", fn_h2_connect),
        ("h2Connect", fn_h2_connect),
        ("hpack_encode", fn_hpack_encode),
        ("hpackEncode", fn_hpack_encode),
        ("hpack_decode", fn_hpack_decode),
        ("hpackDecode", fn_hpack_decode),
        ("h3_create_server", fn_h3_create_server),
        ("h3_listen", fn_h3_listen),
        ("h3Listen", fn_h3_listen),
        ("h3_connect", fn_h3_connect),
        ("h3Connect", fn_h3_connect),
        ("varint_encode", fn_varint_encode),
        ("varint_decode", fn_varint_decode),
        ("qpack_encode", fn_qpack_encode),
        ("qpack_decode", fn_qpack_decode),
    ])
}

pub fn build_http3_module() -> Value {
    make_map(vec![
        ("create_server", fn_h3_create_server),
        ("createServer", fn_h3_create_server),
        ("server", fn_h3_create_server),
        ("listen", fn_h3_listen),
        ("h3_listen", fn_h3_listen),
        ("serve", fn_h3_listen),
        ("connect", fn_h3_connect),
        ("h3_connect", fn_h3_connect),
        ("get", fn_h3_get),
        ("post", fn_h3_post),
        ("request", fn_h3_request),
        ("varint_encode", fn_varint_encode),
        ("varint_decode", fn_varint_decode),
        ("qpack_encode", fn_qpack_encode),
        ("qpack_decode", fn_qpack_decode),
        ("hpack_encode", fn_hpack_encode),
        ("hpack_decode", fn_hpack_decode),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct MemStream {
        read_cur: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl MemStream {
        fn new(data: &[u8]) -> Self {
            Self {
                read_cur: Cursor::new(data.to_vec()),
                written: Vec::new(),
            }
        }
    }

    impl Read for MemStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read_cur.read(buf)
        }
    }

    impl Write for MemStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn test_h1_request_smuggling_and_validation() {
        // 1. Both TE and CL rejected
        let raw = b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n0\r\n\r\n";
        let mut bs = BufStream::new(MemStream::new(raw));
        assert!(matches!(
            parse_h1_request(&mut bs, H1Limits::default(), false),
            Err(H1Error::BadRequest(_))
        ));

        // 2. Conflicting Content-Length rejected
        let raw2 = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 7\r\n\r\nhello";
        let mut bs2 = BufStream::new(MemStream::new(raw2));
        assert!(matches!(
            parse_h1_request(&mut bs2, H1Limits::default(), false),
            Err(H1Error::BadRequest(_))
        ));

        // 3. Identical Content-Length accepted
        let raw3 = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhello";
        let mut bs3 = BufStream::new(MemStream::new(raw3));
        let req = parse_h1_request(&mut bs3, H1Limits::default(), false).unwrap();
        assert_eq!(req.body, b"hello");

        // 4. Space before colon rejected
        let raw4 = b"GET / HTTP/1.1\r\nHost : localhost\r\n\r\n";
        let mut bs4 = BufStream::new(MemStream::new(raw4));
        assert!(matches!(
            parse_h1_request(&mut bs4, H1Limits::default(), false),
            Err(H1Error::BadRequest(_))
        ));

        // 5. obs-fold rejected on server request
        let raw5 = b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Test: a\r\n folded\r\n\r\n";
        let mut bs5 = BufStream::new(MemStream::new(raw5));
        assert!(matches!(
            parse_h1_request(&mut bs5, H1Limits::default(), false),
            Err(H1Error::BadRequest(_))
        ));

        // 6. Outbound CRLF injection rejected
        let mut h = IndexMap::new();
        h.insert("x-bad".to_string(), vec!["val\r\nInjected: yes".to_string()]);
        assert!(serialize_h1_request("GET", "/", "localhost", &h, &[], true).is_err());
    }

    #[test]
    fn test_h1_chunked_extensions_trailers_and_pipelining() {
        let raw = concat!(
            "POST /up HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n",
            "5;ext=1\r\nhello\r\n6;foo=bar\r\n world\r\n0\r\nX-Checksum: 42\r\n\r\n",
            "GET /second HTTP/1.1\r\nHost: example.com\r\n\r\n",
            "GET /third HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
        );
        let mut bs = BufStream::new(MemStream::new(raw.as_bytes()));
        let r1 = parse_h1_request(&mut bs, H1Limits::default(), false).unwrap();
        assert_eq!(r1.body, b"hello world");
        assert_eq!(r1.trailers.get("x-checksum").unwrap(), &vec!["42".to_string()]);
        assert!(r1.keep_alive);

        let r2 = parse_h1_request(&mut bs, H1Limits::default(), false).unwrap();
        assert_eq!(r2.target, "/second");
        assert!(r2.keep_alive);

        let r3 = parse_h1_request(&mut bs, H1Limits::default(), false).unwrap();
        assert_eq!(r3.target, "/third");
        assert!(!r3.keep_alive);
    }

    #[test]
    fn test_h1_bodyless_head_204_and_100_continue() {
        let raw = concat!(
            "HTTP/1.1 100 Continue\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n\r\n",
            "HTTP/1.1 204 No Content\r\n\r\n"
        );
        let mut bs = BufStream::new(MemStream::new(raw.as_bytes()));
        let r1 = parse_h1_response(&mut bs, "HEAD", H1Limits::default()).unwrap();
        assert_eq!(r1.status, 200);
        assert!(r1.body.is_empty());
        assert_eq!(r1.headers.get("set-cookie").unwrap(), &vec!["a=1".to_string(), "b=2".to_string()]);

        let r2 = parse_h1_response(&mut bs, "GET", H1Limits::default()).unwrap();
        assert_eq!(r2.status, 204);
        assert!(r2.body.is_empty());
    }

    #[test]
    fn test_hpack_huffman_kraft_equality_and_rfc7541_vectors() {
        // Mathematical proof of RFC 7541 Appendix B completeness: sum 2^(30 - len) == 2^30
        let kraft_sum: u64 = HPACK_HUFFMAN_CODES
            .iter()
            .map(|&(_, len)| 1u64 << (30 - len as u32))
            .sum();
        assert_eq!(kraft_sum, 1u64 << 30, "Huffman table must satisfy Kraft equality");
        assert!(!huff_trie().is_empty());

        // RFC 7541 C.1.1 / C.1.2 / C.1.3 integers
        let mut buf = Vec::new();
        hpack_encode_int(&mut buf, 5, 0, 10);
        assert_eq!(buf, vec![0x0a]);
        buf.clear();
        hpack_encode_int(&mut buf, 5, 0, 1337);
        assert_eq!(buf, vec![0x1f, 0x9a, 0x0a]);
        buf.clear();
        hpack_encode_int(&mut buf, 8, 0, 42);
        assert_eq!(buf, vec![0x2a]);

        let mut pos = 0;
        assert_eq!(hpack_decode_int(&[0x1f, 0x9a, 0x0a], &mut pos, 5).unwrap(), 1337);

        // RFC 7541 C.4.1 Request Examples with Huffman Coding
        let mut enc = HpackCodec::new(4096, true);
        let mut dec = HpackCodec::new(4096, true);

        let req1 = vec![
            (":method".into(), "GET".into()),
            (":scheme".into(), "http".into()),
            (":path".into(), "/".into()),
            (":authority".into(), "www.example.com".into()),
        ];
        let wire1 = enc.encode(&req1);
        assert_eq!(
            wire1,
            hex_decode("8286 8441 8cf1 e3c2 e5f2 3a6b a0ab 90f4 ff")
        );
        assert_eq!(dec.decode(&wire1).unwrap(), req1);

        // RFC 7541 C.4.2 Second request reuses dynamic table entry 62 (:authority: www.example.com)
        let req2 = vec![
            (":method".into(), "GET".into()),
            (":scheme".into(), "http".into()),
            (":path".into(), "/".into()),
            (":authority".into(), "www.example.com".into()),
            ("cache-control".into(), "no-cache".into()),
        ];
        let wire2 = enc.encode(&req2);
        assert_eq!(
            wire2,
            hex_decode("8286 84be 5886 a8eb 1064 9cbf")
        );
        assert_eq!(dec.decode(&wire2).unwrap(), req2);
    }

    #[test]
    fn test_hpack_adversarial_and_eviction() {
        let mut dec = HpackCodec::new(64, false);
        // Entry ("custom-key", "custom-value") size = 10 + 12 + 32 = 54 <= 64
        // Second entry ("another", "value-two") size = 7 + 9 + 32 = 48 -> evicts first!
        let mut enc = HpackCodec::new(64, false);
        let b1 = enc.encode(&[("custom-key".into(), "custom-value".into())]);
        let b2 = enc.encode(&[("another".into(), "value-two".into())]);
        assert_eq!(dec.decode(&b1).unwrap().len(), 1);
        assert_eq!(dec.decode(&b2).unwrap().len(), 1);
        assert_eq!(dec.table.dynamic_len(), 1);

        // Oversized entry (> 64) with Incremental Indexing (0x40) empties table without error
        let mut b3 = vec![0x40];
        let ov_name = b"very-long-header-name";
        let ov_val = b"very-long-header-value-exceeding-64";
        hpack_encode_int(&mut b3, 7, 0x00, ov_name.len() as u64);
        b3.extend_from_slice(ov_name);
        hpack_encode_int(&mut b3, 7, 0x00, ov_val.len() as u64);
        b3.extend_from_slice(ov_val);
        assert_eq!(dec.decode(&b3).unwrap().len(), 1);
        assert_eq!(dec.table.dynamic_len(), 0);
        assert_eq!(dec.table.current_size(), 0);

        // Dynamic table size update AFTER header representation -> CompressionError
        assert!(matches!(
            dec.decode(&[0x82, 0x20]),
            Err(H2Error { code: H2ErrorCode::CompressionError, .. })
        ));

        // Dynamic table size update > protocol_max_size -> CompressionError
        assert!(matches!(
            dec.decode(&[0x3f, 0xe1, 0x07]),
            Err(H2Error { code: H2ErrorCode::CompressionError, .. })
        ));

        // Invalid Huffman EOS padding (0x00 instead of 1-bits)
        assert!(matches!(
            hpack_huffman_decode(&[0x00]),
            Err(H2Error { code: H2ErrorCode::CompressionError, .. })
        ));
    }

    #[test]
    fn test_h2_frames_and_continuation_state_machine() {
        let frames = vec![
            H2Frame::Data { stream_id: 1, end_stream: false, data: b"abc".to_vec(), pad_len: 4 },
            H2Frame::Headers {
                stream_id: 1,
                end_stream: false,
                end_headers: false,
                priority: Some((true, 0, 15)),
                header_block: b"part1".to_vec(),
                pad_len: 2,
            },
            H2Frame::Continuation { stream_id: 1, end_headers: true, header_block: b"part2".to_vec() },
            H2Frame::Priority { stream_id: 3, exclusive: false, stream_dep: 1, weight: 10 },
            H2Frame::RstStream { stream_id: 3, error_code: 0x8 },
            H2Frame::Settings { ack: false, params: vec![(0x4, 65535), (0x5, 16384)] },
            H2Frame::Settings { ack: true, params: vec![] },
            H2Frame::PushPromise { stream_id: 1, end_headers: true, promised_stream_id: 2, header_block: b"pp".to_vec(), pad_len: 1 },
            H2Frame::Ping { ack: true, opaque: *b"12345678" },
            H2Frame::Goaway { last_stream_id: 1, error_code: 0, debug_data: b"bye".to_vec() },
            H2Frame::WindowUpdate { stream_id: 0, increment: 32768 },
        ];

        for f in &frames {
            let wire = f.encode();
            let mut hdr_b = [0u8; 9];
            hdr_b.copy_from_slice(&wire[..9]);
            let hdr = H2FrameHeader::decode(&hdr_b);
            let decoded = H2Frame::decode(hdr, &wire[9..], 16384).unwrap();
            assert_eq!(&decoded, f);
        }

        // Adversarial: WINDOW_UPDATE with 0 increment -> PROTOCOL_ERROR
        let zero_wu = H2FrameHeader { length: 4, frame_type: 0x8, flags: 0, stream_id: 0 };
        assert_eq!(
            H2Frame::decode(zero_wu, &[0, 0, 0, 0], 16384).unwrap_err().code,
            H2ErrorCode::ProtocolError
        );

        // Adversarial: DATA padding >= payload length -> PROTOCOL_ERROR
        let bad_pad = H2FrameHeader { length: 3, frame_type: 0x0, flags: H2_FLAG_PADDED, stream_id: 1 };
        assert_eq!(
            H2Frame::decode(bad_pad, &[3, 0, 0], 16384).unwrap_err().code,
            H2ErrorCode::ProtocolError
        );

        // Adversarial: interleaved non-CONTINUATION during CONTINUATION -> PROTOCOL_ERROR
        let mut asm = H2ContinuationAssembler::default();
        assert!(asm.process_frame(H2Frame::Headers {
            stream_id: 1,
            end_stream: false,
            end_headers: false,
            priority: None,
            header_block: b"a".to_vec(),
            pad_len: 0,
        }).unwrap().is_none());
        assert_eq!(
            asm.process_frame(H2Frame::Ping { ack: false, opaque: [0; 8] }).unwrap_err().code,
            H2ErrorCode::ProtocolError
        );
    }

    #[test]
    fn test_quic_varints_pn_and_rfc9001_initial_keys() {
        // RFC 9000 Appendix A.1 VarInt vectors
        assert_eq!(varint_decode(&hex_decode("25")).unwrap(), (37, 1));
        assert_eq!(varint_decode(&hex_decode("4025")).unwrap(), (37, 2));
        assert_eq!(varint_decode(&hex_decode("7bbd")).unwrap(), (15293, 2));
        assert_eq!(varint_decode(&hex_decode("9d7f3e7d")).unwrap(), (494878333, 4));
        assert_eq!(
            varint_decode(&hex_decode("c2197c5eff14e88c")).unwrap(),
            (151288809941952652, 8)
        );
        assert!(varint_encode(1u64 << 62).is_err());

        // RFC 9000 Appendix A.3 Packet Number reconstruction
        assert_eq!(reconstruct_pn(0xa82f30ea, 0x9b32, 2), 0xa82f9b32);

        // RFC 9001 Appendix A.1 Initial Key Derivation vector
        let dcid = hex_decode("8394c8f03e515708");
        let (_seed, c_init, _s_init) = derive_initial_secrets(&dcid).unwrap();
        assert_eq!(
            c_init.secret,
            hex_decode("c00cf151ca5be075ed0ebfb5c80323c42d6b7db67881289af4008f1f6c357aea")
        );
        assert_eq!(c_init.key, hex_decode("1f369613dd76d5467730efcbe3b1a22d"));
        assert_eq!(c_init.iv, hex_decode("fa044b2f42a3fd3b46fb255c").as_slice());
        assert_eq!(c_init.hp, hex_decode("9f50449e04a0e810283a1e9933adedd2"));

        // Seal & open Initial + tamper rejection
        let cipher = QuicCipher::Ring(c_init);
        let frames = vec![
            QuicFrame::Ping,
            QuicFrame::Crypto { offset: 0, data: b"hello-quic".to_vec() },
        ];
        let mut pkt = seal_quic_packet(
            QuicPacketType::Initial,
            &dcid,
            b"\x01\x02\x03\x04",
            &[],
            2,
            None,
            &frames,
            &cipher,
            1200,
        )
        .unwrap();
        assert_eq!(pkt.len(), 1200);
        let (opened, _) = open_quic_packet(&pkt, 4, -1, &cipher).unwrap();
        assert_eq!(opened.pn, 2);
        assert!(opened.frames.iter().any(|f| matches!(f, QuicFrame::Crypto { data, .. } if data == b"hello-quic")));

        // Single bit flip in ciphertext must fail AEAD authentication
        let last = pkt.len() - 5;
        pkt[last] ^= 0x01;
        assert!(open_quic_packet(&pkt, 4, -1, &cipher).is_err());
    }

    #[test]
    fn test_quic_frames_qpack_and_h3_frames() {
        let qframes = vec![
            QuicFrame::Ping,
            QuicFrame::Ack { largest: 10, delay: 5, first_range: 2, ranges: vec![(1, 3)], ecn: Some((1, 2, 3)) },
            QuicFrame::ResetStream { stream_id: 4, error_code: 0x100, final_size: 42 },
            QuicFrame::StopSending { stream_id: 4, error_code: 0x100 },
            QuicFrame::Crypto { offset: 0, data: b"hs".to_vec() },
            QuicFrame::Stream { stream_id: 0, offset: Some(12), fin: true, has_len: true, data: b"payload".to_vec() },
            QuicFrame::MaxData { max_data: 65536 },
            QuicFrame::MaxStreamData { stream_id: 0, max_stream_data: 16384 },
            QuicFrame::ConnectionClose { is_app: false, error_code: 0x0a, frame_type: Some(0x06), reason: "done".into() },
        ];
        let mut raw = Vec::new();
        for f in &qframes {
            raw.extend_from_slice(&f.encode().unwrap());
        }
        assert_eq!(QuicFrame::decode_all(&raw).unwrap(), qframes);

        // QPACK 99-entry static table + encode/decode round-trip
        assert_eq!(QPACK_STATIC_TABLE.len(), 99);
        let hdrs = vec![
            (":method".into(), "GET".into()),
            (":path".into(), "/api/v3".into()),
            (":scheme".into(), "https".into()),
            (":authority".into(), "localhost:4433".into()),
            ("content-type".into(), "application/json".into()),
            ("x-custom-h3".into(), "shae-quic".into()),
        ];
        let qencoded = qpack_encode_headers(&hdrs, true);
        assert_eq!(qpack_decode_headers(&qencoded).unwrap(), hdrs);

        // H3 frames round-trip
        let h3f = vec![
            H3Frame::Settings(vec![(0x01, 0), (0x06, 65536)]),
            H3Frame::Headers(qencoded),
            H3Frame::Data(b"{\"ok\":true}".to_vec()),
            H3Frame::CancelPush(7),
            H3Frame::PushPromise { push_id: 2, encoded_field_section: vec![0, 0] },
            H3Frame::MaxPushId(8),
            H3Frame::Goaway(4),
        ];
        let mut h3_raw = Vec::new();
        for f in &h3f {
            h3_raw.extend_from_slice(&f.encode().unwrap());
        }
        assert_eq!(H3Frame::decode_all(&h3_raw).unwrap(), h3f);
    }

    #[test]
    fn test_h3_stream_reassembler_rejects_oom_offset() {
        let mut reasm = H3StreamReassembler::default();
        assert!(reasm.push(1u64 << 50, b"x", true).is_err());
        assert!(reasm.take_if_complete().is_none());

        let mut reasm2 = H3StreamReassembler::default();
        reasm2.fin_offset = Some(1u64 << 50);
        assert!(reasm2.take_if_complete().is_none());
    }

    #[test]
    fn test_h1_serialize_response_chunked_without_trailers() {
        let mut hdrs = IndexMap::new();
        hdrs.insert("transfer-encoding".to_string(), vec!["chunked".to_string()]);
        let resp = WireResponse {
            status: 200,
            headers: hdrs,
            body: b"hello".to_vec(),
            trailers: IndexMap::new(),
        };
        let wire = serialize_h1_response(&resp, "GET", true).unwrap();
        let mut bs = BufStream::new(MemStream {
            read_cur: Cursor::new(wire),
            written: Vec::new(),
        });
        let parsed = parse_h1_response(&mut bs, "GET", H1Limits::default()).unwrap();
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.body, b"hello");
    }

    #[test]
    fn test_h1_request_line_crlf_and_bare_lf_rejected() {
        let err = serialize_h1_request(
            "GET",
            "/ok HTTP/1.1\r\nX-Injected: bad",
            "localhost",
            &IndexMap::new(),
            b"",
            true,
        );
        assert!(matches!(err, Err(H1Error::BadRequest(_))));

        assert!(parse_url("http://localhost/foo\r\nbar").is_err());
        assert!(parse_url("http://localhost/foo bar").is_err());

        let raw_bare_lf = b"GET / HTTP/1.1\r\nHost: localhost\nX-Test: 1\r\n\r\n".to_vec();
        let mut bs = BufStream::new(MemStream {
            read_cur: Cursor::new(raw_bare_lf),
            written: Vec::new(),
        });
        assert!(matches!(
            parse_h1_request(&mut bs, H1Limits::default(), false),
            Err(H1Error::BadRequest(_))
        ));
    }

    #[test]
    fn test_qpack_h3_and_reconstruct_pn_overflow_guards() {
        let bad_qpack = [0x00, 0x00, 0x27, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        assert!(qpack_decode_headers(&bad_qpack).is_err());

        let bad_h3 = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(H3Frame::decode_all(&bad_h3).is_err());

        assert_eq!(reconstruct_pn(0, 1, 8), 1);
        assert_eq!(reconstruct_pn(10, 11, 64), 11);

        let p = parse_url("http://[::1]:8080/test").unwrap();
        assert_eq!(p.host, "::1");
        assert_eq!(p.authority, "[::1]:8080");
        assert_eq!(format_socket_addr(&p.host, p.port), "[::1]:8080");
    }
}
