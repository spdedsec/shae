//! Pure-Rust RFC 6455 WebSocket Engine for Shae.
//!
//! Conforming to RFC 6455:
//! - Full-duplex client and server network connections
//! - Client handshake with Sec-WebSocket-Key and HTTP/1.1 Upgrade
//! - Server handshake with Sec-WebSocket-Accept via SHA-1 + GUID + Base64
//! - Framing: FIN, RSV, OpCodes (Continuation, Text, Binary, Close, Ping, Pong)
//! - Masking: 4-byte XOR client-to-server masking; unmasked server-to-client
//! - Lengths: 7-bit, 16-bit extended, 64-bit extended payload lengths
//! - Control frames: automatic Ping -> Pong reply, graceful Close handshake
//! - Standard library bindings for `std:ws` (`ws.connect`, `ws.listen`, send, recv, etc.)

use crate::ast::Span;
use crate::buffer::ByteBuffer;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::{BuiltinFn, Value};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use indexmap::IndexMap;
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

// ---------------------------------------------------------------------------
// RFC 6455 Constants
// ---------------------------------------------------------------------------

pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub const OPCODE_CONTINUATION: u8 = 0x0;
pub const OPCODE_TEXT: u8 = 0x1;
pub const OPCODE_BINARY: u8 = 0x2;
pub const OPCODE_CLOSE: u8 = 0x8;
pub const OPCODE_PING: u8 = 0x9;
pub const OPCODE_PONG: u8 = 0xA;

pub const CLOSE_NORMAL: u16 = 1000;
pub const CLOSE_GOING_AWAY: u16 = 1001;
pub const CLOSE_PROTOCOL_ERROR: u16 = 1002;
pub const CLOSE_UNSUPPORTED: u16 = 1003;
pub const CLOSE_INVALID_PAYLOAD: u16 = 1007;
pub const CLOSE_POLICY_VIOLATION: u16 = 1008;
pub const CLOSE_MESSAGE_TOO_BIG: u16 = 1009;
pub const CLOSE_INTERNAL_ERROR: u16 = 1011;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsFrame {
    pub fin: bool,
    pub opcode: u8,
    pub masked: bool,
    pub mask_key: Option<[u8; 4]>,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Handshake Helpers
// ---------------------------------------------------------------------------

/// Computes the RFC 6455 `Sec-WebSocket-Accept` value from a `Sec-WebSocket-Key`.
pub fn compute_accept_key(client_key: &str) -> String {
    let mut ctx = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(client_key.trim().as_bytes());
    ctx.update(WS_GUID.as_bytes());
    let sha1 = ctx.finish();
    BASE64_STANDARD.encode(sha1.as_ref())
}

/// Generates a random 16-byte base64-encoded `Sec-WebSocket-Key`.
pub fn generate_key() -> String {
    let mut nonce = [0u8; 16];
    for b in &mut nonce {
        *b = fastrand::u8(..);
    }
    BASE64_STANDARD.encode(&nonce)
}

/// Parses a WebSocket URL into `(host, port, path)`.
pub fn parse_ws_url(url: &str) -> Result<(String, u16, String), String> {
    let clean = url.trim();
    let without_scheme = if let Some(rest) = clean.strip_prefix("ws://") {
        rest
    } else if let Some(rest) = clean.strip_prefix("wss://") {
        rest
    } else {
        clean
    };

    let (host_port, path) = match without_scheme.find('/') {
        Some(idx) => (&without_scheme[..idx], &without_scheme[idx..]),
        None => (without_scheme, "/"),
    };

    let (host, port) = if let Some(colon_idx) = host_port.find(':') {
        let host = &host_port[..colon_idx];
        let port_str = &host_port[colon_idx + 1..];
        let port = port_str
            .parse::<u16>()
            .map_err(|e| format!("Invalid port in URL '{}': {}", url, e))?;
        (host.to_string(), port)
    } else {
        let port = if clean.starts_with("wss://") { 443 } else { 80 };
        (host_port.to_string(), port)
    };

    let host = if host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host
    };

    Ok((host, port, path.to_string()))
}

/// Executes the client-side RFC 6455 handshake on a newly connected `TcpStream`.
pub fn perform_client_handshake(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    path: &str,
    client_key: &str,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<(), String> {
    let mut req = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n",
        path, host, port, client_key
    );

    if let Some(headers) = extra_headers {
        for (k, v) in headers {
            req.push_str(&format!("{}: {}\r\n", k, v));
        }
    }
    req.push_str("\r\n");

    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("Failed to send client handshake request: {}", e))?;
    stream
        .flush()
        .map_err(|e| format!("Failed to flush client handshake request: {}", e))?;

    // Read HTTP response header line by line until \r\n\r\n
    let mut response_bytes = Vec::new();
    let mut buf = [0u8; 1];
    while response_bytes.len() < 8192 {
        stream
            .read_exact(&mut buf)
            .map_err(|e| format!("Failed to read handshake response: {}", e))?;
        response_bytes.push(buf[0]);
        if response_bytes.ends_with(b"\r\n\r\n") || response_bytes.ends_with(b"\n\n") {
            break;
        }
    }

    let resp_str = String::from_utf8_lossy(&response_bytes);
    let mut lines = resp_str.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "Empty handshake response from server".to_string())?;

    if !status_line.contains("101") {
        return Err(format!(
            "WebSocket handshake rejected by server: {}",
            status_line
        ));
    }

    let expected_accept = compute_accept_key(client_key);
    let mut found_accept = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed
            .to_ascii_lowercase()
            .starts_with("sec-websocket-accept:")
        {
            if let Some((_, val)) = line.split_once(':') {
                if val.trim() == expected_accept {
                    found_accept = true;
                    break;
                }
            }
        }
    }

    if !found_accept {
        return Err(format!(
            "Server Sec-WebSocket-Accept did not match expected key '{}'",
            expected_accept
        ));
    }

    Ok(())
}

/// Executes the server-side RFC 6455 handshake on an incoming `TcpStream`.
pub fn perform_server_handshake(stream: &mut TcpStream) -> Result<(String, String), String> {
    let mut request_bytes = Vec::new();
    let mut buf = [0u8; 1];
    while request_bytes.len() < 8192 {
        stream
            .read_exact(&mut buf)
            .map_err(|e| format!("Failed to read handshake request: {}", e))?;
        request_bytes.push(buf[0]);
        if request_bytes.ends_with(b"\r\n\r\n") || request_bytes.ends_with(b"\n\n") {
            break;
        }
    }

    let req_str = String::from_utf8_lossy(&request_bytes);
    let mut lines = req_str.lines();
    let req_line = lines
        .next()
        .ok_or_else(|| "Empty handshake request from client".to_string())?;

    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" {
        return Err(format!(
            "Invalid WebSocket HTTP request line: '{}'",
            req_line
        ));
    }
    let path = parts[1].to_string();

    let mut key = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed
            .to_ascii_lowercase()
            .starts_with("sec-websocket-key:")
        {
            if let Some((_, val)) = line.split_once(':') {
                key = Some(val.trim().to_string());
                break;
            }
        }
    }

    let client_key = key.ok_or_else(|| "Missing Sec-WebSocket-Key in request".to_string())?;
    let accept_key = compute_accept_key(&client_key);

    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    );

    stream
        .write_all(resp.as_bytes())
        .map_err(|e| format!("Failed to send server handshake response: {}", e))?;
    stream
        .flush()
        .map_err(|e| format!("Failed to flush server handshake response: {}", e))?;

    Ok((path, client_key))
}

// ---------------------------------------------------------------------------
// Frame Encoding and Decoding
// ---------------------------------------------------------------------------

/// Encodes an RFC 6455 WebSocket frame into raw bytes.
pub fn encode_frame(
    fin: bool,
    opcode: u8,
    masked: bool,
    mask_key: Option<[u8; 4]>,
    payload: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::new();
    let b0 = (if fin { 0x80 } else { 0x00 }) | (opcode & 0x0F);
    frame.push(b0);

    let mask_bit = if masked { 0x80 } else { 0x00 };
    let len = payload.len();

    if len <= 125 {
        frame.push(mask_bit | (len as u8));
    } else if len <= 65535 {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(mask_bit | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    if masked {
        let key = mask_key.unwrap_or_else(|| fastrand::u32(..).to_ne_bytes());
        frame.extend_from_slice(&key);
        for (i, byte) in payload.iter().enumerate() {
            frame.push(byte ^ key[i % 4]);
        }
    } else {
        frame.extend_from_slice(payload);
    }

    frame
}

/// Decodes an RFC 6455 WebSocket frame from a byte slice.
pub fn decode_frame(bytes: &[u8]) -> Result<(bool, u8, Vec<u8>), String> {
    if bytes.len() < 2 {
        return Err("Frame too short for header".to_string());
    }

    let fin = (bytes[0] & 0x80) != 0;
    let rsv = (bytes[0] & 0x70) >> 4;
    let opcode = bytes[0] & 0x0F;
    if rsv != 0 {
        return Err(format!(
            "Non-zero RSV bits without negotiated extension: {}",
            rsv
        ));
    }
    let masked = (bytes[1] & 0x80) != 0;
    let initial_len = (bytes[1] & 0x7F) as usize;

    let mut cursor = 2;
    let payload_len = if initial_len <= 125 {
        initial_len
    } else if initial_len == 126 {
        if bytes.len() < cursor + 2 {
            return Err("Frame too short for 16-bit extended length".to_string());
        }
        let len = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2;
        len
    } else {
        if bytes.len() < cursor + 8 {
            return Err("Frame too short for 64-bit extended length".to_string());
        }
        let len_bytes: [u8; 8] = bytes[cursor..cursor + 8]
            .try_into()
            .map_err(|_| "Failed to slice 64-bit extended length".to_string())?;
        cursor += 8;
        u64::from_be_bytes(len_bytes) as usize
    };

    let mask_key = if masked {
        if bytes.len() < cursor + 4 {
            return Err("Frame too short for mask key".to_string());
        }
        let mask = [
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ];
        cursor += 4;
        Some(mask)
    } else {
        None
    };

    if bytes.len() < cursor + payload_len {
        return Err("Frame payload truncated".to_string());
    }

    let mut payload = bytes[cursor..cursor + payload_len].to_vec();
    if let Some(mask) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok((fin, opcode, payload))
}

/// Reads a single WebSocket frame synchronously from a `TcpStream`.
pub fn read_frame_stream(stream: &mut TcpStream) -> io::Result<WsFrame> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;

    let fin = (header[0] & 0x80) != 0;
    let rsv = (header[0] & 0x70) >> 4;
    let opcode = header[0] & 0x0F;
    if rsv != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Non-zero RSV bits without negotiated extension: {}", rsv),
        ));
    }
    let masked = (header[1] & 0x80) != 0;
    let initial_len = (header[1] & 0x7F) as usize;

    let payload_len: usize = if initial_len <= 125 {
        initial_len
    } else if initial_len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        u16::from_be_bytes(ext) as usize
    } else {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        u64::from_be_bytes(ext) as usize
    };

    let mask_key = if masked {
        let mut mask = [0u8; 4];
        stream.read_exact(&mut mask)?;
        Some(mask)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    if let Some(mask) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok(WsFrame {
        fin,
        opcode,
        masked,
        mask_key,
        payload,
    })
}

// ---------------------------------------------------------------------------
// Full-Duplex Connection Engine
// ---------------------------------------------------------------------------

pub struct WsReader {
    pub stream: TcpStream,
    pub role: Role,
    pub fragment_buf: Vec<u8>,
    pub fragment_opcode: Option<u8>,
}

pub struct WsWriter {
    pub stream: TcpStream,
    pub role: Role,
}

#[derive(Clone)]
pub struct WsConnection {
    pub id: u64,
    pub role: Role,
    pub url: String,
    pub reader: Arc<Mutex<WsReader>>,
    pub writer: Arc<Mutex<WsWriter>>,
    pub is_open: Arc<AtomicBool>,
}

impl WsConnection {
    pub fn new(id: u64, stream: TcpStream, role: Role, url: String) -> io::Result<Self> {
        let write_stream = stream.try_clone()?;
        let is_open = Arc::new(AtomicBool::new(true));

        Ok(Self {
            id,
            role,
            url,
            reader: Arc::new(Mutex::new(WsReader {
                stream,
                role,
                fragment_buf: Vec::new(),
                fragment_opcode: None,
            })),
            writer: Arc::new(Mutex::new(WsWriter {
                stream: write_stream,
                role,
            })),
            is_open,
        })
    }

    /// Connects a client WebSocket synchronously to a URL (RFC 6455).
    pub fn connect(url: &str, timeout: Option<Duration>) -> Result<Self, String> {
        let (host, port, path) = parse_ws_url(url)?;
        let addr_str = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr_str)
            .map_err(|e| format!("Failed to connect WebSocket to '{}': {}", addr_str, e))?;
        let timeout_duration = timeout.or(Some(Duration::from_secs(5)));
        let _ = stream.set_read_timeout(timeout_duration);
        let _ = stream.set_write_timeout(timeout_duration);
        let client_key = generate_key();
        perform_client_handshake(&mut stream, &host, port, &path, &client_key, None)?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let conn = WsConnection::new(id, stream, Role::Client, url.to_string())
            .map_err(|e| format!("Failed to initialize WebSocket client: {}", e))?;
        register_connection(conn.clone());
        Ok(conn)
    }

    pub fn is_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    pub fn write_frame_internal(
        &self,
        fin: bool,
        opcode: u8,
        payload: &[u8],
    ) -> Result<bool, String> {
        if !self.is_open() {
            return Err("WebSocket connection is closed".to_string());
        }
        let mut w = self.writer.lock().map_err(|e| e.to_string())?;
        let is_client = self.role == Role::Client;
        let mask = if is_client {
            Some(fastrand::u32(..).to_ne_bytes())
        } else {
            None
        };
        let frame = encode_frame(fin, opcode, is_client, mask, payload);
        w.stream
            .write_all(&frame)
            .map_err(|e| format!("Failed to write WebSocket frame: {}", e))?;
        w.stream
            .flush()
            .map_err(|e| format!("Failed to flush WebSocket frame: {}", e))?;
        Ok(true)
    }

    pub fn send(&self, text: &str) -> Result<bool, String> {
        self.write_frame_internal(true, OPCODE_TEXT, text.as_bytes())
    }

    pub fn send_binary(&self, bytes: &[u8]) -> Result<bool, String> {
        self.write_frame_internal(true, OPCODE_BINARY, bytes)
    }

    pub fn ping(&self, payload: &[u8]) -> Result<bool, String> {
        if payload.len() > 125 {
            return Err("Control frame payload must be <= 125 bytes".to_string());
        }
        self.write_frame_internal(true, OPCODE_PING, payload)
    }

    pub fn pong(&self, payload: &[u8]) -> Result<bool, String> {
        if payload.len() > 125 {
            return Err("Control frame payload must be <= 125 bytes".to_string());
        }
        self.write_frame_internal(true, OPCODE_PONG, payload)
    }

    pub fn close(&self, code: Option<u16>, reason: Option<&str>) -> Result<bool, String> {
        if !self.is_open.swap(false, Ordering::SeqCst) {
            return Ok(false);
        }

        let mut payload = Vec::new();
        let status = code.unwrap_or(CLOSE_NORMAL);
        payload.extend_from_slice(&status.to_be_bytes());
        if let Some(r) = reason {
            payload.extend_from_slice(r.as_bytes());
        }

        // Send Close frame
        let is_client = self.role == Role::Client;
        let mask = if is_client {
            Some(fastrand::u32(..).to_ne_bytes())
        } else {
            None
        };
        let frame = encode_frame(true, OPCODE_CLOSE, is_client, mask, &payload);

        if let Ok(mut w) = self.writer.lock() {
            let _ = w.stream.write_all(&frame);
            let _ = w.stream.flush();
            let _ = w.stream.shutdown(Shutdown::Both);
        }

        if let Ok(r) = self.reader.try_lock() {
            let _ = r.stream.shutdown(Shutdown::Both);
        }

        Ok(true)
    }

    pub fn recv(&self, timeout_ms: Option<u64>) -> Result<Option<WsMessage>, String> {
        let mut r = self.reader.lock().map_err(|e| e.to_string())?;
        if !self.is_open() {
            return Ok(None);
        }

        r.stream
            .set_read_timeout(timeout_ms.map(Duration::from_millis))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;

        loop {
            let frame = match read_frame_stream(&mut r.stream) {
                Ok(f) => f,
                Err(ref e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    return Ok(None);
                }
                Err(ref e)
                    if e.kind() == io::ErrorKind::UnexpectedEof
                        || e.kind() == io::ErrorKind::ConnectionReset =>
                {
                    self.is_open.store(false, Ordering::SeqCst);
                    return Ok(None);
                }
                Err(e) => {
                    return Err(format!("WebSocket read error: {}", e));
                }
            };

            // RFC 6455 §5.1: A server MUST close connection upon receiving an unmasked client frame
            if self.role == Role::Server && !frame.masked {
                let _ = self.close(Some(CLOSE_PROTOCOL_ERROR), Some("Unmasked client frame"));
                return Ok(Some(WsMessage::Close(
                    CLOSE_PROTOCOL_ERROR,
                    "Unmasked client frame".to_string(),
                )));
            }

            // RFC 6455 §5.1: A client MUST close connection upon receiving a masked frame from server
            if self.role == Role::Client && frame.masked {
                let _ = self.close(Some(CLOSE_PROTOCOL_ERROR), Some("Masked server frame"));
                return Ok(Some(WsMessage::Close(
                    CLOSE_PROTOCOL_ERROR,
                    "Masked server frame".to_string(),
                )));
            }

            // RFC 6455 §5.5: Control frames MUST have a payload length <= 125 bytes and MUST NOT be fragmented (FIN=1)
            if frame.opcode == OPCODE_PING
                || frame.opcode == OPCODE_PONG
                || frame.opcode == OPCODE_CLOSE
            {
                if frame.payload.len() > 125 {
                    let _ = self.close(
                        Some(CLOSE_PROTOCOL_ERROR),
                        Some("Control frame payload > 125"),
                    );
                    return Ok(Some(WsMessage::Close(
                        CLOSE_PROTOCOL_ERROR,
                        "Control frame payload > 125".to_string(),
                    )));
                }
                if !frame.fin {
                    let _ =
                        self.close(Some(CLOSE_PROTOCOL_ERROR), Some("Fragmented control frame"));
                    return Ok(Some(WsMessage::Close(
                        CLOSE_PROTOCOL_ERROR,
                        "Fragmented control frame".to_string(),
                    )));
                }
            }

            // Dispatch on opcode
            match frame.opcode {
                OPCODE_PING => {
                    // Control frames RFC 6455: reply with Pong containing identical payload
                    let _ = self.pong(&frame.payload);
                    continue;
                }
                OPCODE_PONG => {
                    // Silently ignore Pong response and continue reading next message
                    continue;
                }
                OPCODE_CLOSE => {
                    self.is_open.store(false, Ordering::SeqCst);
                    let code = if frame.payload.len() >= 2 {
                        u16::from_be_bytes([frame.payload[0], frame.payload[1]])
                    } else {
                        CLOSE_NORMAL
                    };
                    let reason = if frame.payload.len() > 2 {
                        String::from_utf8_lossy(&frame.payload[2..]).to_string()
                    } else {
                        String::new()
                    };

                    // Send close response frame echoing the code
                    let reply_code_bytes = code.to_be_bytes();
                    let is_client = self.role == Role::Client;
                    let mask = if is_client {
                        Some(fastrand::u32(..).to_ne_bytes())
                    } else {
                        None
                    };
                    let reply_frame =
                        encode_frame(true, OPCODE_CLOSE, is_client, mask, &reply_code_bytes);
                    if let Ok(mut w) = self.writer.lock() {
                        let _ = w.stream.write_all(&reply_frame);
                        let _ = w.stream.flush();
                        let _ = w.stream.shutdown(Shutdown::Both);
                    }
                    let _ = r.stream.shutdown(Shutdown::Both);
                    return Ok(Some(WsMessage::Close(code, reason)));
                }
                OPCODE_TEXT | OPCODE_BINARY => {
                    if frame.fin {
                        if frame.opcode == OPCODE_TEXT {
                            let text = String::from_utf8(frame.payload)
                                .map_err(|e| format!("Invalid UTF-8 in text frame: {}", e))?;
                            return Ok(Some(WsMessage::Text(text)));
                        } else {
                            return Ok(Some(WsMessage::Binary(frame.payload)));
                        }
                    } else {
                        // Begin fragmented frame assembly
                        r.fragment_opcode = Some(frame.opcode);
                        r.fragment_buf = frame.payload;
                        continue;
                    }
                }
                OPCODE_CONTINUATION => {
                    r.fragment_buf.extend_from_slice(&frame.payload);
                    if frame.fin {
                        let op = r.fragment_opcode.take().unwrap_or(OPCODE_TEXT);
                        let full_payload = std::mem::take(&mut r.fragment_buf);
                        if op == OPCODE_TEXT {
                            let text = String::from_utf8(full_payload).map_err(|e| {
                                format!("Invalid UTF-8 in assembled text frame: {}", e)
                            })?;
                            return Ok(Some(WsMessage::Text(text)));
                        } else {
                            return Ok(Some(WsMessage::Binary(full_payload)));
                        }
                    } else {
                        continue;
                    }
                }
                other => {
                    return Err(format!("Unsupported WebSocket opcode: 0x{:X}", other));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Server Engine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WsServer {
    pub id: u64,
    pub listener: Arc<TcpListener>,
    pub is_closed: Arc<AtomicBool>,
    pub local_addr: SocketAddr,
}

impl WsServer {
    pub fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        Ok(Self {
            id,
            listener: Arc::new(listener),
            is_closed: Arc::new(AtomicBool::new(false)),
            local_addr,
        })
    }

    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn close(&self) -> Result<bool, String> {
        self.is_closed.store(true, Ordering::SeqCst);
        Ok(true)
    }

    pub fn accept(&self, timeout_ms: Option<u64>) -> Result<Option<WsConnection>, String> {
        if self.is_closed.load(Ordering::SeqCst) {
            return Err("WebSocket server is closed".to_string());
        }

        let start = std::time::Instant::now();
        let timeout = timeout_ms.map(Duration::from_millis);

        loop {
            if self.is_closed.load(Ordering::SeqCst) {
                return Ok(None);
            }

            match self.listener.accept() {
                Ok((mut stream, _peer_addr)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

                    match perform_server_handshake(&mut stream) {
                        Ok((_path, _key)) => {
                            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                            let conn = WsConnection::new(
                                id,
                                stream,
                                Role::Server,
                                format!("ws://{}", self.local_addr),
                            )
                            .map_err(|e| {
                                format!("Failed to initialize server connection: {}", e)
                            })?;

                            register_connection(conn.clone());
                            return Ok(Some(conn));
                        }
                        Err(_err) => {
                            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
                            let _ = stream.flush();
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if let Some(t) = timeout {
                        if start.elapsed() >= t {
                            return Ok(None);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => {
                    return Err(format!("Server accept error: {}", e));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global Registries
// ---------------------------------------------------------------------------

static CONNECTIONS: OnceLock<RwLock<HashMap<u64, WsConnection>>> = OnceLock::new();
static SERVERS: OnceLock<RwLock<HashMap<u64, WsServer>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn get_connection(id: u64) -> Option<WsConnection> {
    CONNECTIONS
        .get_or_init(|| RwLock::new(HashMap::new()))
        .read()
        .unwrap()
        .get(&id)
        .cloned()
}

pub fn register_connection(conn: WsConnection) -> u64 {
    let id = conn.id;
    CONNECTIONS
        .get_or_init(|| RwLock::new(HashMap::new()))
        .write()
        .unwrap()
        .insert(id, conn);
    id
}

pub fn remove_connection(id: u64) {
    if let Some(lock) = CONNECTIONS.get() {
        lock.write().unwrap().remove(&id);
    }
}

pub fn get_server(id: u64) -> Option<WsServer> {
    SERVERS
        .get_or_init(|| RwLock::new(HashMap::new()))
        .read()
        .unwrap()
        .get(&id)
        .cloned()
}

pub fn register_server(server: WsServer) -> u64 {
    let id = server.id;
    SERVERS
        .get_or_init(|| RwLock::new(HashMap::new()))
        .write()
        .unwrap()
        .insert(id, server);
    id
}

// ---------------------------------------------------------------------------
// Value Representation & Method Binding Helpers
// ---------------------------------------------------------------------------

pub fn make_connection_value(conn: &WsConnection) -> Value {
    let mut map = IndexMap::new();
    map.insert("_ws_conn_id".to_string(), Value::Int(conn.id as i64));
    map.insert("id".to_string(), Value::Int(conn.id as i64));
    map.insert("url".to_string(), Value::String(conn.url.clone()));
    Value::Map(Arc::new(RwLock::new(map)))
}

pub fn make_server_value(server: &WsServer) -> Value {
    let mut map = IndexMap::new();
    map.insert("_ws_server_id".to_string(), Value::Int(server.id as i64));
    map.insert("id".to_string(), Value::Int(server.id as i64));
    map.insert("port".to_string(), Value::Int(server.port() as i64));
    map.insert(
        "local_addr".to_string(),
        Value::String(server.local_addr().to_string()),
    );
    map.insert(
        "localAddr".to_string(),
        Value::String(server.local_addr().to_string()),
    );
    Value::Map(Arc::new(RwLock::new(map)))
}

fn extract_conn_id(val: &Value, span: Span) -> Result<u64, RuntimeError> {
    match val {
        Value::Int(id) if *id >= 0 => Ok(*id as u64),
        Value::Map(m) => {
            let guard = m.read().unwrap();
            if let Some(Value::Int(id)) = guard.get("_ws_conn_id").or_else(|| guard.get("id")) {
                Ok(*id as u64)
            } else {
                Err(RuntimeError::new(
                    "Expected WebSocket connection map with '_ws_conn_id'".into(),
                )
                .at(span))
            }
        }
        other => Err(RuntimeError::new(format!(
            "Expected WebSocket connection or connection ID, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn extract_server_id(val: &Value, span: Span) -> Result<u64, RuntimeError> {
    match val {
        Value::Int(id) if *id >= 0 => Ok(*id as u64),
        Value::Map(m) => {
            let guard = m.read().unwrap();
            if let Some(Value::Int(id)) = guard.get("_ws_server_id").or_else(|| guard.get("id")) {
                Ok(*id as u64)
            } else {
                Err(
                    RuntimeError::new("Expected WebSocket server map with '_ws_server_id'".into())
                        .at(span),
                )
            }
        }
        other => Err(RuntimeError::new(format!(
            "Expected WebSocket server or server ID, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

// ---------------------------------------------------------------------------
// Evaluator & VM BoundMethod Dispatch Helpers
// ---------------------------------------------------------------------------

pub fn eval_ws_conn_method(
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let conn_id = {
        let guard = map.read().unwrap();
        match guard.get("_ws_conn_id") {
            Some(Value::Int(id)) => *id as u64,
            _ => {
                return Err(
                    RuntimeError::new("Invalid WebSocket connection handle".into()).at(span),
                );
            }
        }
    };

    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or already closed",
            conn_id
        ))
        .at(span)
    })?;

    match method {
        "send" => {
            if args.is_empty() {
                return Err(
                    RuntimeError::new("send() expects at least 1 argument".into()).at(span),
                );
            }
            match &args[0] {
                Value::String(s) => conn
                    .send(s)
                    .map(Value::Bool)
                    .map_err(|e| RuntimeError::new(e).at(span)),
                Value::ByteBuffer(b) => {
                    let bytes = b.read().unwrap().to_bytes();
                    conn.send_binary(&bytes)
                        .map(Value::Bool)
                        .map_err(|e| RuntimeError::new(e).at(span))
                }
                Value::Array(arr) => {
                    let guard = arr.read().unwrap();
                    let mut bytes = Vec::with_capacity(guard.len());
                    for v in guard.iter() {
                        if let Value::Int(b) = v {
                            bytes.push(*b as u8);
                        }
                    }
                    conn.send_binary(&bytes)
                        .map(Value::Bool)
                        .map_err(|e| RuntimeError::new(e).at(span))
                }
                other => Err(RuntimeError::new(format!(
                    "send() expects string or buffer, got {}",
                    other.type_name()
                ))
                .at(span)),
            }
        }
        "send_binary" | "sendBinary" => {
            if args.is_empty() {
                return Err(RuntimeError::new("send_binary() expects 1 argument".into()).at(span));
            }
            match &args[0] {
                Value::ByteBuffer(b) => {
                    let bytes = b.read().unwrap().to_bytes();
                    conn.send_binary(&bytes)
                        .map(Value::Bool)
                        .map_err(|e| RuntimeError::new(e).at(span))
                }
                Value::String(s) => conn
                    .send_binary(s.as_bytes())
                    .map(Value::Bool)
                    .map_err(|e| RuntimeError::new(e).at(span)),
                Value::Array(arr) => {
                    let guard = arr.read().unwrap();
                    let mut bytes = Vec::with_capacity(guard.len());
                    for v in guard.iter() {
                        if let Value::Int(b) = v {
                            bytes.push(*b as u8);
                        }
                    }
                    conn.send_binary(&bytes)
                        .map(Value::Bool)
                        .map_err(|e| RuntimeError::new(e).at(span))
                }
                other => Err(RuntimeError::new(format!(
                    "send_binary() expects buffer or string, got {}",
                    other.type_name()
                ))
                .at(span)),
            }
        }
        "recv" => {
            let timeout_ms = if !args.is_empty() {
                match &args[0] {
                    Value::Int(ms) if *ms >= 0 => Some(*ms as u64),
                    Value::Float(ms) | Value::Number(ms) if *ms >= 0.0 => Some(*ms as u64),
                    _ => None,
                }
            } else {
                None
            };
            match conn.recv(timeout_ms) {
                Ok(Some(WsMessage::Text(s))) => Ok(Value::String(s)),
                Ok(Some(WsMessage::Binary(bytes))) => Ok(Value::ByteBuffer(Arc::new(RwLock::new(
                    ByteBuffer::from_vec(bytes),
                )))),
                Ok(Some(WsMessage::Close(..))) | Ok(None) => Ok(Value::Null),
                Err(e) => Err(RuntimeError::new(e).at(span)),
                _ => Ok(Value::Null),
            }
        }
        "ping" => {
            let payload = if !args.is_empty() {
                match &args[0] {
                    Value::String(s) => s.as_bytes().to_vec(),
                    Value::ByteBuffer(b) => b.read().unwrap().to_bytes(),
                    _ => b"ping".to_vec(),
                }
            } else {
                b"ping".to_vec()
            };
            conn.ping(&payload)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        "close" => {
            let code = if !args.is_empty() {
                match &args[0] {
                    Value::Int(c) => Some(*c as u16),
                    _ => None,
                }
            } else {
                None
            };
            let reason = if args.len() > 1 {
                match &args[1] {
                    Value::String(r) => Some(r.as_str()),
                    _ => None,
                }
            } else {
                None
            };
            conn.close(code, reason)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        "is_open" | "isOpen" => Ok(Value::Bool(conn.is_open())),
        _ => Err(
            RuntimeError::new(format!("Method '{}' not found on WsConnection", method)).at(span),
        ),
    }
}

pub fn eval_ws_server_method(
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let server_id = {
        let guard = map.read().unwrap();
        match guard.get("_ws_server_id") {
            Some(Value::Int(id)) => *id as u64,
            _ => return Err(RuntimeError::new("Invalid WebSocket server handle".into()).at(span)),
        }
    };

    let server = get_server(server_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket server #{} not found or already closed",
            server_id
        ))
        .at(span)
    })?;

    match method {
        "accept" => {
            let timeout_ms = if !args.is_empty() {
                match &args[0] {
                    Value::Int(ms) if *ms >= 0 => Some(*ms as u64),
                    Value::Float(ms) | Value::Number(ms) if *ms >= 0.0 => Some(*ms as u64),
                    _ => None,
                }
            } else {
                None
            };
            match server.accept(timeout_ms) {
                Ok(Some(conn)) => Ok(make_connection_value(&conn)),
                Ok(None) => Ok(Value::Null),
                Err(e) => Err(RuntimeError::new(e).at(span)),
            }
        }
        "close" => server
            .close()
            .map(Value::Bool)
            .map_err(|e| RuntimeError::new(e).at(span)),
        "port" => Ok(Value::Int(server.port() as i64)),
        "local_addr" | "localAddr" => Ok(Value::String(server.local_addr().to_string())),
        _ => Err(RuntimeError::new(format!("Method '{}' not found on WsServer", method)).at(span)),
    }
}

// ---------------------------------------------------------------------------
// Standard Library Module Builtins: std:ws
// ---------------------------------------------------------------------------

pub fn ws_connect(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("ws.connect expects at least 1 argument: url".into()).at(span),
        );
    }

    let url = match &args[0] {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::new(format!(
                "ws.connect expects string URL, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let (host, port, path) = parse_ws_url(&url).map_err(|e| RuntimeError::new(e).at(span))?;
    let addr_str = format!("{}:{}", host, port);

    let mut stream = TcpStream::connect(&addr_str).map_err(|e| {
        RuntimeError::new(format!(
            "Failed to connect WebSocket to '{}': {}",
            addr_str, e
        ))
        .at(span)
    })?;

    let timeout_duration = if args.len() > 1 {
        match &args[1] {
            Value::Map(m) => {
                let guard = m.read().unwrap();
                if let Some(Value::Int(ms)) = guard.get("timeout") {
                    Some(Duration::from_millis(*ms as u64))
                } else {
                    Some(Duration::from_secs(5))
                }
            }
            _ => Some(Duration::from_secs(5)),
        }
    } else {
        Some(Duration::from_secs(5))
    };

    stream
        .set_read_timeout(timeout_duration)
        .map_err(|e| RuntimeError::new(e.to_string()).at(span))?;
    stream
        .set_write_timeout(timeout_duration)
        .map_err(|e| RuntimeError::new(e.to_string()).at(span))?;

    let client_key = generate_key();
    perform_client_handshake(&mut stream, &host, port, &path, &client_key, None)
        .map_err(|e| RuntimeError::new(e).at(span))?;

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let conn = WsConnection::new(id, stream, Role::Client, url).map_err(|e| {
        RuntimeError::new(format!("Failed to initialize WebSocket client: {}", e)).at(span)
    })?;

    register_connection(conn.clone());
    Ok(make_connection_value(&conn))
}

pub fn ws_listen(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new(
            "ws.listen expects at least 1 argument (port or address)".into(),
        )
        .at(span));
    }

    let addr_str = match &args[0] {
        Value::Int(p) => format!("127.0.0.1:{}", p),
        Value::String(s) => {
            if s.parse::<u16>().is_ok() {
                format!("127.0.0.1:{}", s)
            } else if s.starts_with(':') {
                format!("127.0.0.1{}", s)
            } else {
                s.clone()
            }
        }
        other => {
            return Err(RuntimeError::new(format!(
                "ws.listen expects port number or address string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let server = WsServer::bind(&addr_str).map_err(|e| {
        RuntimeError::new(format!(
            "Failed to bind WebSocket server to '{}': {}",
            addr_str, e
        ))
        .at(span)
    })?;

    register_server(server.clone());

    // Optional handler callback
    if args.len() > 1 && args[1].is_callable() {
        let handler = args[1].clone();
        let s_clone = server.clone();
        std::thread::spawn(move || {
            while !s_clone.is_closed.load(Ordering::Relaxed) {
                if let Ok(Some(client)) = s_clone.accept(Some(500)) {
                    let client_val = make_connection_value(&client);
                    let mut ev = Evaluator::new();
                    let _ = ev.call_value(&handler, vec![client_val], Span::new(1, 1));
                }
            }
        });
    }

    Ok(make_server_value(&server))
}

pub fn ws_send(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.send expects (conn, data)".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or closed",
            conn_id
        ))
        .at(span)
    })?;

    if args.len() < 2 {
        return Err(RuntimeError::new("ws.send expects data argument".into()).at(span));
    }

    match &args[1] {
        Value::String(s) => conn
            .send(s)
            .map(Value::Bool)
            .map_err(|e| RuntimeError::new(e).at(span)),
        Value::ByteBuffer(b) => {
            let bytes = b.read().unwrap().to_bytes();
            conn.send_binary(&bytes)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut bytes = Vec::with_capacity(guard.len());
            for v in guard.iter() {
                if let Value::Int(b) = v {
                    bytes.push(*b as u8);
                }
            }
            conn.send_binary(&bytes)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        other => Err(RuntimeError::new(format!(
            "ws.send expects string or buffer data, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn ws_send_binary(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("ws.send_binary expects (conn, bytes)".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or closed",
            conn_id
        ))
        .at(span)
    })?;

    match &args[1] {
        Value::ByteBuffer(b) => {
            let bytes = b.read().unwrap().to_bytes();
            conn.send_binary(&bytes)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        Value::String(s) => conn
            .send_binary(s.as_bytes())
            .map(Value::Bool)
            .map_err(|e| RuntimeError::new(e).at(span)),
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut bytes = Vec::with_capacity(guard.len());
            for v in guard.iter() {
                if let Value::Int(b) = v {
                    bytes.push(*b as u8);
                }
            }
            conn.send_binary(&bytes)
                .map(Value::Bool)
                .map_err(|e| RuntimeError::new(e).at(span))
        }
        other => Err(RuntimeError::new(format!(
            "ws.send_binary expects buffer or string, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn ws_recv(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.recv expects (conn, [timeout_ms])".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or closed",
            conn_id
        ))
        .at(span)
    })?;

    let timeout_ms = if args.len() > 1 {
        match &args[1] {
            Value::Int(ms) if *ms >= 0 => Some(*ms as u64),
            Value::Float(ms) | Value::Number(ms) if *ms >= 0.0 => Some(*ms as u64),
            _ => None,
        }
    } else {
        None
    };

    match conn.recv(timeout_ms) {
        Ok(Some(WsMessage::Text(s))) => Ok(Value::String(s)),
        Ok(Some(WsMessage::Binary(bytes))) => Ok(Value::ByteBuffer(Arc::new(RwLock::new(
            ByteBuffer::from_vec(bytes),
        )))),
        Ok(Some(WsMessage::Close(..))) | Ok(None) => Ok(Value::Null),
        Err(e) => Err(RuntimeError::new(e).at(span)),
        _ => Ok(Value::Null),
    }
}

pub fn ws_ping(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.ping expects (conn, [payload])".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or closed",
            conn_id
        ))
        .at(span)
    })?;

    let payload = if args.len() > 1 {
        match &args[1] {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::ByteBuffer(b) => b.read().unwrap().to_bytes(),
            _ => b"ping".to_vec(),
        }
    } else {
        b"ping".to_vec()
    };

    conn.ping(&payload)
        .map(Value::Bool)
        .map_err(|e| RuntimeError::new(e).at(span))
}

pub fn ws_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.close expects (conn, [code], [reason])".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    let conn = get_connection(conn_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket connection #{} not found or closed",
            conn_id
        ))
        .at(span)
    })?;

    let code = if args.len() > 1 {
        match &args[1] {
            Value::Int(c) => Some(*c as u16),
            _ => None,
        }
    } else {
        None
    };

    let reason = if args.len() > 2 {
        match &args[2] {
            Value::String(r) => Some(r.as_str()),
            _ => None,
        }
    } else {
        None
    };

    conn.close(code, reason)
        .map(Value::Bool)
        .map_err(|e| RuntimeError::new(e).at(span))
}

pub fn ws_is_open(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.is_open expects conn argument".into()).at(span));
    }

    let conn_id = extract_conn_id(&args[0], span)?;
    if let Some(conn) = get_connection(conn_id) {
        Ok(Value::Bool(conn.is_open()))
    } else {
        Ok(Value::Bool(false))
    }
}

pub fn ws_accept(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.accept expects server argument".into()).at(span));
    }

    let server_id = extract_server_id(&args[0], span)?;
    let server = get_server(server_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket server #{} not found or closed",
            server_id
        ))
        .at(span)
    })?;

    let timeout_ms = if args.len() > 1 {
        match &args[1] {
            Value::Int(ms) if *ms >= 0 => Some(*ms as u64),
            Value::Float(ms) | Value::Number(ms) if *ms >= 0.0 => Some(*ms as u64),
            _ => None,
        }
    } else {
        None
    };

    match server.accept(timeout_ms) {
        Ok(Some(conn)) => Ok(make_connection_value(&conn)),
        Ok(None) => Ok(Value::Null),
        Err(e) => Err(RuntimeError::new(e).at(span)),
    }
}

pub fn ws_server_close(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ws.server_close expects server argument".into()).at(span));
    }

    let server_id = extract_server_id(&args[0], span)?;
    let server = get_server(server_id).ok_or_else(|| {
        RuntimeError::new(format!(
            "WebSocket server #{} not found or closed",
            server_id
        ))
        .at(span)
    })?;

    server
        .close()
        .map(Value::Bool)
        .map_err(|e| RuntimeError::new(e).at(span))
}

pub fn build_ws_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, BuiltinFn)> = vec![
        ("connect", ws_connect),
        ("listen", ws_listen),
        ("send", ws_send),
        ("send_binary", ws_send_binary),
        ("sendBinary", ws_send_binary),
        ("recv", ws_recv),
        ("ping", ws_ping),
        ("close", ws_close),
        ("is_open", ws_is_open),
        ("isOpen", ws_is_open),
        ("accept", ws_accept),
        ("server_close", ws_server_close),
        ("serverClose", ws_server_close),
    ];
    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: name.to_string(),
                func,
            },
        );
    }
    Value::Map(Arc::new(RwLock::new(map)))
}
