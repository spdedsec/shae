use serde_json::json;
use shae::run;

#[test]
fn test_io() {
    let script = r#"
        write("tests/temp.txt", "Hello IO")
        let content = read("tests/temp.txt")
        content
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!("Hello IO"));
    std::fs::remove_file("tests/temp.txt").unwrap();
}

#[test]
fn test_fetch() {
    // We just do a dummy test to avoid network flakiness if possible, or use a reliable endpoint.
    let script = r#"
        let body = fetch("https://example.com")
        body.len > 0
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(true));
}
