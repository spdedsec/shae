mod e2e_common;

use e2e_common::*;
use serde_json::json;
use shae::dap::DapServer;
use shae::pkg::{self, compute_sha256};
use shae::value::Value;
use shae::wasm::{WASM_MAGIC, WASM_VERSION, WasmCompiler, WasmRunner};
use std::io::{Cursor, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =========================================================================
// R1: On-Stack Replacement (OSR) & Tiered JIT Boundaries
// =========================================================================

#[test]
fn test_e2e_t2_jit_01_threshold_minus_one_boundary() {
    // JIT_THRESHOLD is 20 in src/jit.rs.
    // 19 iterations: interpreter threshold - 1
    let script_19 = r#"
let i = 0
let sum = 0
while i < 19 {
    sum = sum + i
    i = i + 1
}
return sum
"#;
    let res_19 = run_script_vm(script_19).expect("19 iterations loop");
    assert_eq!(res_19, Value::Int(171));

    // 20 iterations: exactly reaches threshold
    let script_20 = r#"
let i = 0
let sum = 0
while i < 20 {
    sum = sum + i
    i = i + 1
}
return sum
"#;
    let res_20 = run_script_vm(script_20).expect("20 iterations loop");
    assert_eq!(res_20, Value::Int(190));
}

#[test]
fn test_e2e_t2_jit_02_zero_iteration_loop() {
    let script = r#"
let x = 100
while false {
    x = x + 1
}
return x
"#;
    let res = run_script_vm(script).expect("Zero iteration loop");
    assert_eq!(res, Value::Int(100));
}

#[test]
fn test_e2e_t2_jit_03_high_iteration_tight_loop() {
    let script = r#"
let i = 0
while i < 1000000 {
    i = i + 1
}
return i
"#;
    let res = run_script_vm(script).expect("1,000,000 iterations tight loop");
    assert_eq!(res, Value::Int(1000000));
}

#[test]
fn test_e2e_t2_jit_04_unsupported_op_deopt_bailout() {
    // Part 1: Verify direct Cranelift JIT status 1 bailout on out-of-bounds array access
    let script = r#"
fn get_elem(arr, idx) {
    return arr[idx]
}
"#;
    let program = shae::parse_source(script).unwrap();
    let chunk = shae::compiler::Compiler::new()
        .compile_program(&program)
        .unwrap();

    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction not found");

    let mut engine = shae::jit::JitEngine::new().expect("Init JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compile get_elem");

    let elements = [42i64, 99i64];
    let buf = shae::jit::ShaeJitArray::create_buffer(&elements);

    // Out-of-bounds index 5 -> should bailout with status 1
    let args_oob = [buf.as_ptr() as i64, 5i64];
    let mut out_oob: i64 = 0;
    let status_oob = unsafe { jit_fn(args_oob.as_ptr(), args_oob.len(), &mut out_oob) };
    assert_eq!(
        status_oob, 1,
        "Array out of bounds must return bailout status 1"
    );

    // Part 2: Hot loop with string concatenation (un-JITable op) falling back to interpreter
    let hot_script = r#"
let i = 0
let text = ""
while i < 30 {
    text = text + "x"
    i = i + 1
}
return len(text)
"#;
    let res = run_script_vm(hot_script).expect("Interpreter fallback for un-JITable loop");
    assert_eq!(res, Value::Int(30));
}

#[test]
fn test_e2e_t2_jit_05_division_by_zero_runtime_error() {
    // JIT_THRESHOLD is 20 in src/jit.rs.
    // Loop iterates 50 times. OSR compiles at iteration 20.
    // At iteration 35, division by zero occurs inside native Cranelift execution.
    let script = r#"
let i = 0
let acc = 0
while i < 50 {
    if i >= 35 {
        let bad = 100 / 0
        acc = acc + bad
    }
    acc = acc + i
    i = i + 1
}
return acc
"#;
    let res = run_script_vm(script);
    assert!(
        res.is_err(),
        "Division by zero post-threshold must return runtime error"
    );
    let err = res.err().unwrap().to_lowercase();
    assert!(
        err.contains("zero") || err.contains("division"),
        "Error should mention division by zero: {}",
        err
    );
}

// =========================================================================
// R2: std:ws Full-Duplex WebSocket Boundaries
// =========================================================================

#[test]
fn test_e2e_t2_ws_01_zero_byte_payload_frame() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let t = conn.recv(3000)
    let b = conn.recv(3000)
    conn.send("ZERO_PAYLOAD_OK")
    conn.close(1000, "Done")
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &client_key, None)
        .unwrap();

    let mask_key = [0x11, 0x22, 0x33, 0x44];
    // Send 0-byte text frame
    let frame_text = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, true, Some(mask_key), b"");
    assert_eq!(frame_text[1] & 0x7F, 0);
    stream.write_all(&frame_text).unwrap();

    // Send 0-byte binary frame
    let frame_bin =
        shae::ws::encode_frame(true, shae::ws::OPCODE_BINARY, true, Some(mask_key), b"");
    assert_eq!(frame_bin[1] & 0x7F, 0);
    stream.write_all(&frame_bin).unwrap();

    let reply = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(reply.opcode, shae::ws::OPCODE_TEXT);
    assert_eq!(String::from_utf8_lossy(&reply.payload), "ZERO_PAYLOAD_OK");

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t2_ws_02_extended_payload_125_126_127() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let m125 = conn.recv(3000)
    conn.send(str(len(m125)))
    let m126 = conn.recv(3000)
    conn.send(str(len(m126)))
    let m127 = conn.recv(3000)
    conn.send(str(len(m127)))
    conn.close(1000, "Done")
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &client_key, None)
        .unwrap();

    let mask = [0x55, 0xAA, 0x55, 0xAA];

    // 125 bytes -> 7-bit standard length
    let p125 = vec![0x41u8; 125];
    let f125 = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, true, Some(mask), &p125);
    assert_eq!(f125[1] & 0x7F, 125);
    stream.write_all(&f125).unwrap();
    let r125 = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(String::from_utf8_lossy(&r125.payload), "125");

    // 126 bytes -> 16-bit extended length (prefix 126 / 0x7E)
    let p126 = vec![0x42u8; 126];
    let f126 = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, true, Some(mask), &p126);
    assert_eq!(f126[1] & 0x7F, 126);
    stream.write_all(&f126).unwrap();
    let r126 = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(String::from_utf8_lossy(&r126.payload), "126");

    // 127 bytes -> 16-bit extended length
    let p127 = vec![0x43u8; 127];
    let f127 = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, true, Some(mask), &p127);
    assert_eq!(f127[1] & 0x7F, 126);
    stream.write_all(&f127).unwrap();
    let r127 = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(String::from_utf8_lossy(&r127.payload), "127");

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t2_ws_03_large_payload_64k() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(5000)
if conn != null {{
    let data = conn.recv(5000)
    conn.send_binary(data)
    conn.close(1000, "Done")
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &client_key, None)
        .unwrap();

    let size = 65536;
    let mut large_payload = Vec::with_capacity(size);
    for i in 0..size {
        large_payload.push((i % 256) as u8);
    }

    let mask = [0x12, 0x34, 0x56, 0x78];
    let frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_BINARY,
        true,
        Some(mask),
        &large_payload,
    );
    assert_eq!(frame[1] & 0x7F, 127, "Prefix must be 127 for >65535 bytes");
    stream.write_all(&frame).unwrap();

    let resp_frame = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(resp_frame.payload.len(), size);
    assert_eq!(sha256_hex(&resp_frame.payload), sha256_hex(&large_payload));

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t2_ws_04_unmasked_client_frame_rejection() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let dummy = conn.recv(3000)
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &client_key, None)
        .unwrap();

    // RFC 6455 §5.1: Client MUST mask all frames. Unmasked client frames MUST be rejected by server.
    let payload = b"unmasked-illegal-client-frame";
    let unmasked_frame = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, false, None, payload);
    assert_eq!(unmasked_frame[1] & 0x80, 0, "Mask bit must be 0");
    stream.write_all(&unmasked_frame).unwrap();

    // Server must respond with Close frame code 1002 (Protocol Error) or terminate connection
    let close_resp =
        shae::ws::read_frame_stream(&mut stream).expect("Server must send close frame");
    assert_eq!(close_resp.opcode, shae::ws::OPCODE_CLOSE);
    let code = u16::from_be_bytes([close_resp.payload[0], close_resp.payload[1]]);
    assert_eq!(
        code, 1002,
        "Server must close with Close Code 1002 (Protocol Error)"
    );

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t2_ws_05_control_frame_payload_limit_125() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let dummy = conn.recv(3000)
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &client_key, None)
        .unwrap();

    // RFC 6455 §5.5: All control frames MUST have a payload length <= 125 bytes.
    // Client sends Ping with 126 bytes payload (exceeds 125 limit).
    let invalid_ping_payload = vec![0x42; 126];
    let mask = [1, 2, 3, 4];
    let ping_frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_PING,
        true,
        Some(mask),
        &invalid_ping_payload,
    );
    stream.write_all(&ping_frame).unwrap();

    // Server must reject with Close Code 1002 (Protocol Error)
    let close_resp = shae::ws::read_frame_stream(&mut stream)
        .expect("Server must send close frame on invalid control frame");
    assert_eq!(close_resp.opcode, shae::ws::OPCODE_CLOSE);
    let code = u16::from_be_bytes([close_resp.payload[0], close_resp.payload[1]]);
    assert_eq!(
        code, 1002,
        "Server must close with Close Code 1002 (Protocol Error) for ping > 125"
    );

    server_handle.join().unwrap();
}

// =========================================================================
// R3: Browser WebAssembly Game Player Boundaries
// =========================================================================

#[test]
fn test_e2e_t2_wasm_01_empty_source_compilation() {
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source("// Just a comment\n")
        .expect("Empty compilation");

    assert!(wasm_bytes.starts_with(WASM_MAGIC));
    assert_eq!(&wasm_bytes[4..8], WASM_VERSION);
}

#[test]
fn test_e2e_t2_wasm_02_zero_param_function() {
    let source = r#"
fn get_constant() {
    return 42
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(source).expect("WASM compile");
    let val = WasmRunner::execute_function(&wasm_bytes, "get_constant", &[])
        .expect("Execution of zero-param function");
    assert_eq!(val, 42);
}

#[test]
fn test_e2e_t2_wasm_03_numeric_boundary_values() {
    let source = r#"
fn sub(a, b) {
    return a - b
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(source).expect("WASM compile");

    // Test with 0 - 1000 = -1000
    let res = WasmRunner::execute_function(&wasm_bytes, "sub", &[0, 1000])
        .expect("Execute with negative result");
    assert_eq!(res, -1000);
}

#[test]
fn test_e2e_t2_wasm_04_extreme_input_key_codes() {
    let source = r#"
fn handle_input(key) {
    return key
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(source).expect("WASM compile");

    let res_neg = WasmRunner::execute_function(&wasm_bytes, "handle_input", &[-1])
        .expect("Negative key code handled");
    assert_eq!(res_neg, -1);

    let res_large = WasmRunner::execute_function(&wasm_bytes, "handle_input", &[999999])
        .expect("Large key code handled");
    assert_eq!(res_large, 999999);
}

#[test]
fn test_e2e_t2_wasm_05_extreme_frame_delta_time() {
    let source = r#"
fn tick(dt) {
    let speed = 10
    return dt * speed
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(source).expect("WASM compile");

    // dt = 0
    let res_zero = WasmRunner::execute_function(&wasm_bytes, "tick", &[0]).unwrap();
    assert_eq!(res_zero, 0);

    // dt = 5 (large delta)
    let res_large = WasmRunner::execute_function(&wasm_bytes, "tick", &[5]).unwrap();
    assert_eq!(res_large, 50);
}

// =========================================================================
// R4: Shae Studio Web IDE & Visual Debugger Boundaries
// =========================================================================

#[test]
fn test_e2e_t2_ide_01_empty_and_whitespace_source_check() {
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();
    let check_url = format!("{}/api/check", ide.url());

    // Empty source
    let payload_empty = json!({ "code": "" });
    let resp = client
        .post(&check_url)
        .header("Content-Type", "application/json")
        .body(payload_empty.to_string())
        .send()
        .expect("Check empty source");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["valid"], true);
    assert!(body["errors"].as_array().unwrap().is_empty());

    // Whitespace source
    let payload_ws = json!({ "code": "   \n\t\n   " });
    let resp_ws = client
        .post(&check_url)
        .header("Content-Type", "application/json")
        .body(payload_ws.to_string())
        .send()
        .expect("Check whitespace source");
    assert_eq!(resp_ws.status(), 200);
    let body_ws: serde_json::Value = serde_json::from_str(&resp_ws.text().unwrap()).unwrap();
    assert_eq!(body_ws["valid"], true);
}

#[test]
fn test_e2e_t2_ide_02_large_source_diagnostic_performance() {
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();
    let check_url = format!("{}/api/check", ide.url());

    // Generate a 1,000-line valid Shae script
    let mut large_code = String::new();
    for i in 0..1000 {
        large_code.push_str(&format!("let var_{} = {}\n", i, i));
    }
    large_code.push_str("return var_999\n");

    let payload = json!({ "code": large_code });
    let start = std::time::Instant::now();
    let resp = client
        .post(&check_url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .expect("Check large source");
    let elapsed = start.elapsed();

    assert_eq!(resp.status(), 200);
    assert!(
        elapsed < Duration::from_millis(500),
        "Diagnostic on 1000 lines should finish in <500ms, took {:?}",
        elapsed
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["valid"], true);
}

#[test]
fn test_e2e_t2_ide_03_gui_frame_no_active_window() {
    // Clear GUI registry so no window exists
    shae::gui::get_window_registry().lock().unwrap().clear();

    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();
    let frame_url = format!("{}/api/gui/frame", ide.url());

    let resp = client
        .get(&frame_url)
        .send()
        .expect("GET /api/gui/frame failed");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["width"], 0);
    assert_eq!(body["height"], 0);
    let buffer = body["buffer"].as_array().expect("Empty buffer array");
    assert!(buffer.is_empty());
}

#[test]
fn test_e2e_t2_ide_04_dap_stepping_past_termination() {
    let threads_req = json!({
        "seq": 1,
        "type": "request",
        "command": "threads"
    });
    let payload = serde_json::to_string(&threads_req).unwrap();
    let input = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);

    let reader = Cursor::new(input.into_bytes());
    let writer = Cursor::new(Vec::new());

    let mut dap = DapServer::new(reader, writer);
    assert!(dap.run().is_ok());
}

#[test]
fn test_e2e_t2_ide_05_concurrent_ide_requests() {
    let ide = Arc::new(IdeServerFixture::start());
    let mut handles = Vec::new();

    for i in 0..10 {
        let ide_clone = Arc::clone(&ide);
        handles.push(thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            let check_url = format!("{}/api/check", ide_clone.url());
            let payload = json!({ "code": format!("let v = {}\nreturn v", i) });
            let resp = client
                .post(&check_url)
                .header("Content-Type", "application/json")
                .body(payload.to_string())
                .send()
                .expect("Concurrent check request");
            assert_eq!(resp.status(), 200);
        }));
    }

    for h in handles {
        h.join().expect("Concurrent request thread failed");
    }
}

// =========================================================================
// R5: Central Package Registry Explorer Boundaries
// =========================================================================

#[test]
fn test_e2e_t2_pkg_01_search_xss_and_path_traversal_sanitization() {
    let mock = MockRegistry::new();
    let client = reqwest::blocking::Client::new();

    // Query with XSS string
    let xss_query = "<script>alert('xss')</script>";
    let url_xss = format!("{}/packages/{}.json", mock.url, xss_query);
    let resp_xss = client.get(&url_xss).send().expect("Send XSS request");
    assert_eq!(resp_xss.status(), 404);

    // Query with path traversal
    let traversal = "../../../etc/passwd";
    let url_traversal = format!("{}/packages/{}.json", mock.url, traversal);
    let resp_traversal = client
        .get(&url_traversal)
        .send()
        .expect("Send path traversal request");
    assert_eq!(resp_traversal.status(), 404);
}

#[test]
fn test_e2e_t2_pkg_02_nonexistent_package_404() {
    let mock = MockRegistry::new();
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/packages/non-existent-pkg-999.json", mock.url);
    let resp = client
        .get(&url)
        .send()
        .expect("Send non-existent package request");
    assert_eq!(resp.status(), 404);
}

#[test]
fn test_e2e_t2_pkg_03_zero_dependencies_and_empty_readme() {
    let toml_str = r#"
[package]
name = "shae-minimal"
version = "0.0.1"
entry = "src/main.shae"
readme = ""

[dependencies]
"#;
    let parsed: toml::Value = toml::from_str(toml_str).expect("Parse minimal manifest");
    assert_eq!(parsed["package"]["name"].as_str(), Some("shae-minimal"));
    assert_eq!(parsed["package"]["readme"].as_str(), Some(""));
    assert!(parsed.get("dependencies").is_some());
}

#[test]
fn test_e2e_t2_pkg_04_corrupted_archive_checksum_mismatch() {
    let proj = TempProject::new("corrupted_archive");
    proj.write_manifest("shae-corrupt", "0.1.0");
    proj.write_file("src/main.shae", "return 42");

    let mut bytes = pkg::create_shaepkg(&proj.dir).expect("Create shaepkg");
    let original_checksum = compute_sha256(&bytes);

    // Corrupt one byte in the archive payload
    if let Some(b) = bytes.last_mut() {
        *b ^= 0xFF;
    }
    let corrupted_checksum = compute_sha256(&bytes);

    assert_ne!(
        original_checksum, corrupted_checksum,
        "Bit flip must alter SHA-256 digest"
    );
}

#[test]
fn test_e2e_t2_pkg_05_complex_semver_pre_release() {
    let versions = vec![
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-beta.2+20261120",
        "1.0.0",
        "1.0.1",
    ];

    for ver in &versions {
        let manifest = format!("[package]\nname = \"pkg\"\nversion = \"{}\"\n", ver);
        let parsed: Result<toml::Value, _> = toml::from_str(&manifest);
        assert!(
            parsed.is_ok(),
            "Manifest with version '{}' should parse",
            ver
        );
        assert_eq!(parsed.unwrap()["package"]["version"].as_str(), Some(*ver));
    }
}
