//! Comprehensive Integration Test Suite for Shae Studio Web IDE & Visual Debugger (Milestone 4).

use serde_json::Value as JsonValue;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn send_http_request(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .expect("Failed to connect to IDE test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("Set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("Set write timeout");

    let body_str = body.unwrap_or("");
    let req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        method,
        path,
        port,
        body_str.len(),
        body_str
    );

    stream
        .write_all(req.as_bytes())
        .expect("Write HTTP request");
    stream.flush().expect("Flush HTTP request");

    let mut resp_bytes = Vec::new();
    let _ = stream.read_to_end(&mut resp_bytes);
    let resp_str = String::from_utf8_lossy(&resp_bytes).to_string();

    let status_code = if let Some(first_line) = resp_str.lines().next() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            parts[1].parse::<u16>().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    let body_part = if let Some(idx) = resp_str.find("\r\n\r\n") {
        resp_str[idx + 4..].to_string()
    } else {
        String::new()
    };

    (status_code, body_part)
}

fn start_test_ide_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind IDE test listener");
    let port = listener.local_addr().expect("Local addr").port();
    let root = PathBuf::from(".");

    thread::spawn(move || {
        // Run server for handling test requests
        let repl_evaluator =
            std::sync::Arc::new(std::sync::Mutex::new(shae::eval::Evaluator::new()));
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                let r = root.clone();
                let rep = repl_evaluator.clone();
                thread::spawn(move || {
                    let _ = shae::ide::handle_ide_client_test(s, &r, rep);
                });
            }
        }
    });

    thread::sleep(Duration::from_millis(50));
    port
}

#[test]
fn test_ide_index_html_serves_debugger_and_canvas() {
    let port = start_test_ide_server();
    let (status, body) = send_http_request(port, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(
        body.contains("SHAE STUDIO"),
        "HTML should contain brand header"
    );
    assert!(
        body.contains("Visual Debugger"),
        "HTML should contain visual debugger tab"
    );
    assert!(
        body.contains("std:gui Live Preview"),
        "HTML should contain GUI live preview"
    );
    assert!(
        body.contains("diagnostics-badge"),
        "HTML should contain LSP diagnostics badge"
    );
    assert!(
        body.contains("btn-dbg-start"),
        "HTML should contain debug button"
    );
}

#[test]
fn test_ide_api_check_structured_diagnostics() {
    let port = start_test_ide_server();

    // 1. Valid code
    let valid_payload = serde_json::json!({
        "code": "let x = 10\nlet y = 20\nlet z = x + y"
    })
    .to_string();
    let (status, body) = send_http_request(port, "POST", "/api/check", Some(&valid_payload));
    assert_eq!(status, 200);
    let json: JsonValue = serde_json::from_str(&body).expect("Valid JSON response");
    assert_eq!(json["valid"], true);
    assert_eq!(json["errors"].as_array().unwrap().len(), 0);

    // 2. Syntax error code
    let invalid_payload = serde_json::json!({
        "code": "fn unclosed_function( { let x = 10"
    })
    .to_string();
    let (status2, body2) = send_http_request(port, "POST", "/api/check", Some(&invalid_payload));
    assert_eq!(status2, 200);
    let json2: JsonValue = serde_json::from_str(&body2).expect("Valid JSON response");
    assert_eq!(json2["valid"], false);
    assert!(!json2["errors"].as_array().unwrap().is_empty());
    assert!(!json2["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn test_ide_api_gui_frame_endpoint() {
    let port = start_test_ide_server();
    let (status, body) = send_http_request(port, "GET", "/api/gui/frame", None);
    assert_eq!(status, 200);
    let json: JsonValue = serde_json::from_str(&body).expect("Valid JSON response");
    assert!(json.get("width").is_some());
    assert!(json.get("height").is_some());
    assert!(json.get("buffer").is_some());
}

#[test]
fn test_ide_visual_debugger_start_step_continue_stop() {
    let port = start_test_ide_server();

    // 1. Start Debug Session with breakpoint at line 2
    let script = "let a = 10\nlet b = 20\nlet c = a + b\nc";
    let start_payload = serde_json::json!({
        "code": script,
        "breakpoints": [2]
    })
    .to_string();

    let (status1, body1) =
        send_http_request(port, "POST", "/api/debug/start", Some(&start_payload));
    assert_eq!(status1, 200);
    let state1: JsonValue = serde_json::from_str(&body1).expect("Parse debug start");
    assert_eq!(state1["active"], true);
    assert_eq!(state1["stopped"], true);
    assert_eq!(state1["current_line"], 2);

    // 2. Step Over to line 3
    let step_payload = serde_json::json!({ "mode": "over" }).to_string();
    let (status2, body2) = send_http_request(port, "POST", "/api/debug/step", Some(&step_payload));
    assert_eq!(status2, 200);
    let state2: JsonValue = serde_json::from_str(&body2).expect("Parse debug step");
    assert_eq!(state2["active"], true);
    assert_eq!(state2["stopped"], true);
    assert_eq!(state2["current_line"], 3);

    // Inspect local variables captured at step
    let vars = state2["variables"].as_object().expect("Variables object");
    assert!(vars.contains_key("var_0") || vars.contains_key("a") || !vars.is_empty());

    // 3. Continue execution until termination
    let (status3, body3) = send_http_request(port, "POST", "/api/debug/continue", None);
    assert_eq!(status3, 200);
    let state3: JsonValue = serde_json::from_str(&body3).expect("Parse debug continue");
    assert_eq!(state3["is_terminated"], true);

    // 4. Stop Debugger cleanly
    let (status4, body4) = send_http_request(port, "POST", "/api/debug/stop", None);
    assert_eq!(status4, 200);
    assert!(body4.contains("stopped"));
}

#[test]
fn test_ide_api_format_and_run() {
    let port = start_test_ide_server();

    // Format
    let fmt_payload = serde_json::json!({
        "code": "fn test(a,b){return a+b;}"
    })
    .to_string();
    let (status_fmt, body_fmt) = send_http_request(port, "POST", "/api/fmt", Some(&fmt_payload));
    assert_eq!(status_fmt, 200);
    let json_fmt: JsonValue = serde_json::from_str(&body_fmt).unwrap();
    assert!(json_fmt["formatted"]
        .as_str()
        .unwrap()
        .contains("fn test(a, b)"));

    // Run
    let run_payload = serde_json::json!({
        "code": "let msg = \"Studio Runner Output\"\nprint(msg)\n100 + 42"
    })
    .to_string();
    let (status_run, body_run) = send_http_request(port, "POST", "/api/run", Some(&run_payload));
    assert_eq!(status_run, 200);
    let json_run: JsonValue = serde_json::from_str(&body_run).unwrap();
    assert_eq!(json_run["exit_code"], 0);
    assert!(json_run["stdout"]
        .as_str()
        .unwrap()
        .contains("Studio Runner Output"));
    assert_eq!(json_run["result"], "142");
}
