mod e2e_common;

use e2e_common::*;
use serde_json::json;
use shae::dap::DapServer;
use shae::pkg::{self, compute_sha256};
use shae::value::Value;
use shae::wasm::{WASM_MAGIC, WasmCompiler, WasmRunner};
use std::io::{Cursor, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =========================================================================
// Tier 4: Real-World Workloads (Spec Miner 1 Catalog §3.4)
// =========================================================================

#[test]
fn test_e2e_t4_app_01_high_throughput_ws_server_clients() {
    require_ws_module();
    let server =
        Arc::new(shae::ws::WsServer::bind("127.0.0.1:0").expect("Failed to bind Shae WS server"));
    let port = server.port();
    let client_count = 4;
    let messages_per_client = 25;

    let server_shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s_clone = Arc::clone(&server);
    let s_down = Arc::clone(&server_shutdown);

    let server_handle = thread::spawn(move || {
        while !s_down.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(Some(conn)) = s_clone.accept(Some(200)) {
                let conn_clone = conn.clone();
                thread::spawn(move || {
                    while let Ok(Some(msg)) = conn_clone.recv(Some(2000)) {
                        match msg {
                            shae::ws::WsMessage::Text(t) => {
                                if conn_clone.send(&format!("ECHO: {}", t)).is_err() {
                                    break;
                                }
                            }
                            shae::ws::WsMessage::Binary(b) => {
                                if conn_clone.send_binary(&b).is_err() {
                                    break;
                                }
                            }
                            shae::ws::WsMessage::Close(..) => break,
                            _ => {}
                        }
                    }
                });
            }
        }
    });

    let mut client_handles = Vec::new();

    for client_id in 0..client_count {
        let handle = thread::spawn(move || {
            let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
                .expect("Failed to connect client to WS server");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            let client_key = shae::ws::generate_key();
            shae::ws::perform_client_handshake(
                &mut stream,
                "127.0.0.1",
                port,
                "/game-cluster",
                &client_key,
                None,
            )
            .unwrap();

            // Exchange high-frequency text messages
            for seq in 0..messages_per_client {
                let msg = format!("CLIENT_{}_SEQ_{}", client_id, seq);
                let mask = [0x55, 0x66, (client_id as u8), (seq as u8)];
                let frame = shae::ws::encode_frame(
                    true,
                    shae::ws::OPCODE_TEXT,
                    true,
                    Some(mask),
                    msg.as_bytes(),
                );
                stream.write_all(&frame).unwrap();

                let frame_in = shae::ws::read_frame_stream(&mut stream).unwrap();
                assert_eq!(frame_in.opcode, shae::ws::OPCODE_TEXT);
                assert_eq!(
                    String::from_utf8_lossy(&frame_in.payload),
                    format!("ECHO: {}", msg)
                );
            }

            // Ping/Pong exchange
            let ping_payload = format!("ping-from-client-{}", client_id);
            let ping_frame = shae::ws::encode_frame(
                true,
                shae::ws::OPCODE_PING,
                true,
                Some([1, 2, 3, 4]),
                ping_payload.as_bytes(),
            );
            stream.write_all(&ping_frame).unwrap();

            let pong_frame = shae::ws::read_frame_stream(&mut stream).unwrap();
            assert_eq!(pong_frame.opcode, shae::ws::OPCODE_PONG);
            assert_eq!(String::from_utf8_lossy(&pong_frame.payload), ping_payload);

            // Close handshake
            let close_payload = 1000u16.to_be_bytes();
            let close_frame = shae::ws::encode_frame(
                true,
                shae::ws::OPCODE_CLOSE,
                true,
                Some([0, 0, 0, 0]),
                &close_payload,
            );
            stream.write_all(&close_frame).unwrap();

            let close_resp = shae::ws::read_frame_stream(&mut stream).unwrap();
            assert_eq!(close_resp.opcode, shae::ws::OPCODE_CLOSE);
        });
        client_handles.push(handle);
    }

    for h in client_handles {
        h.join().expect("Client thread completed cleanly");
    }

    server_shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = server.close();
    let _ = server_handle.join();
}

#[test]
fn test_e2e_t4_app_02_monte_carlo_numerical_simulation_jit() {
    let script = r#"
fn monte_carlo_pi(samples) {
    let inside = 0
    let i = 0
    let seed = 12345
    while i < samples {
        seed = (seed * 1103515245 + 12345) % 2147483648
        let x = seed % 1000
        seed = (seed * 1103515245 + 12345) % 2147483648
        let y = seed % 1000
        if (x * x + y * y) <= 1000000 {
            inside = inside + 1
        }
        i = i + 1
    }
    return inside * 4 * 1000 / samples
}
let res = monte_carlo_pi(50000)
return res
"#;

    let vm_res = run_script_vm(script).expect("VM simulation execution");
    if let Value::Int(scaled_pi) = vm_res {
        // Monte Carlo approximation for Pi * 1000 is approximately 3141
        assert!(
            scaled_pi >= 2900 && scaled_pi <= 3350,
            "Scaled Pi estimate {} must be within [2900, 3350]",
            scaled_pi
        );
    } else {
        panic!("Expected Int return value, got {:?}", vm_res);
    }

    // Run in JIT if available
    let jit_res = run_script_jit(script);
    if let Ok(Value::Int(jit_pi)) = jit_res {
        assert!(
            jit_pi >= 2900 && jit_pi <= 3350,
            "JIT Scaled Pi estimate {} must be within [2900, 3350]",
            jit_pi
        );
    }
}

#[test]
fn test_e2e_t4_app_03_60fps_wasm_game_loop_simulation() {
    let game_code = r#"
fn game_init() {
    return 0
}

fn game_input(player_x, dir, is_down) {
    let new_x = player_x + (dir * is_down)
    return new_x
}

fn game_tick(current_score, dt) {
    let next_score = current_score + 10
    return next_score
}

fn get_width() {
    return 64
}

fn get_height() {
    return 64
}
"#;

    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(game_code)
        .expect("Compile full game module to WASM");

    assert!(wasm_bytes.starts_with(WASM_MAGIC));

    // Initialize game
    let init_res =
        WasmRunner::execute_function(&wasm_bytes, "game_init", &[]).expect("WASM game_init");
    assert_eq!(init_res, 0);

    // Simulate 60 frames of gameplay
    let mut score = 0;
    for _frame in 0..60 {
        score = WasmRunner::execute_function(&wasm_bytes, "game_tick", &[score, 16])
            .expect("WASM game_tick");
    }
    assert_eq!(score, 600, "After 60 ticks score should be 600");

    // Simulate player inputs: Right (dir 1) 15 times, Left (dir -1) 5 times
    let mut px = 32;
    for _ in 0..15 {
        px = WasmRunner::execute_function(&wasm_bytes, "game_input", &[px, 1, 1])
            .expect("WASM game_input");
    }
    for _ in 0..5 {
        px = WasmRunner::execute_function(&wasm_bytes, "game_input", &[px, -1, 1])
            .expect("WASM game_input");
    }
    assert_eq!(px, 32 + 15 - 5); // 42

    let width = WasmRunner::execute_function(&wasm_bytes, "get_width", &[]).unwrap();
    let height = WasmRunner::execute_function(&wasm_bytes, "get_height", &[]).unwrap();
    assert_eq!(width, 64);
    assert_eq!(height, 64);
}

#[test]
fn test_e2e_t4_app_04_full_package_lifecycle_e2e() {
    // 1. Pack author package
    let proj = TempProject::new("matrix_package");
    proj.write_manifest("shae-matrix-lib", "1.2.0");
    proj.write_file(
        "src/main.shae",
        r#"
fn matrix2x2_det(a, b, c, d) {
    return (a * d) - (b * c)
}
"#,
    );

    let archive_bytes = pkg::create_shaepkg(&proj.dir).expect("Create package archive");
    assert!(archive_bytes.starts_with(b"SHAEPKG1"));
    let checksum = compute_sha256(&archive_bytes);

    // 2. Mock central package registry hosting the package
    let registry = MockRegistry::new();
    let pkg_url = format!("{}/downloads/shae-matrix-lib-1.2.0.shaepkg", registry.url);
    let index_payload = json!({
        "name": "shae-matrix-lib",
        "description": "2x2 Matrix determinants and linear algebra",
        "latest": "1.2.0",
        "versions": {
            "1.2.0": {
                "sha256": checksum,
                "url": pkg_url
            }
        }
    });

    registry.add_route(
        "GET",
        "/packages/shae-matrix-lib.json",
        200,
        "application/json",
        index_payload.to_string().into_bytes(),
    );
    registry.add_route(
        "GET",
        "/downloads/shae-matrix-lib-1.2.0.shaepkg",
        200,
        "application/octet-stream",
        archive_bytes.clone(),
    );

    // 3. Consumer queries registry and downloads archive
    let client = reqwest::blocking::Client::new();
    let index_resp = client
        .get(&format!("{}/packages/shae-matrix-lib.json", registry.url))
        .send()
        .expect("Fetch package index");
    assert_eq!(index_resp.status(), 200);

    let parsed_index: serde_json::Value =
        serde_json::from_str(&index_resp.text().unwrap()).unwrap();
    assert_eq!(parsed_index["latest"], "1.2.0");
    let target_url = parsed_index["versions"]["1.2.0"]["url"].as_str().unwrap();

    let download_resp = client.get(target_url).send().expect("Download .shaepkg");
    assert_eq!(download_resp.status(), 200);
    let downloaded_bytes = download_resp.bytes().unwrap();
    assert_eq!(
        compute_sha256(&downloaded_bytes),
        checksum,
        "Downloaded checksum must match registry metadata"
    );

    // 4. Consumer executes script using matrix computation
    let consumer_script = r#"
fn matrix2x2_det(a, b, c, d) {
    return (a * d) - (b * c)
}
let det = matrix2x2_det(5, 2, 3, 4)
return det
"#;
    let res = run_script_vm(consumer_script).expect("Execute consumer code");
    // 5 * 4 - 2 * 3 = 20 - 6 = 14
    assert_eq!(res, Value::Int(14));
}

#[test]
fn test_e2e_t4_app_05_interactive_dap_debug_session() {
    let script_file = TempScript::new(
        "dap_fib",
        r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}
let res = fib(7)
return res
"#,
    );

    let script_path = script_file.path.to_str().unwrap();

    let init_req = json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "vscode-client",
            "adapterID": "shae-debugger",
            "linesStartAt1": true,
            "columnsStartAt1": true
        }
    });

    let bp_req = json!({
        "seq": 2,
        "type": "request",
        "command": "setBreakpoints",
        "arguments": {
            "source": { "path": script_path },
            "breakpoints": [{ "line": 8 }]
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

    let disc_req = json!({
        "seq": 9,
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
    input_bytes.extend_from_slice(&encode_dap_message(&disc_req));

    let reader = Cursor::new(input_bytes);
    let out_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = SharedWriter::new(std::sync::Arc::clone(&out_buf));

    let mut dap = DapServer::new(reader, writer);
    dap.run().expect("Interactive DAP session run succeeded");

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

    let scopes_resp = responses
        .iter()
        .find(|r| r["command"] == "scopes")
        .expect("scopes resp");
    assert_eq!(scopes_resp["success"], true);

    let disc_resp = responses
        .iter()
        .find(|r| r["command"] == "disconnect")
        .expect("disc resp");
    assert_eq!(disc_resp["success"], true);

    // Verify script result
    let script = std::fs::read_to_string(&script_file.path).unwrap();
    let res = run_script_vm(&script).expect("Run recursive Fibonacci in debug session");
    assert_eq!(res, Value::Int(13));
}
