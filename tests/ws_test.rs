//! Comprehensive Integration Tests for Shae 'std:ws' RFC 6455 Engine.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ring::digest::{SHA256, digest};
use shae::value::Value;
use shae::ws::{
    CLOSE_NORMAL, CLOSE_PROTOCOL_ERROR, OPCODE_BINARY, OPCODE_CLOSE, OPCODE_PING, OPCODE_PONG,
    OPCODE_TEXT, Role, WS_GUID, WsConnection, WsMessage, WsServer, compute_accept_key,
    decode_frame, encode_frame, generate_key, parse_ws_url, perform_client_handshake,
    perform_server_handshake,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// 1. RFC 6455 Handshake & Key Derivation Tests
// ---------------------------------------------------------------------------

#[test]
fn test_ws_rfc6455_guid_and_accept_vector() {
    assert_eq!(WS_GUID, "258EAFA5-E914-47DA-95CA-C5AB0DC85B11");

    // Test vector from RFC 6455 §1.3
    let test_key = "dGhlIHNhbXBsZSBub25jZQ==";
    let expected_accept = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
    let computed = compute_accept_key(test_key);
    assert_eq!(computed, expected_accept);

    // Random key generation format
    let key = generate_key();
    let decoded = BASE64_STANDARD.decode(&key).expect("Valid base64 key");
    assert_eq!(decoded.len(), 16);
}

#[test]
fn test_ws_url_parsing() {
    let (h1, p1, path1) = parse_ws_url("ws://localhost:8080/chat").unwrap();
    assert_eq!(h1, "localhost");
    assert_eq!(p1, 8080);
    assert_eq!(path1, "/chat");

    let (h2, p2, path2) = parse_ws_url("ws://127.0.0.1:9000/").unwrap();
    assert_eq!(h2, "127.0.0.1");
    assert_eq!(p2, 9000);
    assert_eq!(path2, "/");

    let (h3, p3, path3) = parse_ws_url("ws://example.com/live?room=1").unwrap();
    assert_eq!(h3, "example.com");
    assert_eq!(p3, 80);
    assert_eq!(path3, "/live?room=1");

    let (h4, p4, path4) = parse_ws_url("wss://secure.io:8443").unwrap();
    assert_eq!(h4, "secure.io");
    assert_eq!(p4, 8443);
    assert_eq!(path4, "/");
}

#[test]
fn test_ws_client_server_tcp_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (path, key) = perform_server_handshake(&mut stream).unwrap();
        assert_eq!(path, "/handshake-test");
        assert!(!key.is_empty());
    });

    let mut client_stream = TcpStream::connect(addr).unwrap();
    let client_key = generate_key();
    perform_client_handshake(
        &mut client_stream,
        "127.0.0.1",
        addr.port(),
        "/handshake-test",
        &client_key,
        None,
    )
    .unwrap();

    server_handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// 2. RFC 6455 Frame Encoding & Decoding Tests
// ---------------------------------------------------------------------------

#[test]
fn test_ws_frame_encoding_and_decoding_all_opcodes() {
    let mask = Some([0xAA, 0xBB, 0xCC, 0xDD]);

    // Text frame (OpCode 0x1)
    let text_payload = b"Hello RFC 6455!";
    let enc_text = encode_frame(true, OPCODE_TEXT, true, mask, text_payload);
    let (fin, op, dec_text) = decode_frame(&enc_text).unwrap();
    assert!(fin);
    assert_eq!(op, OPCODE_TEXT);
    assert_eq!(dec_text, text_payload);

    // Binary frame (OpCode 0x2)
    let bin_payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let enc_bin = encode_frame(true, OPCODE_BINARY, false, None, &bin_payload);
    let (fin, op, dec_bin) = decode_frame(&enc_bin).unwrap();
    assert!(fin);
    assert_eq!(op, OPCODE_BINARY);
    assert_eq!(dec_bin, bin_payload);

    // Ping frame (OpCode 0x9)
    let ping_payload = b"heartbeat";
    let enc_ping = encode_frame(true, OPCODE_PING, true, mask, ping_payload);
    let (fin, op, dec_ping) = decode_frame(&enc_ping).unwrap();
    assert!(fin);
    assert_eq!(op, OPCODE_PING);
    assert_eq!(dec_ping, ping_payload);

    // Pong frame (OpCode 0xA)
    let pong_payload = b"heartbeat";
    let enc_pong = encode_frame(true, OPCODE_PONG, false, None, pong_payload);
    let (fin, op, dec_pong) = decode_frame(&enc_pong).unwrap();
    assert!(fin);
    assert_eq!(op, OPCODE_PONG);
    assert_eq!(dec_pong, pong_payload);

    // Close frame (OpCode 0x8)
    let mut close_payload = Vec::new();
    close_payload.extend_from_slice(&1000u16.to_be_bytes());
    close_payload.extend_from_slice(b"Normal Closure");
    let enc_close = encode_frame(true, OPCODE_CLOSE, true, mask, &close_payload);
    let (fin, op, dec_close) = decode_frame(&enc_close).unwrap();
    assert!(fin);
    assert_eq!(op, OPCODE_CLOSE);
    assert_eq!(dec_close, close_payload);
}

#[test]
fn test_ws_payload_length_boundaries() {
    let mask = Some([0x12, 0x34, 0x56, 0x78]);

    // 1. Short payload: 0 bytes and 125 bytes (7-bit length)
    let p0 = b"";
    let f0 = encode_frame(true, OPCODE_TEXT, true, mask, p0);
    assert_eq!(f0[1] & 0x7F, 0);
    let (_, _, d0) = decode_frame(&f0).unwrap();
    assert_eq!(d0, p0);

    let p125 = vec![0x41; 125];
    let f125 = encode_frame(true, OPCODE_TEXT, true, mask, &p125);
    assert_eq!(f125[1] & 0x7F, 125);
    let (_, _, d125) = decode_frame(&f125).unwrap();
    assert_eq!(d125, p125);

    // 2. Medium extended payload: 126 bytes and 65,535 bytes (16-bit extended length)
    let p126 = vec![0x42; 126];
    let f126 = encode_frame(true, OPCODE_TEXT, true, mask, &p126);
    assert_eq!(f126[1] & 0x7F, 126);
    let len126 = u16::from_be_bytes([f126[2], f126[3]]);
    assert_eq!(len126, 126);
    let (_, _, d126) = decode_frame(&f126).unwrap();
    assert_eq!(d126, p126);

    let p65k = vec![0x43; 65535];
    let f65k = encode_frame(true, OPCODE_BINARY, false, None, &p65k);
    assert_eq!(f65k[1] & 0x7F, 126);
    let len65k = u16::from_be_bytes([f65k[2], f65k[3]]);
    assert_eq!(len65k, 65535);
    let (_, _, d65k) = decode_frame(&f65k).unwrap();
    assert_eq!(d65k.len(), 65535);

    // 3. Large extended payload: 70,000 bytes (64-bit extended length)
    let mut p70k = Vec::with_capacity(70000);
    for i in 0..70000 {
        p70k.push((i % 256) as u8);
    }
    let f70k = encode_frame(true, OPCODE_BINARY, false, None, &p70k);
    assert_eq!(f70k[1] & 0x7F, 127);
    let len_bytes: [u8; 8] = f70k[2..10].try_into().unwrap();
    assert_eq!(u64::from_be_bytes(len_bytes), 70000);
    let (_, _, d70k) = decode_frame(&f70k).unwrap();
    assert_eq!(d70k.len(), 70000);

    let hash_expected = digest(&SHA256, &p70k);
    let hash_actual = digest(&SHA256, &d70k);
    assert_eq!(hash_expected.as_ref(), hash_actual.as_ref());
}

// ---------------------------------------------------------------------------
// 3. Full-Duplex Network Socket Tests (Pure Rust API)
// ---------------------------------------------------------------------------

#[test]
fn test_ws_echo_text_send_recv() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server
            .accept(Some(3000))
            .unwrap()
            .expect("Server accepted connection");
        let msg = conn
            .recv(Some(3000))
            .unwrap()
            .expect("Server received message");
        if let WsMessage::Text(text) = msg {
            conn.send(&format!("ECHO: {}", text)).unwrap();
        } else {
            panic!("Expected text message");
        }
        server.close().unwrap();
    });

    let client_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let mut client_s = client_stream;
    let client_key = generate_key();
    perform_client_handshake(&mut client_s, "127.0.0.1", port, "/", &client_key, None).unwrap();

    let client_conn = WsConnection::new(
        1001,
        client_s,
        Role::Client,
        format!("ws://127.0.0.1:{}", port),
    )
    .unwrap();

    client_conn.send("Hello WebSocket!").unwrap();
    let reply = client_conn
        .recv(Some(3000))
        .unwrap()
        .expect("Client received echo");

    match reply {
        WsMessage::Text(t) => assert_eq!(t, "ECHO: Hello WebSocket!"),
        _ => panic!("Expected text message from server"),
    }

    server_thread.join().unwrap();
}

#[test]
fn test_ws_binary_bytebuffer_send_recv() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        let msg = conn.recv(Some(3000)).unwrap().expect("Server recv");
        if let WsMessage::Binary(mut bytes) = msg {
            bytes.reverse();
            conn.send_binary(&bytes).unwrap();
        } else {
            panic!("Expected binary frame");
        }
    });

    let mut client_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let client_key = generate_key();
    perform_client_handshake(
        &mut client_stream,
        "127.0.0.1",
        port,
        "/",
        &client_key,
        None,
    )
    .unwrap();

    let client_conn = WsConnection::new(
        1002,
        client_stream,
        Role::Client,
        format!("ws://127.0.0.1:{}", port),
    )
    .unwrap();

    let original = vec![10u8, 20, 30, 40, 50];
    client_conn.send_binary(&original).unwrap();

    let reply = client_conn.recv(Some(3000)).unwrap().expect("Client recv");
    if let WsMessage::Binary(bytes) = reply {
        assert_eq!(bytes, vec![50u8, 40, 30, 20, 10]);
    } else {
        panic!("Expected binary frame reply");
    }

    server_thread.join().unwrap();
}

#[test]
fn test_ws_ping_pong_automatic_response() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        // Server calls recv(); when client sends Ping, recv() automatically sends Pong!
        let _ = conn.recv(Some(1000));
    });

    let mut client_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let client_key = generate_key();
    perform_client_handshake(
        &mut client_stream,
        "127.0.0.1",
        port,
        "/",
        &client_key,
        None,
    )
    .unwrap();

    let client_conn = WsConnection::new(
        1003,
        client_stream,
        Role::Client,
        format!("ws://127.0.0.1:{}", port),
    )
    .unwrap();

    let ping_payload = b"ping-probe-123";
    client_conn.ping(ping_payload).unwrap();

    // Client reads raw Pong frame from stream
    let mut r_stream = client_conn
        .reader
        .lock()
        .unwrap()
        .stream
        .try_clone()
        .unwrap();
    r_stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let frame = shae::ws::read_frame_stream(&mut r_stream).unwrap();

    assert_eq!(frame.opcode, OPCODE_PONG, "Must be OpCode 0xA (Pong)");
    assert_eq!(
        frame.payload, ping_payload,
        "Pong payload must match Ping exactly"
    );

    server_thread.join().unwrap();
}

#[test]
fn test_ws_graceful_close_handshake() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        // When client closes, recv() echoes Close frame and shuts down
        let res = conn.recv(Some(3000)).unwrap();
        assert!(matches!(res, Some(WsMessage::Close(..)) | None));
        assert!(!conn.is_open());
    });

    let mut client_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let client_key = generate_key();
    perform_client_handshake(
        &mut client_stream,
        "127.0.0.1",
        port,
        "/",
        &client_key,
        None,
    )
    .unwrap();

    let client_conn = WsConnection::new(
        1004,
        client_stream,
        Role::Client,
        format!("ws://127.0.0.1:{}", port),
    )
    .unwrap();

    assert!(client_conn.is_open());
    client_conn.close(Some(CLOSE_NORMAL), Some("Bye")).unwrap();
    assert!(!client_conn.is_open());

    server_thread.join().unwrap();
}

#[test]
fn test_ws_concurrent_client_connections() {
    let server = Arc::new(WsServer::bind("127.0.0.1:0").unwrap());
    let port = server.port();
    let client_count = 6;
    let messages_per_client = 10;

    let server_clone = Arc::clone(&server);
    let server_handle = thread::spawn(move || {
        let mut server_conns = Vec::new();
        for _ in 0..client_count {
            if let Ok(Some(conn)) = server_clone.accept(Some(5000)) {
                server_conns.push(conn);
            }
        }
        assert_eq!(server_conns.len(), client_count);

        let mut worker_threads = Vec::new();
        for conn in server_conns {
            worker_threads.push(thread::spawn(move || {
                for _ in 0..messages_per_client {
                    if let Ok(Some(WsMessage::Text(msg))) = conn.recv(Some(3000)) {
                        conn.send(&format!("ACK: {}", msg)).unwrap();
                    }
                }
            }));
        }

        for wt in worker_threads {
            wt.join().unwrap();
        }
    });

    let mut client_threads = Vec::new();
    for c_id in 0..client_count {
        client_threads.push(thread::spawn(move || {
            let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            let key = generate_key();
            perform_client_handshake(&mut stream, "127.0.0.1", port, "/", &key, None).unwrap();
            let conn = WsConnection::new(
                2000 + (c_id as u64),
                stream,
                Role::Client,
                format!("ws://127.0.0.1:{}", port),
            )
            .unwrap();

            for m_id in 0..messages_per_client {
                let msg = format!("client_{}_msg_{}", c_id, m_id);
                conn.send(&msg).unwrap();
                let reply = conn.recv(Some(3000)).unwrap().expect("Client recv ACK");
                if let WsMessage::Text(ack) = reply {
                    assert_eq!(ack, format!("ACK: {}", msg));
                } else {
                    panic!("Expected text ACK");
                }
            }

            conn.close(Some(CLOSE_NORMAL), None).unwrap();
        }));
    }

    for ct in client_threads {
        ct.join().unwrap();
    }
    server_handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// 4. Shae Language Script Standard Library Integration Tests
// ---------------------------------------------------------------------------

#[test]
fn test_ws_shae_stdlib_module_import() {
    let script = r#"
use { connect, listen, send, recv, close, ping, is_open } from "std:ws"

let ws = use "std:ws"
ws != null
"#;
    let res = shae::run(script).expect("Module import failed in AST evaluator");
    assert_eq!(res, Value::Bool(true));

    let res_vm = shae::run_vm(script).expect("Module import failed in Bytecode VM");
    assert_eq!(res_vm, Value::Bool(true));
}

#[test]
fn test_ws_shae_script_method_syntax() {
    // Start background listener
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        let msg = conn.recv(Some(3000)).unwrap().expect("Server recv");
        if let WsMessage::Text(t) = msg {
            conn.send(&format!("ECHO: {}", t)).unwrap();
        }
    });

    let script = format!(
        r#"
let ws = use "std:ws"
let conn = ws.connect("ws://127.0.0.1:{}")
let open_before = conn.is_open()
conn.send("hello from shae script")
let reply = conn.recv(3000)
conn.close(1000, "Done")
let open_after = conn.is_open()

{{
    "open_before": open_before,
    "reply": reply,
    "open_after": open_after
}}
"#,
        port
    );

    let res = shae::run(&script).expect("Execution failed in Evaluator");
    if let Value::Map(m) = res {
        let guard = m.read().unwrap();
        assert_eq!(guard.get("open_before"), Some(&Value::Bool(true)));
        assert_eq!(
            guard.get("reply"),
            Some(&Value::String("ECHO: hello from shae script".to_string()))
        );
        assert_eq!(guard.get("open_after"), Some(&Value::Bool(false)));
    } else {
        panic!("Expected map result from Shae script");
    }

    server_thread.join().unwrap();
}

#[test]
fn test_ws_shae_script_binary_bytebuffer() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        let msg = conn.recv(Some(3000)).unwrap().expect("Server recv");
        if let WsMessage::Binary(bytes) = msg {
            conn.send_binary(&bytes).unwrap();
        }
    });

    let script = format!(
        r#"
let ws = use "std:ws"
let buf_mod = use "std:buffer"
let conn = ws.connect("ws://127.0.0.1:{}")

let buf = buf_mod.from_utf8("Shae Binary Stream")
conn.send_binary(buf)
let echoed_buf = conn.recv(3000)
conn.close()

echoed_buf != null
"#,
        port
    );

    let res = shae::run(&script).expect("Binary buffer exchange in Shae script");
    assert_eq!(res, Value::Bool(true));

    server_thread.join().unwrap();
}

#[test]
fn test_ws_shae_script_vm_execution() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.port();

    let server_thread = thread::spawn(move || {
        let conn = server.accept(Some(3000)).unwrap().expect("Server accept");
        let msg = conn.recv(Some(3000)).unwrap().expect("Server recv");
        if let WsMessage::Text(t) = msg {
            conn.send(&format!("VM_ECHO: {}", t)).unwrap();
        }
    });

    let script = format!(
        r#"
let ws = use "std:ws"
let conn = ws.connect("ws://127.0.0.1:{}")
conn.send("Testing Bytecode VM")
let reply = conn.recv(3000)
conn.close()
reply
"#,
        port
    );

    let res = shae::run_vm(&script).expect("Execution failed in VM");
    assert_eq!(
        res,
        Value::String("VM_ECHO: Testing Bytecode VM".to_string())
    );

    server_thread.join().unwrap();
}

#[test]
fn test_ws_shae_script_pure_script_server_and_client() {
    let script = r#"
let ws = use "std:ws"
let server = ws.listen(0, fn(client) {
    let msg = client.recv(3000)
    client.send("Reply: " + msg)
})
let p = server.port
let conn = ws.connect("ws://127.0.0.1:{p}")

conn.send("Full-duplex Shae-to-Shae")
let final_msg = conn.recv(3000)
conn.close()
server.close()

final_msg
"#;

    let res = shae::run(script).expect("Full Shae script server and client exchange failed");
    assert_eq!(
        res,
        Value::String("Reply: Full-duplex Shae-to-Shae".to_string())
    );
}

// ---------------------------------------------------------------------------
// Adversarial RFC 6455 & RFC 7230 Hardening Tests
// ---------------------------------------------------------------------------

#[test]
fn test_ws_rfc7230_clean_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let client_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        perform_client_handshake(
            &mut stream,
            "127.0.0.1",
            port,
            "/",
            "dGhlIHNhbXBsZSBub25jZQ==",
            None,
        )
        .unwrap();
    });

    let (mut stream, _) = listener.accept().unwrap();
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while buf.len() < 4096 {
        stream.read_exact(&mut b).unwrap();
        buf.push(b[0]);
        if buf.ends_with(b"\r\n\r\n") || buf.ends_with(b"\n\n") {
            break;
        }
    }
    let req_str = String::from_utf8_lossy(&buf);
    for line in req_str.lines() {
        assert!(
            !line.starts_with(' '),
            "RFC 7230 violation: header line has leading space: '{}'",
            line
        );
        assert!(
            !line.starts_with('\t'),
            "RFC 7230 violation: header line has leading tab: '{}'",
            line
        );
    }

    // Now send response with clean headers
    let accept = compute_accept_key("dGhlIHNhbXBsZSBub25jZQ==");
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept
    );
    stream.write_all(resp.as_bytes()).unwrap();
    stream.flush().unwrap();

    client_handle.join().unwrap();
}

#[test]
fn test_ws_adversarial_unmasked_client_frame_rejected() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr.port();

    let client_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        perform_client_handshake(
            &mut stream,
            "127.0.0.1",
            port,
            "/",
            "dGhlIHNhbXBsZSBub25jZQ==",
            None,
        )
        .unwrap();
        // RFC 6455 violation: Client sends unmasked text frame
        let unmasked_frame = encode_frame(true, OPCODE_TEXT, false, None, b"unmasked-payload");
        stream.write_all(&unmasked_frame).unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(50));
    });

    let conn = server
        .accept(Some(2000))
        .unwrap()
        .expect("Server accept conn");
    let msg = conn
        .recv(Some(2000))
        .unwrap()
        .expect("Close frame received");
    match msg {
        WsMessage::Close(code, reason) => {
            assert_eq!(code, CLOSE_PROTOCOL_ERROR);
            assert!(reason.contains("Unmasked client frame"));
        }
        other => panic!("Expected Protocol Error close frame, got: {:?}", other),
    }

    client_handle.join().unwrap();
}

#[test]
fn test_ws_adversarial_masked_server_frame_rejected() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr.port();

    let server_handle = thread::spawn(move || {
        let conn = server
            .accept(Some(2000))
            .unwrap()
            .expect("Server accept conn");
        // RFC 6455 violation: Server sends masked frame to client
        let masked_frame = encode_frame(
            true,
            OPCODE_TEXT,
            true,
            Some([1, 2, 3, 4]),
            b"masked-server-payload",
        );
        let mut w = conn.writer.lock().unwrap();
        w.stream.write_all(&masked_frame).unwrap();
        w.stream.flush().unwrap();
    });

    let client = WsConnection::connect(&format!("ws://127.0.0.1:{}", port), None).unwrap();
    let msg = client
        .recv(Some(2000))
        .unwrap()
        .expect("Close frame received");
    match msg {
        WsMessage::Close(code, reason) => {
            assert_eq!(code, CLOSE_PROTOCOL_ERROR);
            assert!(reason.contains("Masked server frame"));
        }
        other => panic!("Expected Protocol Error close frame, got: {:?}", other),
    }

    server_handle.join().unwrap();
}

#[test]
fn test_ws_adversarial_control_frame_fin_and_length_enforced() {
    // 1. Fragmented control frame (FIN=false)
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr.port();

    let client_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        perform_client_handshake(
            &mut stream,
            "127.0.0.1",
            port,
            "/",
            "dGhlIHNhbXBsZSBub25jZQ==",
            None,
        )
        .unwrap();
        // FIN = false on Ping frame (violation)
        let frag_ping = encode_frame(false, OPCODE_PING, true, Some([1, 2, 3, 4]), b"frag-ping");
        stream.write_all(&frag_ping).unwrap();
        stream.flush().unwrap();
    });

    let conn = server
        .accept(Some(2000))
        .unwrap()
        .expect("Server accept conn");
    let msg = conn
        .recv(Some(2000))
        .unwrap()
        .expect("Close frame received");
    match msg {
        WsMessage::Close(code, reason) => {
            assert_eq!(code, CLOSE_PROTOCOL_ERROR);
            assert!(reason.contains("Fragmented control frame"));
        }
        other => panic!("Expected Protocol Error close frame, got: {:?}", other),
    }
    client_handle.join().unwrap();

    // 2. Control frame payload > 125 bytes
    let server2 = WsServer::bind("127.0.0.1:0").unwrap();
    let port2 = server2.local_addr.port();

    let client_handle2 = thread::spawn(move || {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port2)).unwrap();
        perform_client_handshake(
            &mut stream,
            "127.0.0.1",
            port2,
            "/",
            "dGhlIHNhbXBsZSBub25jZQ==",
            None,
        )
        .unwrap();
        // Ping with 126 bytes (violation)
        let big_payload = vec![0x42; 126];
        let big_ping = encode_frame(true, OPCODE_PING, true, Some([1, 2, 3, 4]), &big_payload);
        stream.write_all(&big_ping).unwrap();
        stream.flush().unwrap();
    });

    let conn2 = server2
        .accept(Some(2000))
        .unwrap()
        .expect("Server accept conn");
    let msg2 = conn2
        .recv(Some(2000))
        .unwrap()
        .expect("Close frame received");
    match msg2 {
        WsMessage::Close(code, reason) => {
            assert_eq!(code, CLOSE_PROTOCOL_ERROR);
            assert!(reason.contains("Control frame payload > 125"));
        }
        other => panic!("Expected Protocol Error close frame, got: {:?}", other),
    }
    client_handle2.join().unwrap();
}

#[test]
fn test_ws_adversarial_non_zero_rsv_rejected() {
    // Frame with RSV1 = 1: byte 0 is 0x80 (FIN) | 0x40 (RSV1) | 0x01 (TEXT) = 0xC1
    let bad_frame = vec![0xC1, 0x04, b't', b'e', b's', b't'];
    let res = decode_frame(&bad_frame);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Non-zero RSV bits"));
}

#[test]
fn test_ws_server_resilience_to_bad_handshake() {
    let server = WsServer::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr.port();

    // 1. Connect a rogue HTTP client that sends non-WebSocket GET
    let rogue_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let rogue_req =
            b"GET /plain.html HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: curl/7.68.0\r\n\r\n";
        stream.write_all(rogue_req).unwrap();
        stream.flush().unwrap();

        let mut resp = [0u8; 64];
        let n = stream.read(&mut resp).unwrap_or(0);
        let resp_str = String::from_utf8_lossy(&resp[..n]);
        assert!(resp_str.contains("400 Bad Request"));
    });

    // 2. Legitimate client connecting immediately after rogue client
    let valid_client_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        let client = WsConnection::connect(&format!("ws://127.0.0.1:{}", port), None).unwrap();
        client.send("I am legitimate").unwrap();
        let reply = client.recv(Some(2000)).unwrap().expect("Valid reply");
        match reply {
            WsMessage::Text(s) => assert_eq!(s, "ACK: I am legitimate"),
            other => panic!("Unexpected message: {:?}", other),
        }
        client.close(Some(CLOSE_NORMAL), None).unwrap();
    });

    // Server accepts: first rogue connection is rejected internally, second valid connection succeeds
    let conn = server
        .accept(Some(3000))
        .unwrap()
        .expect("Server accepts valid client after rogue dropped");
    let msg = conn.recv(Some(2000)).unwrap().expect("Message received");
    match msg {
        WsMessage::Text(s) => {
            conn.send(&format!("ACK: {}", s)).unwrap();
        }
        other => panic!("Unexpected msg: {:?}", other),
    }

    rogue_handle.join().unwrap();
    valid_client_handle.join().unwrap();
}
