use shae::compiler::Compiler;
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};
use std::io::{Read, Write};
use std::net::TcpListener;

fn run_shae(code: &str) -> Value {
    let program = parse_source(code).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(err) => panic!("Shae runtime error: {:?}", err),
        InterpretResult::CompileError => panic!("Shae compile error"),
    }
}

#[test]
fn test_http_get_with_query_params_and_json_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req_text = String::from_utf8_lossy(&buf[..n]);

            if req_text.starts_with("GET /search?q=shae+lang&limit=5") {
                let json_body = r#"{"query":"shae lang","results":[{"id":1,"name":"Intro"},{"id":2,"name":"JIT"}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json_body.len(),
                    json_body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if req_text.starts_with("HEAD /ping") {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let resp =
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            }
            let _ = stream.flush();
        }
    });

    let script = format!(
        r#"
let http = use "std:http"

let url = "http://127.0.0.1:{}/search"
let res = http.get(url, {{
    query: {{ "q": "shae lang", "limit": 5 }}
}})

let head_res = http.head("http://127.0.0.1:{}/ping")

[res.status, res.ok, res.json["query"], len(res.json["results"]), res.json["results"][1]["name"], head_res.status]
"#,
        port, port
    );

    let res = run_shae(&script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(200));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::String("shae lang".into()));
            assert_eq!(guard[3], Value::Int(2));
            assert_eq!(guard[4], Value::String("JIT".into()));
            assert_eq!(guard[5], Value::Int(200));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_http_post_json_and_get_json_shortcuts() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req_text = String::from_utf8_lossy(&buf[..n]);

            if req_text.starts_with("POST /items") {
                assert!(
                    req_text.contains("content-type: application/json")
                        || req_text.contains("Content-Type: application/json")
                );
                let body = req_text.split("\r\n\r\n").nth(1).unwrap_or("");
                let resp = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if req_text.starts_with("GET /items/42") {
                let json_body = r#"{"id":42,"title":"Database Driver"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json_body.len(),
                    json_body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let resp =
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            }
            let _ = stream.flush();
        }
    });

    let script = format!(
        r#"
let http = use "std:http"

let post_data = http.post_json("http://127.0.0.1:{}/items", {{
    name: "Cranelift Optimizer",
    tier: 1
}})

let get_data = http.get_json("http://127.0.0.1:{}/items/42")

[post_data["name"], post_data["tier"], get_data["id"], get_data["title"]]
"#,
        port, port
    );

    let res = run_shae(&script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::String("Cranelift Optimizer".into()));
            assert_eq!(guard[1], Value::Int(1));
            assert_eq!(guard[2], Value::Int(42));
            assert_eq!(guard[3], Value::String("Database Driver".into()));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}
