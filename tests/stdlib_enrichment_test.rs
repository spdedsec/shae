use shae::ast::Program;
use shae::compiler::Compiler;
use shae::lexer::Lexer;
use shae::parser::Parser;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

fn run_shae(code: &str) -> Value {
    let mut lexer = Lexer::new(code);
    let tokens = lexer.tokenize().expect("Lexing failed");
    let mut parser = Parser::new(tokens);
    let program: Program = parser.parse().expect("Parsing failed");
    let chunk = Compiler::new()
        .compile_program(&program)
        .expect("Compilation failed");
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(e) => panic!("Runtime error: {}", e),
        InterpretResult::CompileError => panic!("Compile error in VM"),
    }
}

#[test]
fn test_std_env_module() {
    let script = r##"
use { get, set, has, remove, all, parse_dotenv } from "std:env"

// 1. Defaults and basic operations
let fallback = get("SHAE_NON_EXISTENT_VAR_123", "default_val")
assert_eq(fallback, "default_val")

let builtin_fallback = env("SHAE_NON_EXISTENT_VAR_123", "builtin_default")
assert_eq(builtin_fallback, "builtin_default")

set("SHAE_TEST_ENV_VAR", "super_secret_value")
assert(has("SHAE_TEST_ENV_VAR"))
assert_eq(get("SHAE_TEST_ENV_VAR"), "super_secret_value")

remove("SHAE_TEST_ENV_VAR")
assert_eq(has("SHAE_TEST_ENV_VAR"), false)

// 2. Dotenv parsing
let dotenv_text = "# This is a comment\nPORT=8080\nexport APP_NAME=\"Shae Platform\"\nDATABASE_URL='postgres://localhost:5432/db'\nEMPTY="
let parsed = parse_dotenv(dotenv_text)
assert_eq(parsed.PORT, "8080")
assert_eq(parsed.APP_NAME, "Shae Platform")
assert_eq(parsed.DATABASE_URL, "postgres://localhost:5432/db")

"env_ok"
"##;
    let res = run_shae(script);
    assert_eq!(res, Value::String("env_ok".into()));
}

#[test]
fn test_std_cli_module() {
    let script = r#"
use { parse } from "std:cli"

let raw_args = ["--port", "8080", "--verbose", "-d", "--host=localhost", "run", "server.shae"]
let parsed = parse(raw_args)

assert_eq(parsed.options.port, "8080")
assert_eq(parsed.options.host, "localhost")
assert(parsed.flags.verbose)
assert(parsed.flags.d)
assert_eq(parsed.positional, ["run", "server.shae"])
assert_eq(parsed.subcommand, "run")

"cli_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("cli_ok".into()));
}

#[test]
fn test_std_collections_module() {
    let script = r#"
use { unique, union, intersect, difference, chunk, zip } from "std:collections"

// 1. Unique / Set
let u = unique([1, 2, 2, 3, 1, 4, 3, 5])
assert_eq(u, [1, 2, 3, 4, 5])

// 2. Set operations
let a = [1, 2, 3]
let b = [2, 3, 4]
assert_eq(union(a, b), [1, 2, 3, 4])
assert_eq(intersect(a, b), [2, 3])
assert_eq(difference(a, b), [1])

// 3. Chunk
let chunks = chunk([1, 2, 3, 4, 5], 2)
assert_eq(chunks, [[1, 2], [3, 4], [5]])

// 4. Zip
let zipped = zip(["a", "b", "c"], [10, 20, 30])
assert_eq(zipped, [["a", 10], ["b", 20], ["c", 30]])

"collections_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("collections_ok".into()));
}

#[test]
fn test_std_http_client() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req_text = String::from_utf8_lossy(&buf[..n]);

            if req_text.starts_with("POST /echo") {
                let body = req_text.split("\r\n\r\n").nth(1).unwrap_or("");
                let resp = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 10\r\nConnection: close\r\n\r\nHello Shae";
                let _ = stream.write_all(resp.as_bytes());
            }
            let _ = stream.flush();
        }
    });

    let script = format!(
        r#"
use {{ get, post, request }} from "std:http"

let url = "http://127.0.0.1:{}/hello"
let resp = get(url)
assert_eq(resp.status, 200)
assert(resp.ok)
assert_eq(resp.body, "Hello Shae")

let post_url = "http://127.0.0.1:{}/echo"
let post_resp = post(post_url, "custom_payload")
assert_eq(post_resp.status, 201)
assert(post_resp.ok)
assert_eq(post_resp.body, "custom_payload")

// Test fetch with options map
let fetch_resp = fetch(post_url, {{ method: "POST", body: "fetch_payload" }})
assert_eq(fetch_resp.status, 201)
assert_eq(fetch_resp.body, "fetch_payload")

"http_ok"
"#,
        port, port
    );

    let res = run_shae(&script);
    assert_eq!(res, Value::String("http_ok".into()));
}
