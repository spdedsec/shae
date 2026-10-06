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
