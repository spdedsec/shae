use serde_json::json;
use shae::eval::Evaluator;
use shae::lexer::tokenize;
use shae::parser::parse;
use shae::value::Value;

fn run_source(src: &str) -> Result<Value, String> {
    let tokens = tokenize(src).map_err(|e| format!("{:?}", e))?;
    let program = parse(tokens).map_err(|e| format!("{:?}", e))?;
    let mut eval = Evaluator::new();
    eval.eval_program(&program).map_err(|e| format!("{:?}", e))
}

#[test]
fn test_readme_quickstart_and_interpolation() {
    // 1. Test ${name} syntax from README
    let script1 = r#"
let name = "Developer"
let greeting = "Hello, ${name}! Welcome to Shae."
greeting
"#;
    let res1 = run_source(script1).expect("script1 failed");
    assert_eq!(res1.to_json(), json!("Hello, Developer! Welcome to Shae."));

    // 2. Test {name} syntax
    let script2 = r#"
let name = "Developer"
let greeting = "Hello, {name}! Welcome to Shae."
greeting
"#;
    let res2 = run_source(script2).expect("script2 failed");
    assert_eq!(res2.to_json(), json!("Hello, Developer! Welcome to Shae."));

    // 3. Test println builtin
    let script3 = r#"
let name = "Shae"
println("Running with", name)
"#;
    let res3 = run_source(script3).expect("println test failed");
    assert_eq!(res3, Value::Null);
}

#[test]
fn test_serve_tls_map_signature_eval() {
    // Test that serve_tls accepts config map { port, cert, key }
    // We expect it to validate paths (it will fail to find cert, but validates argument signature)
    let script = r#"
let cfg = { port: 8443, cert: "missing_cert.pem", key: "missing_key.pem" }
try {
    serve_tls(cfg, fn(req) { "ok" })
} catch (e) {
    str(e)
}
"#;
    let res = run_source(script).expect("serve_tls map signature failed");
    if let Value::String(err_msg) = res {
        // Must fail with certificate error or IO error, NOT "expects 4 arguments"
        assert!(!err_msg.contains("expects 4 arguments"));
        assert!(err_msg.contains("Failed to open certificate") || err_msg.contains("No such file"));
    } else {
        panic!("Expected string error result from serve_tls missing certs");
    }
}
