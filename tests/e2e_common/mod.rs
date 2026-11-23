#![allow(dead_code, unused_imports)]

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ring::digest::{self, SHA1_FOR_LEGACY_USE_ONLY, SHA256};
use shae::compiler::Compiler;
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Returns path to the compiled `shae` CLI binary.
pub fn shae_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_shae"))
}

/// Creates a `Command` configured to invoke the `shae` CLI binary.
pub fn shae_cmd() -> Command {
    Command::new(shae_bin())
}

/// Executes `shae` with CLI arguments and returns (success, stdout, stderr).
pub fn run_cli(args: &[&str]) -> (bool, String, String) {
    let output = shae_cmd()
        .args(args)
        .output()
        .expect("Failed to execute shae CLI binary");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Finds and binds an ephemeral free TCP port on 127.0.0.1.
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral TCP port");
    listener.local_addr().unwrap().port()
}

/// Dynamically allocates an available ephemeral TCP port on localhost.
pub fn get_available_port() -> u16 {
    free_port()
}

/// Temporary Shae script file that cleans up automatically on drop.
pub struct TempScript {
    pub path: PathBuf,
}

impl TempScript {
    pub fn new(name_prefix: &str, content: &str) -> Self {
        let unique_id = fastrand::u64(..);
        let path =
            std::env::temp_dir().join(format!("shae_e2e_{}_{}.shae", name_prefix, unique_id));
        fs::write(&path, content).expect("Failed to write temp shae script");
        Self { path }
    }
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Temporary Shae project workspace with shae.toml and source files.
pub struct TempProject {
    pub dir: PathBuf,
}

impl TempProject {
    pub fn new(name: &str) -> Self {
        let unique_id = fastrand::u64(..);
        let dir = std::env::temp_dir().join(format!("shae_e2e_proj_{}_{}", name, unique_id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).expect("Failed to create project dir");
        Self { dir }
    }

    pub fn write_manifest(&self, name: &str, version: &str) {
        let content = format!(
            r#"[package]
name = "{}"
version = "{}"
entry = "src/main.shae"

[dependencies]
"#,
            name, version
        );
        fs::write(self.dir.join("shae.toml"), content).expect("Failed to write shae.toml");
    }

    pub fn write_file(&self, rel_path: &str, content: &str) {
        let target = self.dir.join(rel_path);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(target, content).expect("Failed to write project file");
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// In-process bytecode VM executor for Shae source code.
pub fn run_script_vm(code: &str) -> Result<Value, String> {
    let program = parse_source(code).map_err(|e| format!("Parse error: {:?}", e))?;
    let chunk = Compiler::new()
        .compile_program(&program)
        .map_err(|e| format!("Compile error: {:?}", e))?;
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => Ok(val),
        InterpretResult::RuntimeError(err) => Err(format!("Runtime error: {}", err)),
        InterpretResult::CompileError => Err("Compile error in VM".to_string()),
    }
}

/// In-process Cranelift JIT executor for Shae source code.
pub fn run_script_jit(code: &str) -> Result<Value, String> {
    shae::run_jit(code).map_err(|e| format!("{:?}", e))
}

/// Checks whether `std:ws` module is available in the current runtime build.
pub fn is_ws_module_available() -> bool {
    let test_script = r#"
use { connect, listen } from "std:ws"
let ws = use "std:ws"
ws != null
"#;
    shae::run(test_script).is_ok()
}

/// Asserts that `std:ws` module is registered in the Shae runtime.
/// Emits an actionable failure if Milestone 2 has not registered the module.
pub fn require_ws_module() {
    assert!(
        is_ws_module_available(),
        "CRITICAL: std:ws module (R2) is not registered in src/stdlib.rs.          Cannot verify WebSocket E2E contract without Milestone 2 implementation."
    );
}

/// Computes the RFC 6455 Sec-WebSocket-Accept key from Sec-WebSocket-Key.
pub fn compute_ws_accept(key: &str) -> String {
    let combined = format!("{}{}", key.trim(), WS_GUID);
    let sha1 = digest::digest(&SHA1_FOR_LEGACY_USE_ONLY, combined.as_bytes());
    BASE64_STANDARD.encode(sha1.as_ref())
}

/// Computes SHA-256 hexadecimal string.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let hash = digest::digest(&SHA256, bytes);
    let mut s = String::with_capacity(64);
    for b in hash.as_ref() {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Encodes a DAP JSON-RPC message with standard Content-Length header.
pub fn encode_dap_message(msg: &serde_json::Value) -> Vec<u8> {
    let payload = serde_json::to_string(msg).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload).into_bytes()
}

/// Thread-safe in-memory writer for DAP and network stream capturing.
#[derive(Clone)]
pub struct SharedWriter(pub Arc<Mutex<Vec<u8>>>);

impl SharedWriter {
    pub fn new(buf: Arc<Mutex<Vec<u8>>>) -> Self {
        SharedWriter(buf)
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Parses sequenced DAP JSON-RPC responses from raw output bytes.
pub fn parse_dap_responses(output: &[u8]) -> Vec<serde_json::Value> {
    let mut responses = Vec::new();
    let mut cursor = 0;
    while cursor < output.len() {
        let text = String::from_utf8_lossy(&output[cursor..]);
        if let Some(header_end) = text.find("\r\n\r\n") {
            let header = &text[..header_end];
            let len: usize = header
                .lines()
                .find_map(|l| {
                    if l.starts_with("Content-Length:") {
                        l["Content-Length:".len()..].trim().parse().ok()
                    } else {
                        None
                    }
                })
                .expect("Valid Content-Length header");
            let body_start = cursor + header_end + 4;
            let body_end = body_start + len;
            if body_end <= output.len() {
                let parsed: serde_json::Value =
                    serde_json::from_slice(&output[body_start..body_end]).unwrap();
                responses.push(parsed);
                cursor = body_end;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    responses
}

pub struct MockRegistry {
    pub url: String,
    routes: Arc<Mutex<BTreeMap<String, (u16, String, Vec<u8>)>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockRegistry {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock registry");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        let routes: Arc<Mutex<BTreeMap<String, (u16, String, Vec<u8>)>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let r_clone = Arc::clone(&routes);
        let s_clone = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            let _ = listener.set_nonblocking(true);
            while !s_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 8192];
                        if let Ok(n) = stream.read(&mut buf) {
                            if n > 0 {
                                let req = String::from_utf8_lossy(&buf[..n]);
                                let mut lines = req.lines();
                                let req_line = lines.next().unwrap_or("");
                                let mut parts = req_line.split_whitespace();
                                let method = parts.next().unwrap_or("GET");
                                let path = parts.next().unwrap_or("/");

                                let route_key = format!("{} {}", method, path);
                                let routes_guard = r_clone.lock().unwrap();
                                if let Some((code, mime, body)) = routes_guard.get(&route_key) {
                                    let resp = format!(
                                        "HTTP/1.1 {} OK
Content-Type: {}
Content-Length: {}
Connection: close

",
                                        code,
                                        mime,
                                        body.len()
                                    );
                                    let _ = stream.write_all(resp.as_bytes());
                                    let _ = stream.write_all(body);
                                } else {
                                    let not_found = b"HTTP/1.1 404 Not Found
Content-Length: 0
Connection: close

";
                                    let _ = stream.write_all(not_found);
                                }
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            url,
            routes,
            shutdown,
            handle: Some(handle),
        }
    }

    pub fn add_route(&self, method: &str, path: &str, code: u16, mime: &str, body: Vec<u8>) {
        let key = format!("{} {}", method, path);
        self.routes
            .lock()
            .unwrap()
            .insert(key, (code, mime.to_string(), body));
    }
}

impl Drop for MockRegistry {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Background fixture for running Shae Studio IDE server.
pub struct IdeServerFixture {
    pub port: u16,
    pub workspace_dir: PathBuf,
    shutdown: Arc<AtomicBool>,
}

impl IdeServerFixture {
    pub fn start() -> Self {
        let port = free_port();
        let unique_id = fastrand::u64(..);
        let workspace_dir = std::env::temp_dir().join(format!("shae_ide_fixture_{}", unique_id));
        let _ = fs::create_dir_all(&workspace_dir);
        fs::write(
            workspace_dir.join("main.shae"),
            "let x = 42
return x
",
        )
        .expect("Failed to write initial main.shae");

        let shutdown = Arc::new(AtomicBool::new(false));
        let ws_dir_clone = workspace_dir.clone();

        thread::spawn(move || {
            let _ = shae::ide::start_ide_server(&ws_dir_clone, "127.0.0.1", port, false);
        });

        // Wait for server to start responding
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{}/", port);
        for _ in 0..50 {
            if client.get(&url).send().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        Self {
            port,
            workspace_dir,
            shutdown,
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for IdeServerFixture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = fs::remove_dir_all(&self.workspace_dir);
    }
}
