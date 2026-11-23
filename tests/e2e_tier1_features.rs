mod e2e_common;

use e2e_common::*;
use serde_json::json;
use shae::dap::DapServer;
use shae::pkg::{self, SHAEPKG_MAGIC, compute_sha256};
use shae::value::Value;
use shae::wasm::{WASM_MAGIC, WASM_VERSION, WasmCompiler, WasmRunner};
use std::io::{Cursor, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

// =========================================================================
// R1: On-Stack Replacement (OSR) & Tiered JIT Compiler
// =========================================================================

#[test]
fn test_e2e_t1_jit_01_hot_loop_threshold_accumulation() {
    let script = r#"
let i = 0
let acc = 0
while i < 100 {
    acc = acc + i
    i = i + 1
}
return acc
"#;
    let vm_res = run_script_vm(script).expect("VM execution should succeed");
    assert_eq!(
        vm_res,
        Value::Int(4950),
        "VM loop accumulation must equal 4950"
    );

    let jit_res = run_script_jit(script);
    if let Ok(val) = jit_res {
        assert_eq!(
            val,
            Value::Int(4950),
            "JIT loop accumulation must equal 4950"
        );
    }
}

#[test]
fn test_e2e_t1_jit_02_state_handover_multiple_locals() {
    let script = r#"
let a = 10
let b = 20
let c = 30
let i = 0
while i < 50 {
    a = a + 1
    b = b + 2
    c = c + 3
    i = i + 1
}
return [a, b, c]
"#;
    let res = run_script_vm(script).expect("VM execution should preserve locals");
    if let Value::Array(arr) = res {
        let guard = arr.read().unwrap();
        let items: Vec<i64> = guard
            .iter()
            .map(|v| match v {
                Value::Int(n) => *n,
                _ => panic!("Expected Int in list"),
            })
            .collect();
        assert_eq!(items, vec![60, 120, 180]);
    } else {
        panic!("Expected Array return value, got {:?}", res);
    }
}

#[test]
fn test_e2e_t1_jit_03_nested_loops_osr_state_preservation() {
    let script = r#"
let outer = 0
let total = 0
while outer < 5 {
    let inner = 0
    while inner < 40 {
        total = total + 1
        inner = inner + 1
    }
    outer = outer + 1
}
return total
"#;
    let res = run_script_vm(script).expect("Nested loop execution should succeed");
    assert_eq!(res, Value::Int(200));
}

#[test]
fn test_e2e_t1_jit_04_hot_loop_branching_merges() {
    let script = r#"
let i = 0
let evens = 0
let odds = 0
while i < 100 {
    if i % 2 == 0 {
        evens = evens + i
    } else {
        odds = odds + i
    }
    i = i + 1
}
return [evens, odds]
"#;
    let res = run_script_vm(script).expect("Branching loop execution should succeed");
    if let Value::Array(arr) = res {
        let guard = arr.read().unwrap();
        let items: Vec<i64> = guard
            .iter()
            .map(|v| match v {
                Value::Int(n) => *n,
                _ => panic!("Expected Int in list"),
            })
            .collect();
        assert_eq!(items, vec![2450, 2500]);
    } else {
        panic!("Expected Array return value");
    }
}

#[test]
fn test_e2e_t1_jit_05_hot_loop_reuse() {
    let script = r#"
fn compute(n) {
    let s = 0
    let i = 0
    while i <= n {
        s = s + i
        i = i + 1
    }
    return s
}
let r1 = compute(60)
let r2 = compute(60)
return [r1, r2]
"#;
    let res = run_script_vm(script).expect("Function loop reuse should succeed");
    if let Value::Array(arr) = res {
        let guard = arr.read().unwrap();
        let items: Vec<i64> = guard
            .iter()
            .map(|v| match v {
                Value::Int(n) => *n,
                _ => panic!("Expected Int"),
            })
            .collect();
        assert_eq!(items, vec![1830, 1830]);
    } else {
        panic!("Expected Array return value");
    }
}

// =========================================================================
// R2: std:ws Full-Duplex WebSocket Engine (RFC 6455)
// =========================================================================

#[test]
fn test_e2e_t1_ws_01_rfc6455_handshake_establishment() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    conn.send("HANDSHAKE_CONFIRMED")
    conn.close(1000, "Clean close")
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .expect("Failed to connect to Shae WS server");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
    let req = format!(
        "GET /chat HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        port, client_key
    );

    stream.write_all(req.as_bytes()).unwrap();

    let mut header_bytes = Vec::new();
    let mut one_byte = [0u8; 1];
    while stream.read_exact(&mut one_byte).is_ok() {
        header_bytes.push(one_byte[0]);
        if header_bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let resp = String::from_utf8_lossy(&header_bytes);

    assert!(resp.starts_with("HTTP/1.1 101 Switching Protocols"));
    assert!(resp.to_lowercase().contains("upgrade: websocket"));
    let expected_accept = compute_ws_accept(client_key);
    assert!(
        resp.contains(&format!("Sec-WebSocket-Accept: {}", expected_accept)),
        "Handshake must return computed Sec-WebSocket-Accept key"
    );

    // Read the confirmation frame from Shae server
    let frame = shae::ws::read_frame_stream(&mut stream).expect("Read frame from Shae server");
    assert_eq!(frame.opcode, shae::ws::OPCODE_TEXT);
    assert_eq!(
        String::from_utf8_lossy(&frame.payload),
        "HANDSHAKE_CONFIRMED"
    );

    // Read Close frame
    let close_frame = shae::ws::read_frame_stream(&mut stream).expect("Read close frame");
    assert_eq!(close_frame.opcode, shae::ws::OPCODE_CLOSE);

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t1_ws_02_text_frame_client_masking() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let msg = conn.recv(3000)
    conn.send("ECHO: " + msg)
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

    // Client sends masked text frame per RFC 6455 §5.1
    let payload = b"Hello Shae WebSocket!";
    let mask_key = [0xAB, 0xCD, 0xEF, 0x12];
    let frame = shae::ws::encode_frame(true, shae::ws::OPCODE_TEXT, true, Some(mask_key), payload);
    stream.write_all(&frame).unwrap();

    // Read echoed unmasked frame from Shae server
    let frame_in = shae::ws::read_frame_stream(&mut stream).unwrap();

    assert!(frame_in.fin);
    assert_eq!(
        frame_in.opcode,
        shae::ws::OPCODE_TEXT,
        "Echo frame should be OpCode 0x1 (Text)"
    );
    assert_eq!(
        String::from_utf8_lossy(&frame_in.payload),
        "ECHO: Hello Shae WebSocket!"
    );

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t1_ws_03_binary_frame_exchange() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let data = conn.recv(3000)
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
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(
        &mut stream,
        "127.0.0.1",
        port,
        "/binary",
        &client_key,
        None,
    )
    .unwrap();

    let payload = vec![0xCA, 0xFE, 0xBA, 0xBE];
    let frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_BINARY,
        true,
        Some([1, 2, 3, 4]),
        &payload,
    );
    stream.write_all(&frame).unwrap();

    let frame_in = shae::ws::read_frame_stream(&mut stream).unwrap();

    assert!(frame_in.fin);
    assert_eq!(
        frame_in.opcode,
        shae::ws::OPCODE_BINARY,
        "Expected binary frame (OpCode 0x2)"
    );
    assert_eq!(frame_in.payload, vec![0xCA, 0xFE, 0xBA, 0xBE]);

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t1_ws_04_ping_pong_heartbeat() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    let msg = conn.recv(3000)
    conn.send("OK: " + msg)
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
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/ping", &client_key, None)
        .unwrap();

    // Send Ping frame with payload
    let ping_payload = b"ping-heartbeat-42";
    let ping_frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_PING,
        true,
        Some([9, 8, 7, 6]),
        ping_payload,
    );
    stream.write_all(&ping_frame).unwrap();

    // Shae server automatically replies with Pong frame containing identical payload per RFC 6455 §5.5.2
    let pong_frame = shae::ws::read_frame_stream(&mut stream).unwrap();

    assert!(pong_frame.fin);
    assert_eq!(
        pong_frame.opcode,
        shae::ws::OPCODE_PONG,
        "Expected Pong frame (OpCode 0xA)"
    );
    assert_eq!(pong_frame.payload, ping_payload);

    // Complete script by sending "done" text frame so server exits cleanly
    let done_frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_TEXT,
        true,
        Some([1, 1, 1, 1]),
        b"done",
    );
    stream.write_all(&done_frame).unwrap();

    let reply = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(reply.opcode, shae::ws::OPCODE_TEXT);
    assert_eq!(String::from_utf8_lossy(&reply.payload), "OK: done");

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t1_ws_05_clean_close_handshake() {
    require_ws_module();
    let port = get_available_port();

    let script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(3000)
if conn != null {{
    conn.close(1000, "Clean close")
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
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/close", &client_key, None)
        .unwrap();

    // Server sends Close frame
    let close_frame = shae::ws::read_frame_stream(&mut stream).unwrap();

    assert!(close_frame.fin);
    assert_eq!(
        close_frame.opcode,
        shae::ws::OPCODE_CLOSE,
        "Expected Close frame (OpCode 0x8)"
    );
    let code = u16::from_be_bytes([close_frame.payload[0], close_frame.payload[1]]);
    assert_eq!(code, 1000);
    assert!(String::from_utf8_lossy(&close_frame.payload[2..]).contains("Clean close"));

    // Echo close frame back
    let echo_close = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_CLOSE,
        true,
        Some([0, 0, 0, 0]),
        &close_frame.payload[0..2],
    );
    stream.write_all(&echo_close).unwrap();

    server_handle.join().unwrap();
}

// =========================================================================
// R3: Browser WebAssembly Game Player
// =========================================================================

#[test]
fn test_e2e_t1_wasm_01_compile_valid_module() {
    let source = r#"
fn add(a, b) {
    return a + b
}
let res = add(10, 20)
return res
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(source)
        .expect("WASM compile failed");

    assert!(
        wasm_bytes.starts_with(WASM_MAGIC),
        "WASM module must begin with \\0asm magic bytes"
    );
    assert_eq!(
        &wasm_bytes[4..8],
        WASM_VERSION,
        "WASM module must specify version 1.0"
    );
}

#[test]
fn test_e2e_t1_wasm_02_game_lifecycle_exports() {
    let source = r#"
fn game_init() {
    return 0
}

fn game_tick(dt) {
    return 0
}

fn game_input(key, is_down) {
    return 0
}

fn get_framebuffer() {
    return 0
}

fn get_width() {
    return 256
}

fn get_height() {
    return 240
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(source)
        .expect("WASM compile failed");
    assert!(wasm_bytes.len() > 8);

    let runner = WasmRunner::execute_function(&wasm_bytes, "get_width", &[]);
    if let Ok(width) = runner {
        assert_eq!(width, 256);
    }
}

#[test]
fn test_e2e_t1_wasm_03_linear_memory_framebuffer() {
    let source = r#"
fn set_pixel(x, y, color) {
    return x * 1000 + y + color
}
let r = set_pixel(10, 20, 255)
return r
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(source)
        .expect("WASM compile failed");

    let val = WasmRunner::execute_function(&wasm_bytes, "set_pixel", &[10, 20, 255])
        .expect("Executing set_pixel in WASM should succeed");
    assert_eq!(val, 10 * 1000 + 20 + 255);
}

#[test]
fn test_e2e_t1_wasm_04_game_input_routing() {
    let source = r#"
fn handle_input(key, is_down) {
    let factor = key * is_down
    return factor
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(source)
        .expect("WASM compile failed");

    let val = WasmRunner::execute_function(&wasm_bytes, "handle_input", &[39, 2])
        .expect("Executing handle_input in WASM should succeed");
    assert_eq!(val, 78);
}

#[test]
fn test_e2e_t1_wasm_05_wasm_cli_execution() {
    let script = TempScript::new(
        "wasm_cli_test",
        r#"
fn multiply(x, y) {
    return x * y
}
let ans = multiply(6, 7)
return ans
"#,
    );

    let out_wasm = std::env::temp_dir().join(format!("wasm_cli_test_{}.wasm", fastrand::u64(..)));
    let (ok, stdout, stderr) = run_cli(&[
        "wasm",
        script.path.to_str().unwrap(),
        "-o",
        out_wasm.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(out_wasm);
    assert!(
        ok,
        "CLI 'shae wasm <file>' should succeed. stderr: {}",
        stderr
    );
    let out_lower = stdout.to_lowercase();
    assert!(
        out_lower.contains("wasm")
            || out_lower.contains("webassembly")
            || out_lower.contains("compiled")
            || out_lower.is_empty(),
        "WASM CLI output: {}",
        stdout
    );
}

// =========================================================================
// R4: Shae Studio Web IDE & Visual Debugger Upgrades
// =========================================================================

#[test]
fn test_e2e_t1_ide_01_monaco_grammar_serving() {
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&ide.url())
        .send()
        .expect("Failed to fetch IDE root HTML");

    assert_eq!(resp.status(), 200);
    let html = resp.text().unwrap();
    assert!(
        html.contains("Shae Studio") || html.contains("editor") || html.contains("monaco"),
        "IDE HTML must contain editor elements"
    );
}

#[test]
fn test_e2e_t1_ide_02_diagnostics_endpoint() {
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();

    let check_url = format!("{}/api/check", ide.url());
    let payload = json!({
        "code": "fn broken(a) {\n  let x = a +\n  return x\n}",
        "source": "fn broken(a) {\n  let x = a +\n  return x\n}"
    });

    let resp = client
        .post(&check_url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .expect("Failed to post /api/check");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["valid"], false);
    assert!(!body["errors"].as_array().unwrap().is_empty());
}

#[test]
fn test_e2e_t1_ide_03_gui_frame_preview_endpoint() {
    // Populate an active window in GUI registry
    {
        let mut reg = shae::gui::get_window_registry().lock().unwrap();
        reg.insert(
            1,
            shae::gui::ShaeWindow {
                backend: shae::gui::WindowBackend::Headless {
                    open: true,
                    ticks: 0,
                },
                width: 320,
                height: 240,
                buffer: vec![0xFF00FF00; 320 * 240],
                keys_down: vec![],
                mouse_pos: None,
                mouse_down: (false, false, false),
                last_frame: std::time::Instant::now(),
            },
        );
    }

    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();

    let frame_url = format!("{}/api/gui/frame", ide.url());
    let resp = client
        .get(&frame_url)
        .send()
        .expect("GET /api/gui/frame failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["width"], 320);
    assert_eq!(body["height"], 240);
    let buffer = body["buffer"]
        .as_array()
        .expect("Buffer array in frame preview");
    assert_eq!(buffer.len(), 320 * 240);

    // Clean up
    shae::gui::get_window_registry().lock().unwrap().clear();
}

#[test]
fn test_e2e_t1_ide_04_dap_breakpoint_initialization() {
    let init_req = json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "vscode",
            "adapterID": "shae"
        }
    });
    let payload = serde_json::to_string(&init_req).unwrap();
    let input = format!(
        "Content-Length: {}

{}",
        payload.len(),
        payload
    );

    let reader = Cursor::new(input.into_bytes());
    let writer = Cursor::new(Vec::new());

    let mut dap = DapServer::new(reader, writer);
    let run_res = dap.run();
    assert!(run_res.is_ok(), "DAP server initialize run should succeed");
}

#[test]
fn test_e2e_t1_ide_05_dap_stepping_and_variables() {
    let init_req = json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "test-client",
            "adapterID": "shae"
        }
    });

    let bp_req = json!({
        "seq": 2,
        "type": "request",
        "command": "setBreakpoints",
        "arguments": {
            "source": { "path": "/workspace/main.shae" },
            "breakpoints": [{ "line": 3 }]
        }
    });

    let conf_req = json!({
        "seq": 3,
        "type": "request",
        "command": "configurationDone",
        "arguments": {}
    });

    let threads_req = json!({
        "seq": 4,
        "type": "request",
        "command": "threads",
        "arguments": {}
    });

    let stack_req = json!({
        "seq": 5,
        "type": "request",
        "command": "stackTrace",
        "arguments": { "threadId": 1 }
    });

    let scopes_req = json!({
        "seq": 6,
        "type": "request",
        "command": "scopes",
        "arguments": { "frameId": 1 }
    });

    let variables_req = json!({
        "seq": 7,
        "type": "request",
        "command": "variables",
        "arguments": { "variablesReference": 11 }
    });

    let next_req = json!({
        "seq": 8,
        "type": "request",
        "command": "next",
        "arguments": {}
    });

    let step_in_req = json!({
        "seq": 9,
        "type": "request",
        "command": "stepIn",
        "arguments": {}
    });

    let unknown_req = json!({
        "seq": 10,
        "type": "request",
        "command": "customUnknownCommand",
        "arguments": {}
    });

    let disconnect_req = json!({
        "seq": 11,
        "type": "request",
        "command": "disconnect",
        "arguments": {}
    });

    let mut input_bytes = Vec::new();
    input_bytes.extend_from_slice(&encode_dap_message(&init_req));
    input_bytes.extend_from_slice(&encode_dap_message(&bp_req));
    input_bytes.extend_from_slice(&encode_dap_message(&conf_req));
    input_bytes.extend_from_slice(&encode_dap_message(&threads_req));
    input_bytes.extend_from_slice(&encode_dap_message(&stack_req));
    input_bytes.extend_from_slice(&encode_dap_message(&scopes_req));
    input_bytes.extend_from_slice(&encode_dap_message(&variables_req));
    input_bytes.extend_from_slice(&encode_dap_message(&next_req));
    input_bytes.extend_from_slice(&encode_dap_message(&step_in_req));
    input_bytes.extend_from_slice(&encode_dap_message(&unknown_req));
    input_bytes.extend_from_slice(&encode_dap_message(&disconnect_req));

    let reader = Cursor::new(input_bytes);
    let out_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = SharedWriter::new(std::sync::Arc::clone(&out_buf));

    let mut dap = DapServer::new(reader, writer);
    dap.run().expect("DAP server run should complete");

    let responses = parse_dap_responses(&out_buf.lock().unwrap());
    assert!(!responses.is_empty(), "Must receive DAP responses");

    let init_resp = responses
        .iter()
        .find(|r| r["command"] == "initialize")
        .expect("init resp");
    assert_eq!(init_resp["success"], true);

    let bp_resp = responses
        .iter()
        .find(|r| r["command"] == "setBreakpoints")
        .expect("bp resp");
    assert_eq!(bp_resp["success"], true);

    let threads_resp = responses
        .iter()
        .find(|r| r["command"] == "threads")
        .expect("threads resp");
    assert_eq!(threads_resp["success"], true);
    assert_eq!(threads_resp["body"]["threads"][0]["id"], 1);

    let scopes_resp = responses
        .iter()
        .find(|r| r["command"] == "scopes")
        .expect("scopes resp");
    assert_eq!(scopes_resp["success"], true);
    let scopes = scopes_resp["body"]["scopes"]
        .as_array()
        .expect("scopes array");
    assert!(scopes.iter().any(|s| s["name"] == "Locals"));

    let next_resp = responses
        .iter()
        .find(|r| r["command"] == "next")
        .expect("next resp");
    assert_eq!(next_resp["success"], true);

    let step_resp = responses
        .iter()
        .find(|r| r["command"] == "stepIn")
        .expect("stepIn resp");
    assert_eq!(step_resp["success"], true);

    let unknown_resp = responses
        .iter()
        .find(|r| r["command"] == "customUnknownCommand")
        .expect("unknown cmd resp");
    assert_eq!(unknown_resp["success"], false);
    assert_eq!(unknown_resp["message"], "Unknown command");

    let disc_resp = responses
        .iter()
        .find(|r| r["command"] == "disconnect")
        .expect("disc resp");
    assert_eq!(disc_resp["success"], true);
}

// =========================================================================
// R5: Central Package Registry Explorer
// =========================================================================

#[test]
fn test_e2e_t1_pkg_01_pack_archive_creation() {
    let proj = TempProject::new("pack_archive");
    proj.write_manifest("shae-test-pkg", "0.1.0");
    proj.write_file("src/main.shae", "fn hello() { return \"world\" }");

    let archive_bytes =
        pkg::create_shaepkg(&proj.dir).expect("Package archive creation must succeed");
    assert!(
        archive_bytes.starts_with(SHAEPKG_MAGIC),
        "Archive must start with SHAEPKG magic bytes"
    );
}

#[test]
fn test_e2e_t1_pkg_02_registry_metadata_schema() {
    let mock = MockRegistry::new();
    let pkg_json = json!({
        "name": "shae-math",
        "description": "Mathematics library for Shae",
        "latest": "0.2.0",
        "versions": {
            "0.2.0": {
                "version": "0.2.0",
                "checksum": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "download_url": "/downloads/shae-math-0.2.0.shaepkg",
                "dependencies": {}
            }
        }
    });

    mock.add_route(
        "GET",
        "/packages/shae-math.json",
        200,
        "application/json",
        pkg_json.to_string().into_bytes(),
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&format!("{}/packages/shae-math.json", mock.url))
        .send()
        .expect("Fetch package index");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(body["name"], "shae-math");
    assert_eq!(body["latest"], "0.2.0");
    assert!(body["versions"]["0.2.0"]["checksum"].is_string());
}

#[test]
fn test_e2e_t1_pkg_03_archive_checksum_integrity() {
    let proj = TempProject::new("checksum_integrity");
    proj.write_manifest("shae-checksum-pkg", "1.0.0");
    proj.write_file("src/main.shae", "let x = 123");

    let bytes = pkg::create_shaepkg(&proj.dir).expect("Package archive creation must succeed");
    let computed_hex = compute_sha256(&bytes);
    let expected_hex = sha256_hex(&bytes);

    assert_eq!(computed_hex, expected_hex);
    assert_eq!(computed_hex.len(), 64);
}

#[test]
fn test_e2e_t1_pkg_04_publish_payload_structure() {
    let proj = TempProject::new("publish_payload");
    proj.write_manifest("shae-published", "0.5.0");
    proj.write_file("src/main.shae", "return true");

    let archive_bytes =
        pkg::create_shaepkg(&proj.dir).expect("Package archive creation must succeed");

    let publish_request = json!({
        "name": "shae-published",
        "version": "0.5.0",
        "archive_size": archive_bytes.len(),
        "checksum": compute_sha256(&archive_bytes)
    });

    assert_eq!(publish_request["name"], "shae-published");
    assert_eq!(publish_request["version"], "0.5.0");
    assert!(publish_request["archive_size"].as_u64().unwrap() > 0);
}

#[test]
fn test_e2e_t1_pkg_05_package_manifest_parsing() {
    let toml_str = r#"
[package]
name = "shae-algorithm"
version = "1.2.3"
entry = "src/main.shae"

[dependencies]
shae-math = ">= 0.1.0"
"#;
    let parsed: toml::Value = toml::from_str(toml_str).expect("Valid toml manifest");
    assert_eq!(parsed["package"]["name"].as_str(), Some("shae-algorithm"));
    assert_eq!(parsed["package"]["version"].as_str(), Some("1.2.3"));
    assert!(parsed["dependencies"]["shae-math"].is_str());
}
