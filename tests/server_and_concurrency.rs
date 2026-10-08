use shae::run;
use shae::value::Value;
use std::thread;
use std::time::Duration;

#[test]
fn test_spawn_and_join() {
    let script = r#"
        fn worker() {
            let total = 0
            let i = 0
            while i < 100 {
                i += 1
                total += i
            }
            total
        }
        let handle = spawn(worker)
        let result = handle.join()
        result
    "#;
    let res = run(script).expect("Execution should succeed");
    assert_eq!(res, Value::Number(5050.0));
}

#[test]
fn test_spawn_join_builtin() {
    let script = r#"
        fn compute() {
            "task_completed"
        }
        let t = spawn(compute)
        join(t)
    "#;
    let res = run(script).expect("Execution should succeed");
    assert_eq!(res, Value::String("task_completed".into()));
}

#[test]
fn test_spawn_task_error_propagation() {
    let script = r#"
        fn bad_task() {
            42 / 0
        }
        let t = spawn(bad_task)
        t.join()
    "#;
    let err = run(script).unwrap_err();
    assert!(err.to_string().contains("Dividing by zero creates black holes"));
}

#[test]
fn test_http_server_concurrent_and_status_codes() {
    // Start HTTP server in a background thread
    let server_script = r#"
        fn handle(req) {
            if req.path == "/json" {
                return { "name": "shae", "version": "0.2.0" }
            }
            if req.path == "/custom-404" {
                return { "status": 404, "body": "Page not found in Shae" }
            }
            if req.path == "/created" {
                return { "status": 201, "headers": { "X-Language": "Shae" }, "body": { "id": 99 } }
            }
            return "Hello from Shae!"
        }
        serve(18989, handle)
    "#;

    thread::spawn(move || {
        let _ = run(server_script);
    });

    // Give server a moment to bind
    thread::sleep(Duration::from_millis(150));

    // 1. Test 200 plain text
    let resp = reqwest::blocking::get("http://127.0.0.1:18989/").expect("Request failed");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().unwrap(), "Hello from Shae!");

    // 2. Test 200 JSON
    let resp = reqwest::blocking::get("http://127.0.0.1:18989/json").expect("Request failed");
    assert_eq!(resp.status().as_u16(), 200);
    let text = resp.text().unwrap();
    assert!(text.contains("\"name\":\"shae\""));

    // 3. Test real 404 status code!
    let resp = reqwest::blocking::get("http://127.0.0.1:18989/custom-404").expect("Request failed");
    assert_eq!(resp.status().as_u16(), 404);
    assert_eq!(resp.text().unwrap(), "Page not found in Shae");

    // 4. Test 201 Created with custom headers and JSON body
    let resp = reqwest::blocking::get("http://127.0.0.1:18989/created").expect("Request failed");
    assert_eq!(resp.status().as_u16(), 201);
    assert_eq!(resp.headers().get("X-Language").unwrap(), "Shae");
    let text = resp.text().unwrap();
    assert!(text.contains("\"id\":99"));
}

#[test]
fn test_route_match_builtin() {
    let script = r#"
let m1 = route_match("/api/:version/users/:id", "/api/v2/users/999")
let m2 = route_match("/posts/:slug", "/posts/hello-world")
let m3 = route_match("/api/:version", "/web/test")

[m1.version, m1.id, m2.slug, m3]
"#;
    let res = run(script).expect("route_match test failed");
    assert_eq!(
        res.to_json(),
        serde_json::json!(["v2", "999", "hello-world", null])
    );
}

#[test]
fn test_http_server_router_and_keep_alive() {
    let port = 19191;
    let server_script = format!(
        r#"
let app = {{
    "GET /": fn(req) {{ "Home" }},
    "GET /users/:userId/posts/:postId": fn(req) {{
        {{ "user": req.params.userId, "post": req.params.postId }}
    }},
    "GET /search": fn(req) {{
        {{ "query": req.queryParams.q, "page": req.queryParams.page }}
    }},
    "POST /echo": fn(req) {{
        {{ "status": 201, "body": req.body }}
    }}
}}
serve({}, app)
"#,
        port
    );

    thread::spawn(move || {
        let _ = run(&server_script);
    });

    thread::sleep(Duration::from_millis(150));

    // Test with persistent client (Keep-Alive)
    let client = reqwest::blocking::Client::builder()
        .pool_idle_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // 1. Root route
    let r1 = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .send()
        .expect("Root request failed");
    assert_eq!(r1.status().as_u16(), 200);
    assert_eq!(r1.text().unwrap(), "Home");

    // 2. Route with path parameters: /users/:userId/posts/:postId
    let r2 = client
        .get(&format!("http://127.0.0.1:{}/users/42/posts/999", port))
        .send()
        .expect("Params request failed");
    assert_eq!(r2.status().as_u16(), 200);
    let json2: serde_json::Value = serde_json::from_str(&r2.text().unwrap()).unwrap();
    assert_eq!(json2["user"], "42");
    assert_eq!(json2["post"], "999");

    // 3. Query params: /search?q=shae+programming&page=3
    let r3 = client
        .get(&format!("http://127.0.0.1:{}/search?q=shae+programming&page=3", port))
        .send()
        .expect("Query request failed");
    assert_eq!(r3.status().as_u16(), 200);
    let json3: serde_json::Value = serde_json::from_str(&r3.text().unwrap()).unwrap();
    assert_eq!(json3["query"], "shae programming");
    assert_eq!(json3["page"], "3");

    // 4. POST echo
    let r4 = client
        .post(&format!("http://127.0.0.1:{}/echo", port))
        .body("payload-content")
        .send()
        .expect("Post request failed");
    assert_eq!(r4.status().as_u16(), 201);
    assert_eq!(r4.text().unwrap(), "payload-content");

    // 5. 404 Route Not Found
    let r5 = client
        .get(&format!("http://127.0.0.1:{}/unknown/path", port))
        .send()
        .expect("404 request failed");
    assert_eq!(r5.status().as_u16(), 404);
}

#[test]
fn test_https_server_tls() {
    let port = 19443;
    let cert_file = "tests/test_cert.pem";
    let key_file = "tests/test_key.pem";

    // Generate test certificate using rcgen
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    std::fs::write(cert_file, cert_pem).unwrap();
    std::fs::write(key_file, key_pem).unwrap();

    let server_script = format!(
        r#"
fn handle(req) {{
    return "Secure Hello from Shae HTTPS!"
}}
serve_tls({}, handle, "{}", "{}")
"#,
        port, cert_file, key_file
    );

    thread::spawn(move || {
        let _ = run(&server_script);
    });

    thread::sleep(Duration::from_millis(200));

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let resp = client
        .get(&format!("https://127.0.0.1:{}/", port))
        .send()
        .expect("HTTPS request failed");

    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().unwrap(), "Secure Hello from Shae HTTPS!");

    let _ = std::fs::remove_file(cert_file);
    let _ = std::fs::remove_file(key_file);
}
