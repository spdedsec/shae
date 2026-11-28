use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use shae::value::Value;
use shae::web_http::{H2ClientConn, H3Client, H3Server, HttpClientPool};

fn free_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}

fn run_both(label: &str, script: &str, expected: Value) {
    let ev_res = shae::run(script).unwrap_or_else(|e| {
        panic!("[{}] AST Evaluator failed: {}", label, e);
    });
    assert_eq!(ev_res, expected, "[{}] AST Evaluator result mismatch", label);

    let vm_res = shae::run_vm(script).unwrap_or_else(|e| {
        panic!("[{}] Bytecode VM failed: {}", label, e);
    });
    assert_eq!(vm_res, expected, "[{}] Bytecode VM result mismatch", label);
}

#[test]
fn test_h1_pool_reuse_stale_retry_pipelining_and_head() {
    // Spawn a raw HTTP/1.1 test server that supports keep-alive, chunked, HEAD, 100-continue, and pipelining
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_srv = stop_flag.clone();

    let srv_thread = thread::spawn(move || {
        use shae::web_http::{parse_h1_request, serialize_h1_response, BufStream, H1Limits, WireResponse};
        let mut conn_num = 0usize;
        for stream_res in listener.incoming() {
            if stop_srv.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream_res else { break; };
            conn_num += 1;
            let current_conn = conn_num;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut bs = BufStream::new(stream);
            loop {
                let Ok(req) = parse_h1_request(&mut bs, H1Limits::default(), true) else {
                    break;
                };
                if req.target == "/close-after-first" && current_conn == 1 {
                    // Reply then abruptly drop socket on next loop to trigger stale detection/retry!
                    let resp = WireResponse {
                        status: 200,
                        headers: IndexMap::new(),
                        body: b"first".to_vec(),
                        trailers: IndexMap::new(),
                    };
                    let wire = serialize_h1_response(&resp, &req.method, true).unwrap();
                    let _ = bs.write_all(&wire);
                    let _ = bs.flush();
                    // Wait briefly so client checks socket back in, then drop it!
                    thread::sleep(Duration::from_millis(20));
                    break;
                }
                if req.target == "/head-check" {
                    let mut h = IndexMap::new();
                    h.insert("content-length".to_string(), vec!["1234".to_string()]);
                    let resp = WireResponse {
                        status: 200,
                        headers: h,
                        body: b"should-not-be-sent".to_vec(),
                        trailers: IndexMap::new(),
                    };
                    let wire = serialize_h1_response(&resp, "HEAD", true).unwrap();
                    let _ = bs.write_all(&wire);
                    let _ = bs.flush();
                    continue;
                }
                if req.target == "/echo" {
                    let resp = WireResponse {
                        status: 200,
                        headers: IndexMap::new(),
                        body: req.body,
                        trailers: IndexMap::new(),
                    };
                    let wire = serialize_h1_response(&resp, &req.method, true).unwrap();
                    let _ = bs.write_all(&wire);
                    let _ = bs.flush();
                    continue;
                }
                let resp = WireResponse {
                    status: 200,
                    headers: IndexMap::new(),
                    body: format!("ok:{}:conn{}", req.target, current_conn).into_bytes(),
                    trailers: IndexMap::new(),
                };
                let wire = serialize_h1_response(&resp, &req.method, true).unwrap();
                let _ = bs.write_all(&wire);
                let _ = bs.flush();
            }
        }
    });

    let pool = HttpClientPool::new(1);
    let base = format!("http://{}", addr);

    // 1. Stale connection eviction + retry test
    let r1 = pool
        .request("GET", &format!("{}/close-after-first", base), &IndexMap::new(), &[], false, false)
        .unwrap();
    assert_eq!(r1.body, b"first");
    // Give server thread time to close conn 1 so pooled socket becomes stale (EOF)
    thread::sleep(Duration::from_millis(60));
    let r2 = pool
        .request("GET", &format!("{}/after-stale", base), &IndexMap::new(), &[], false, false)
        .unwrap();
    assert_eq!(r2.body, b"ok:/after-stale:conn2");

    // 2. Keep-alive pool reuse (conn2 reused)
    let r3 = pool
        .request("GET", &format!("{}/reuse1", base), &IndexMap::new(), &[], false, false)
        .unwrap();
    assert_eq!(r3.body, b"ok:/reuse1:conn2");

    let stats = pool.stats_map();
    if let Value::Map(m) = stats {
        let g = m.read().unwrap();
        let hits = match g.get("hits") { Some(Value::Int(n)) => *n, _ => 0 };
        let stale = match g.get("stale_evictions") { Some(Value::Int(n)) => *n, _ => 0 };
        assert!(hits >= 1, "Expected pool hits >= 1, got {}", hits);
        assert!(stale >= 1, "Expected stale_evictions >= 1, got {}", stale);
    }

    // 3. HEAD response with Content-Length: 1234 must read 0 body bytes and keep connection healthy!
    let r_head = pool
        .request("HEAD", &format!("{}/head-check", base), &IndexMap::new(), &[], false, false)
        .unwrap();
    assert_eq!(r_head.status, 200);
    assert!(r_head.body.is_empty());

    // 4. Expect: 100-continue over pooled connection
    let mut exp_hdr = IndexMap::new();
    exp_hdr.insert("expect".to_string(), vec!["100-continue".to_string()]);
    let r_echo = pool
        .request("POST", &format!("{}/echo", base), &exp_hdr, b"hello-100-continue", false, false)
        .unwrap();
    assert_eq!(r_echo.body, b"hello-100-continue");

    // 5. True HTTP/1.1 pipelining (5 requests back-to-back on 1 socket)
    let pipe_reqs: Vec<_> = (0..5)
        .map(|i| ("GET".to_string(), format!("/pipe/{}", i), IndexMap::new(), Vec::new()))
        .collect();
    let pipe_resps = pool.pipeline(&base, &pipe_reqs, false).unwrap();
    assert_eq!(pipe_resps.len(), 5);
    for (i, pr) in pipe_resps.iter().enumerate() {
        assert_eq!(pr.status, 200);
        assert_eq!(String::from_utf8_lossy(&pr.body), format!("ok:/pipe/{}:conn2", i));
    }

    stop_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = TcpStream::connect(addr);
    srv_thread.join().unwrap();
}

#[test]
fn test_h1_server_shutdown_speed_and_routing() {
    let script = r#"
use * from "std:web:http"
let srv = create_server()
srv.get("/users/:id", fn(req) {
    return { status: 200, json: { id: req.params.id, q: req.query_params.role } }
})
let info = srv.listen("127.0.0.1:0")
let res = get("http://" + info.local_addr + "/users/42?role=admin")
let stopped = srv.stop()
[res.status, res.json.id, res.json.q, stopped]
"#;
    let t0 = Instant::now();
    run_both(
        "h1_server_shutdown_and_routing",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("42".into()),
            Value::String("admin".into()),
            Value::Bool(true),
        ]))),
    );
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "Both runs + clean stop() should complete very quickly"
    );
}

#[test]
fn test_h2_multiplexing_large_flow_control_and_tls_alpn() {
    // 1. Plain h2c concurrent multiplexing + >64KiB flow control via Shae
    let script = r#"
use * from "std:web:http"
use { sleep } from "std:time"
let big_chunk = "A"
let i = 0
while i < 17 {
    big_chunk = big_chunk + big_chunk
    i = i + 1
}
// 2^17 = 131072 bytes (> 65535 initial window)
let srv = h2_listen("127.0.0.1:0", {
    "GET /fast": fn(req) { return { status: 200, body: "fast-ok" } },
    "GET /slow": fn(req) {
        sleep(25)
        return { status: 200, body: "slow-ok" }
    },
    "POST /large": fn(req) {
        return { status: 200, body: req.body }
    }
})

let h2 = h2_connect("h2c://" + srv.local_addr)
let ping_ok = h2.ping("12345678")
let resps = h2.multiplex([
    { method: "GET", path: "/slow" },
    { method: "GET", path: "/fast" },
    { method: "GET", path: "/fast" },
    { method: "POST", path: "/large", body: big_chunk }
])
let st = h2.stats()
h2.close()
srv.stop()
[ping_ok, resps[0].body, resps[1].body, resps[2].body, resps[3].body.len, st.responses_recv]
"#;
    run_both(
        "h2_multiplex_and_flow_control",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Bool(true),
            Value::String("slow-ok".into()),
            Value::String("fast-ok".into()),
            Value::String("fast-ok".into()),
            Value::Int(131072),
            Value::Int(4),
        ]))),
    );

    // 2. TLS ALPN "h2" with ephemeral rcgen certificate
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let tmp_dir = std::env::temp_dir().join(format!("shae_h2_tls_{}", fastrand::u64(..)));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let cert_path = tmp_dir.join("cert.pem");
    let key_path = tmp_dir.join("key.pem");
    std::fs::write(&cert_path, &cert_pem).unwrap();
    std::fs::write(&key_path, &key_pem).unwrap();

    let tls_script = format!(
        r#"
use * from "std:web:http"
let srv = h2_listen("127.0.0.1:0", fn(req) {{
    return {{ status: 200, json: {{ proto: req.version, path: req.path }} }}
}}, {{ cert: {:?}, key: {:?} }})
let client = h2_connect("https://localhost:" + str(srv.port), {{ ca: {:?} }})
let r = client.get("/alpn-test")
let alpn = client.alpn
client.close()
srv.stop()
[r.status, r.json.proto, r.json.path, alpn]
"#,
        cert_path.to_string_lossy(),
        key_path.to_string_lossy(),
        cert_path.to_string_lossy()
    );
    let res = shae::run_vm(&tls_script).expect("TLS ALPN h2 script failed");
    assert_eq!(
        res,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("HTTP/2".into()),
            Value::String("/alpn-test".into()),
            Value::String("h2".into()),
        ])))
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_h2_rust_8_concurrent_streams_interleaved() {
    // Also verify H2ClientConn directly against an h2_listen server started via Shae
    let script = r#"
use { h2_listen, h2_connect } from "std:web:http"
let srv = h2_listen("127.0.0.1:0", {
    "GET /s/:id": fn(req) { return { status: 200, body: "stream-" + req.params.id } }
})
let c = h2_connect("http://" + srv.local_addr)
let batch = []
let i = 0
while i < 8 {
    batch.push({ method: "GET", path: "/s/" + str(i) })
    i = i + 1
}
let out = c.multiplex(batch)
c.close()
srv.stop()
let ok = true
i = 0
while i < 8 {
    if out[i].body != "stream-" + str(i) {
        ok = false
    }
    i = i + 1
}
[ok, out.len]
"#;
    run_both(
        "h2_8_concurrent_streams_selective_import",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Bool(true),
            Value::Int(8),
        ]))),
    );
    // Keep unused import check clean for H2ClientConn in tests
    let _ = std::any::type_name::<H2ClientConn>();
    let _ = free_addr();
}

#[test]
fn test_h3_udp_loopback_multistream_and_large_payload() {
    let script = r#"
use * from "std:http3"
let prefix = "quic:"
fn format_tag(p, name) {
    return p + name
}
let srv = listen("127.0.0.1:0", {
    "GET /hello/:name": fn(req) {
        return { status: 200, json: { tag: format_tag(prefix, req.params.name), ver: req.version } }
    },
    "POST /echo": fn(req) {
        return { status: 201, body: req.body }
    }
})
let client = connect("h3://" + srv.local_addr)
let r1 = client.get("/hello/world")
let big = "Q"
let i = 0
while i < 12 {
    big = big + big
    i = i + 1
}
// 4096 bytes > 1150 MTU chunk -> exercises multi-packet QUIC STREAM fragmentation & reassembly
let r2 = client.post("/echo", big)
let batch = client.multiplex([
    { method: "GET", path: "/hello/a" },
    { method: "GET", path: "/hello/b" }
])
client.close()
srv.stop()
[r1.status, r1.json.tag, r1.json.ver, r2.status, r2.body.len, batch[0].json.tag, batch[1].json.tag]
"#;
    run_both(
        "h3_udp_loopback_shae",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("quic:world".into()),
            Value::String("HTTP/3".into()),
            Value::Int(201),
            Value::Int(4096),
            Value::String("quic:a".into()),
            Value::String("quic:b".into()),
        ]))),
    );

    let _ = std::any::type_name::<H3Server>();
    let _ = std::any::type_name::<H3Client>();
}

#[test]
fn test_dual_engine_closure_upvalues_and_top_level_fns() {
    // Specifically stress VM upvalue closing on closures passed to BoundMethods + top-level fn visibility
    let script = r#"
use * from "std:web:http"

fn compute_badge(secret, id) {
    return secret + "-" + id
}

fn make_server_and_test() {
    let local_secret = "closed-upvalue"
    let srv = create_server()
    srv.get("/badge/:id", fn(req) {
        return {
            status: 200,
            headers: { "x-badge": compute_badge(local_secret, req.params.id) },
            json: { badge: compute_badge(local_secret, req.params.id) }
        }
    })
    return srv
}

let srv = make_server_and_test()
// `local_secret` frame has returned -> VM MUST have closed upvalues inside route closure!
srv.listen("127.0.0.1:0")
let pool = create_pool()
let r1 = pool.get("http://" + srv.local_addr + "/badge/99")
let r2 = pool.get("http://" + srv.local_addr + "/badge/100")
let st = pool.stats()
pool.close()
srv.stop()
[r1.status, r1.headers["x-badge"], r1.json.badge, r2.json.badge, st.hits >= 1]
"#;
    run_both(
        "closure_upvalue_after_factory_return",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("closed-upvalue-99".into()),
            Value::String("closed-upvalue-99".into()),
            Value::String("closed-upvalue-100".into()),
            Value::Bool(true),
        ]))),
    );
}

#[test]
fn test_codecs_exposed_to_shae_and_legacy_std_http_intact() {
    let script = r#"
use * from "std:web:http"
let legacy = use "std:http"
let has_legacy = legacy.has("get") && legacy.has("post_json")

let vi = varint_encode(15293)
let vd = varint_decode(vi)

let hp = hpack_encode({ ":method": "GET", ":path": "/test", "x-shae": "fast" })
let hdec = hpack_decode(hp)

let qp = qpack_encode({ ":method": "POST", ":path": "/q", "x-q": "ok" })
let qdec = qpack_decode(qp)

[has_legacy, vd.value, vd.bytes_read, hdec["x-shae"], qdec["x-q"]]
"#;
    run_both(
        "codecs_and_legacy_std_http_intact",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Bool(true),
            Value::Int(15293),
            Value::Int(2),
            Value::String("fast".into()),
            Value::String("ok".into()),
        ]))),
    );
}


fn spawn_shae_server_with_handshake(script_template: &str) -> (String, std::path::PathBuf, std::path::PathBuf, thread::JoinHandle<()>) {
    let tmp_dir = std::env::temp_dir().join(format!("shae_web_http_it_{}", fastrand::u64(..)));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let addr_file = tmp_dir.join("addr.txt");
    let stop_file = tmp_dir.join("stop.flag");

    let script = script_template
        .replace("__ADDR_FILE__", &format!("{:?}", addr_file.to_string_lossy()))
        .replace("__STOP_FILE__", &format!("{:?}", stop_file.to_string_lossy()));

    let jh = thread::spawn(move || {
        shae::run_vm(&script).expect("Shae server script failed");
    });

    let start = Instant::now();
    let addr = loop {
        if let Ok(s) = std::fs::read_to_string(&addr_file) {
            let trimmed = s.trim().to_string();
            if !trimmed.is_empty() {
                break trimmed;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(10), "Timed out waiting for Shae server addr");
        thread::sleep(Duration::from_millis(10));
    };

    (addr, stop_file, tmp_dir, jh)
}

#[test]
fn test_h1_mid_body_timeout_does_not_desync_or_smuggle() {
    use std::io::Write;
    use shae::web_http::{parse_h1_response, BufStream, H1Limits};

    let (addr, stop_file, tmp_dir, jh) = spawn_shae_server_with_handshake(
        r#"
use * from "std:web:http"
use { write, exists } from "std:fs"
use { sleep } from "std:time"

let srv = listen("127.0.0.1:0", {
    "POST /echo": fn(req) { return { status: 200, body: req.body } },
    "GET /smuggle": fn(req) { return { status: 200, body: "SMUGGLED" } }
})
write(__ADDR_FILE__, srv.local_addr)
while !exists(__STOP_FILE__) {
    sleep(20)
}
srv.stop()
"#,
    );

    let mut stream = TcpStream::connect(&addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    let smuggled_body = b"GET /smuggle HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let head = format!(
        "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        smuggled_body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.flush().unwrap();

    // Pause longer than the server's 200ms SO_RCVTIMEO keep-alive poll interval!
    thread::sleep(Duration::from_millis(250));

    stream.write_all(smuggled_body).unwrap();
    stream.flush().unwrap();

    let mut bs = BufStream::new(stream);
    let resp = parse_h1_response(&mut bs, "POST", H1Limits::default())
        .expect("Server must keep reading delayed POST body instead of desyncing");
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.body, smuggled_body,
        "Delayed POST body must be returned verbatim, not executed as /smuggle"
    );

    // Verify connection remains cleanly synchronized for subsequent keep-alive request
    bs.write_all(b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\nok")
        .unwrap();
    bs.flush().unwrap();
    let resp2 = parse_h1_response(&mut bs, "POST", H1Limits::default()).unwrap();
    assert_eq!(resp2.status, 200);
    assert_eq!(resp2.body, b"ok");
    drop(bs);

    std::fs::write(&stop_file, "stop").unwrap();
    jh.join().unwrap();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_h2_concurrent_slow_and_fast_streams() {
    let (addr, stop_file, tmp_dir, jh) = spawn_shae_server_with_handshake(
        r#"
use * from "std:web:http"
use { write, exists } from "std:fs"
use { sleep } from "std:time"

let srv = h2_listen("127.0.0.1:0", {
    "GET /slow": fn(req) {
        sleep(250)
        return { status: 200, body: "slow-done" }
    },
    "GET /fast/:id": fn(req) {
        return { status: 200, body: "fast-" + req.params.id }
    }
})
write(__ADDR_FILE__, srv.local_addr)
while !exists(__STOP_FILE__) {
    sleep(20)
}
srv.stop()
"#,
    );

    let conn = std::sync::Arc::new(H2ClientConn::connect(&format!("http://{}", addr), false, None).unwrap());
    let slow_conn = conn.clone();
    let slow_t = thread::spawn(move || {
        let t0 = Instant::now();
        let r = slow_conn.request("GET", "/slow", &IndexMap::new(), b"").unwrap();
        (t0.elapsed(), r.body)
    });

    // Give /slow a slight head start so it definitely reaches the thread pool before /fast
    thread::sleep(Duration::from_millis(20));

    let mut fast_handles = Vec::new();
    for i in 0..6 {
        let c = conn.clone();
        fast_handles.push(thread::spawn(move || {
            let t0 = Instant::now();
            let r = c
                .request("GET", &format!("/fast/{}", i), &IndexMap::new(), b"")
                .unwrap();
            (i, t0.elapsed(), r.body)
        }));
    }

    let mut max_fast = Duration::ZERO;
    for fh in fast_handles {
        let (i, dt, body) = fh.join().unwrap();
        assert_eq!(body, format!("fast-{}", i).as_bytes());
        if dt > max_fast {
            max_fast = dt;
        }
    }

    let (slow_dt, slow_body) = slow_t.join().unwrap();
    assert_eq!(slow_body, b"slow-done");
    assert!(slow_dt >= Duration::from_millis(220), "slow took {:?}", slow_dt);
    assert!(
        max_fast < Duration::from_millis(160),
        "Fast streams serialized behind /slow! max_fast={:?}, slow_dt={:?}",
        max_fast,
        slow_dt
    );

    conn.close();
    std::fs::write(&stop_file, "stop").unwrap();
    jh.join().unwrap();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_h2_server_oversized_response_headers_continuation() {
    let script = r#"
use * from "std:web:http"

let big_hdr = "H"
let i = 0
while i < 15 {
    big_hdr = big_hdr + big_hdr
    i = i + 1
}
// 2^15 = 32768 bytes (> 16384 MAX_FRAME_SIZE -> requires HEADERS + CONTINUATION)

let srv = h2_listen("127.0.0.1:0", fn(req) {
    return {
        status: 200,
        headers: { "x-big": big_hdr },
        body: "hdrs-ok"
    }
})
let c = h2_connect("h2c://" + srv.local_addr)
let r = c.get("/")
c.close()
srv.stop()
[r.status, r.body, r.headers["x-big"].len]
"#;
    run_both(
        "h2_server_response_continuation_headers",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("hdrs-ok".into()),
            Value::Int(32768),
        ]))),
    );
}

#[test]
fn test_h3_rustls_quic_tls_end_to_end() {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let tmp_dir = std::env::temp_dir().join(format!("shae_h3_tls_{}", fastrand::u64(..)));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let cert_path = tmp_dir.join("cert.pem");
    let key_path = tmp_dir.join("key.pem");
    std::fs::write(&cert_path, &cert_pem).unwrap();
    std::fs::write(&key_path, &key_pem).unwrap();

    let script = format!(
        r#"
use * from "std:http3"
let srv = h3_listen("127.0.0.1:0", {{
    "GET /hello/:who": fn(req) {{
        return {{ status: 200, json: {{ who: req.params.who, ver: req.version }} }}
    }},
    "POST /echo": fn(req) {{
        return {{ status: 201, text: req.body }}
    }}
}}, {{ cert: {:?}, key: {:?} }})

let st = srv.stats()
let c1 = connect("h3://" + srv.local_addr, {{ tls: true, insecure: true }})
let r1 = c1.get("/hello/quic-tls")
let r2 = c1.post("/echo", "tls13-payload")
let batch = c1.multiplex([
    {{ method: "GET", path: "/hello/m1" }},
    {{ method: "GET", path: "/hello/m2" }}
])
c1.close()
srv.stop()

// Also verify h3_create_server().listen(addr, routes, {{ cert, key }}) 3rd-arg TLS config + CA verification
let srv2 = create_server()
srv2.listen("127.0.0.1:0", {{
    "GET /ca": fn(req) {{ return {{ status: 200, text: "verified" }} }}
}}, {{ cert: {:?}, key: {:?} }})
let c2 = connect("https://localhost:" + str(srv2.port), {{ tls: true, ca: {:?} }})
let r3 = c2.get("/ca")
c2.close()
srv2.stop()

[st.tls_configured, r1.status, r1.json.who, r1.json.ver, r2.status, r2.body, batch[0].json.who, batch[1].json.who, r3.status, r3.body]
"#,
        cert_path.to_string_lossy(),
        key_path.to_string_lossy(),
        cert_path.to_string_lossy(),
        key_path.to_string_lossy(),
        cert_path.to_string_lossy()
    );

    run_both(
        "h3_rustls_quic_tls_end_to_end",
        &script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Bool(true),
            Value::Int(200),
            Value::String("quic-tls".into()),
            Value::String("HTTP/3".into()),
            Value::Int(201),
            Value::String("tls13-payload".into()),
            Value::String("m1".into()),
            Value::String("m2".into()),
            Value::Int(200),
            Value::String("verified".into()),
        ]))),
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_h2_client_corrupt_hpack_fails_promptly_with_goaway() {
    use std::io::{Read, Write};
    use shae::web_http::{H2Frame, H2_PREFACE};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let srv_jh = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        let mut preface = [0u8; 24];
        sock.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, H2_PREFACE);
        // Send server SETTINGS then corrupt HPACK HEADERS (index 0 = 0x80 -> COMPRESSION_ERROR)
        sock.write_all(&H2Frame::Settings { ack: false, params: vec![] }.encode()).unwrap();
        sock.write_all(
            &H2Frame::Headers {
                stream_id: 1,
                end_stream: true,
                end_headers: true,
                priority: None,
                header_block: vec![0x80],
                pad_len: 0,
            }
            .encode(),
        )
        .unwrap();
        sock.flush().unwrap();
        // Keep socket open for up to 1.5s while reading client GOAWAY
        let mut buf = [0u8; 256];
        let mut saw_goaway = false;
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(1200) {
            match sock.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if buf[..n].windows(2).any(|w| w == [0x07, 0x00]) {
                        saw_goaway = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        saw_goaway
    });

    let conn = H2ClientConn::connect(&format!("http://{}", addr), false, None).unwrap();
    let t0 = Instant::now();
    let res = conn.request("GET", "/", &IndexMap::new(), b"");
    let dt = t0.elapsed();
    assert!(res.is_err(), "Corrupt HPACK must fail immediately");
    assert!(
        dt < Duration::from_millis(800),
        "Client hung instead of failing promptly on HPACK error: {:?}",
        dt
    );
    conn.close();
    assert!(srv_jh.join().unwrap(), "Server should receive client GOAWAY");
}

#[test]
fn test_m6_http_regressions_text_key_properties_and_gc_parity() {
    let script = r#"
use * from "std:web:http"

let srv1 = create_server()
srv1.get("/text", fn(req) {
    return { status: 200, text: "hello-text" }
})
srv1.listen("127.0.0.1:0")
let p = create_pool()
let r1 = p.get("http://" + srv1.local_addr + "/text")
let port1 = srv1.port
let addr1 = srv1.local_addr
p.close()
srv1.stop()

let srv3 = h3_create_server()
srv3.listen("127.0.0.1:0", fn(req) {
    return { status: 200, text: "h3-text" }
})
let c3 = h3_connect("h3://" + srv3.local_addr)
let r3 = c3.get("/")
let port3 = srv3.port
let addr3 = srv3.local_addr
c3.close()
srv3.stop()

[r1.status, r1.body, port1 > 0, addr1.len > 0, r3.status, r3.body, port3 > 0, addr3.len > 0]
"#;
    run_both(
        "text_key_and_server_properties",
        script,
        Value::Array(std::sync::Arc::new(std::sync::RwLock::new(vec![
            Value::Int(200),
            Value::String("hello-text".into()),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int(200),
            Value::String("h3-text".into()),
            Value::Bool(true),
            Value::Bool(true),
        ]))),
    );

    // Also verify VM GcMap / GcString / GcArray materialization into web_http builtins
    let mut vm = shae::vm::VM::new();
    let body_str = vm.heap.alloc_string("gc-body-ok".to_string());
    let mut hdr_inner = IndexMap::new();
    hdr_inner.insert("x-gc".to_string(), Value::GcString(body_str));
    let hdr_ref = vm.heap.alloc_map(hdr_inner);
    let mut map_inner = IndexMap::new();
    map_inner.insert(":method".to_string(), Value::String("GET".to_string()));
    map_inner.insert("x-gc".to_string(), Value::GcString(body_str));
    let gc_map_ref = vm.heap.alloc_map(map_inner);
    let _ = hdr_ref;
    vm.globals.insert("gc_hdrs".to_string(), Value::GcMap(gc_map_ref));

    let prog = shae::parse_source(
        r#"
use { hpack_encode, hpack_decode } from "std:web:http"
let enc = hpack_encode(gc_hdrs)
let dec = hpack_decode(enc)
dec["x-gc"]
"#,
    )
    .unwrap();
    let chunk = shae::compiler::Compiler::new().compile_program(&prog).unwrap();
    let res = vm.interpret(chunk);
    assert_eq!(res, shae::vm::InterpretResult::Ok(Value::String("gc-body-ok".into())));
}
