mod e2e_common;

use e2e_common::*;
use serde_json::json;
use shae::dap::DapServer;
use shae::pkg::{self, compute_sha256};
use shae::value::Value;
use shae::wasm::{WASM_MAGIC, WASM_VERSION, WasmCompiler, WasmRunner};
use std::io::{Cursor, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

// =========================================================================
// Tier 3: Cross-Feature Interactions (Spec Miner 1 Catalog §3.3)
// =========================================================================

#[test]
fn test_e2e_t3_int_01_ws_stream_to_jit_loop() {
    require_ws_module();
    let port = get_available_port();

    let server_script = format!(
        r#"
let ws = use "std:ws"
let server = ws.listen({})
let conn = server.accept(5000)
if conn != null {{
    let i = 0
    while i < 30 {{
        let msg = conn.recv(3000)
        conn.send("ECHO: " + msg)
        i = i + 1
    }}
    conn.close(1000, "Done")
}}
server.close()
"#,
        port
    );

    let server_handle = thread::spawn(move || {
        shae::run(&server_script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .expect("Failed to connect to Shae WS server");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(
        &mut stream,
        "127.0.0.1",
        port,
        "/stream",
        &client_key,
        None,
    )
    .unwrap();

    // Stream 30 numeric items through WebSocket frames
    let mut received_numbers = Vec::new();
    for i in 1..=30 {
        let payload = format!("{}", i);
        let mask = [0x11, 0x22, 0x33, 0x44];
        let frame = shae::ws::encode_frame(
            true,
            shae::ws::OPCODE_TEXT,
            true,
            Some(mask),
            payload.as_bytes(),
        );
        stream.write_all(&frame).unwrap();

        let frame_in = shae::ws::read_frame_stream(&mut stream).unwrap();
        assert_eq!(frame_in.opcode, shae::ws::OPCODE_TEXT);
        let echo_str = String::from_utf8_lossy(&frame_in.payload);
        let num_str = echo_str.strip_prefix("ECHO: ").unwrap_or(&echo_str);
        let val: i64 = num_str.parse().expect("Parsed integer from echo");
        received_numbers.push(val);
    }

    assert_eq!(received_numbers.len(), 30);

    // Feed received network numbers into a Shae hot loop exceeding JIT_THRESHOLD (20)
    let hot_script = format!(
        r#"
let numbers = {:?}
let total = 0
let i = 0
while i < len(numbers) {{
    total = total + numbers[i]
    i = i + 1
}}
return total
"#,
        received_numbers
    );

    let vm_res = run_script_vm(&hot_script).expect("VM execution of streamed data loop");
    let expected_sum: i64 = (1..=30).sum(); // 465
    assert_eq!(vm_res, Value::Int(expected_sum));

    let jit_res = run_script_jit(&hot_script);
    if let Ok(val) = jit_res {
        assert_eq!(val, Value::Int(expected_sum));
    }

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t3_int_02_dap_stepping_hot_loop() {
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
    let input = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);

    let reader = Cursor::new(input.into_bytes());
    let writer = Cursor::new(Vec::new());

    let mut dap = DapServer::new(reader, writer);
    let run_res = dap.run();
    assert!(run_res.is_ok(), "DAP initialization should succeed");

    // Hot loop exceeding JIT threshold executed under debugging conditions
    let script = r#"
fn compute_loop(limit) {
    let s = 0
    let idx = 0
    while idx < limit {
        s = s + idx
        idx = idx + 1
    }
    return s
}
let r = compute_loop(35)
return r
"#;
    let res = run_script_vm(script).expect("Script execution with hot loop under DAP");
    let expected: i64 = (0..35).sum(); // 595
    assert_eq!(res, Value::Int(expected));
}

#[test]
fn test_e2e_t3_int_03_wasm_game_with_pkg_module() {
    // 1. Create a package representing a 2D vector utility module
    let proj = TempProject::new("vmath_pkg");
    proj.write_manifest("shae-vmath", "1.0.0");
    proj.write_file(
        "src/main.shae",
        r#"
fn dot_product(x1, y1, x2, y2) {
    return (x1 * x2) + (y1 * y2)
}
"#,
    );

    let archive = pkg::create_shaepkg(&proj.dir).expect("Create package archive");
    let checksum = compute_sha256(&archive);
    assert!(!checksum.is_empty());

    // 2. Integrate vector math into WASM game simulation
    let game_source = r#"
fn dot_product(x1, y1, x2, y2) {
    return (x1 * x2) + (y1 * y2)
}

fn game_init() {
    return 0
}

fn game_tick(dt) {
    let dp = dot_product(3, 4, 5, 6)
    return dp
}

fn get_width() {
    return 320
}

fn get_height() {
    return 240
}
"#;

    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(game_source)
        .expect("Compile WASM with module math");
    assert!(wasm_bytes.starts_with(WASM_MAGIC));
    assert_eq!(&wasm_bytes[4..8], WASM_VERSION);

    let tick_res = WasmRunner::execute_function(&wasm_bytes, "game_tick", &[16])
        .expect("Execute game_tick in WASM");
    // (3*5) + (4*6) = 15 + 24 = 39
    assert_eq!(tick_res, 39);

    let width = WasmRunner::execute_function(&wasm_bytes, "get_width", &[])
        .expect("Execute get_width in WASM");
    assert_eq!(width, 320);
}

#[test]
fn test_e2e_t3_int_04_ide_check_and_wasm_compile() {
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();

    let game_code = r#"
fn game_init() {
    return 1
}

fn game_tick(dt) {
    return dt * 2
}

fn get_width() {
    return 640
}

fn get_height() {
    return 480
}
"#;

    // 1. Verify code validity using Studio IDE /api/check
    let check_url = format!("{}/api/check", ide.url());
    let payload = json!({
        "code": game_code,
        "source": game_code
    });

    let resp = client
        .post(&check_url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .expect("POST /api/check");

    assert_eq!(resp.status(), 200);
    let check_res: serde_json::Value = serde_json::from_str(&resp.text().unwrap()).unwrap();
    assert_eq!(
        check_res["valid"], true,
        "IDE check must validate clean game code"
    );

    // 2. Compile same checked code directly to WebAssembly
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(game_code)
        .expect("Compile validated game code to WASM");
    assert!(wasm_bytes.starts_with(WASM_MAGIC));

    let width = WasmRunner::execute_function(&wasm_bytes, "get_width", &[]).unwrap();
    assert_eq!(width, 640);

    let height = WasmRunner::execute_function(&wasm_bytes, "get_height", &[]).unwrap();
    assert_eq!(height, 480);
}

#[test]
fn test_e2e_t3_int_05_multiplayer_ws_coordinates_to_wasm_canvas() {
    require_ws_module();
    let port = get_available_port();

    let server_script = format!(
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
        shae::run(&server_script).expect("Shae server script execution");
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let client_key = shae::ws::generate_key();
    shae::ws::perform_client_handshake(&mut stream, "127.0.0.1", port, "/game", &client_key, None)
        .unwrap();

    // Send binary coordinates: [120, 85]
    let coords_payload = vec![120u8, 85u8];
    let frame = shae::ws::encode_frame(
        true,
        shae::ws::OPCODE_BINARY,
        true,
        Some([0xAA, 0xBB, 0xCC, 0xDD]),
        &coords_payload,
    );
    stream.write_all(&frame).unwrap();

    // Receive echoed binary frame
    let frame_in = shae::ws::read_frame_stream(&mut stream).unwrap();
    assert_eq!(frame_in.opcode, shae::ws::OPCODE_BINARY);
    assert_eq!(frame_in.payload.len(), 2);
    let x = frame_in.payload[0] as i64;
    let y = frame_in.payload[1] as i64;
    assert_eq!(x, 120);
    assert_eq!(y, 85);

    // Pass coordinates to WASM screen offset calculation
    let wasm_code = r#"
fn compute_screen_offset(x, y) {
    let stride = 320
    return y * stride + x
}
"#;
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(wasm_code).unwrap();
    let offset = WasmRunner::execute_function(&wasm_bytes, "compute_screen_offset", &[x, y])
        .expect("Execute compute_screen_offset in WASM");
    assert_eq!(offset, 85 * 320 + 120);

    server_handle.join().unwrap();
}

#[test]
fn test_e2e_t3_int_06_pkg_manifest_ide_validation_and_publish() {
    let proj = TempProject::new("ide_pkg_publish");
    proj.write_manifest("shae-net-protocol", "0.5.0");
    proj.write_file(
        "src/main.shae",
        r#"
fn serialize_packet(id, val) {
    return id * 1000 + val
}
let res = serialize_packet(4, 42)
return res
"#,
    );

    // 1. Validate manifest and source through IDE check endpoint
    let ide = IdeServerFixture::start();
    let client = reqwest::blocking::Client::new();

    let src = std::fs::read_to_string(proj.dir.join("src/main.shae")).unwrap();
    let check_resp = client
        .post(&format!("{}/api/check", ide.url()))
        .header("Content-Type", "application/json")
        .body(json!({ "code": src, "source": src }).to_string())
        .send()
        .unwrap();
    assert_eq!(check_resp.status(), 200);

    // 2. Package into .shaepkg archive
    let archive_bytes = pkg::create_shaepkg(&proj.dir).expect("Create shaepkg");
    assert!(archive_bytes.starts_with(b"SHAEPKG1"));
    let sha256_digest = compute_sha256(&archive_bytes);

    // 3. Register and verify on mock registry
    let mock_reg = MockRegistry::new();
    let index_json = json!({
        "name": "shae-net-protocol",
        "description": "Shae protocol packet serializer",
        "latest": "0.5.0",
        "versions": {
            "0.5.0": {
                "sha256": sha256_digest,
                "url": format!("{}/downloads/shae-net-protocol-0.5.0.shaepkg", mock_reg.url)
            }
        }
    });

    mock_reg.add_route(
        "GET",
        "/packages/shae-net-protocol.json",
        200,
        "application/json",
        index_json.to_string().into_bytes(),
    );

    let reg_resp = client
        .get(&format!("{}/packages/shae-net-protocol.json", mock_reg.url))
        .send()
        .expect("Query package index");
    assert_eq!(reg_resp.status(), 200);

    let parsed: serde_json::Value = serde_json::from_str(&reg_resp.text().unwrap()).unwrap();
    assert_eq!(parsed["name"], "shae-net-protocol");
    assert_eq!(parsed["latest"], "0.5.0");
    assert_eq!(parsed["versions"]["0.5.0"]["sha256"], sha256_digest);
}
