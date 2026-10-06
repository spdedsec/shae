use shae::run;
use serde_json::json;

#[test]
fn test_try_catch_success() {
    let script = r#"
        let result = "init"
        try {
            let temp = 10 + 20
            result = "success"
        } catch (e) {
            result = "failed"
        }
        result
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!("success"));
}

#[test]
fn test_try_catch_error() {
    let script = r#"
        let result = "init"
        let error_msg = ""
        try {
            let temp = 10 + "string" // this causes RuntimeError
            result = "success"
        } catch (e) {
            result = "caught"
            error_msg = e
        }
        [result, error_msg]
    "#;
    let val = run(script).expect("Execution failed");
    let arr = val.to_json().as_array().unwrap().clone();
    assert_eq!(arr[0], json!("caught"));
    assert!(arr[1].as_str().unwrap().contains("Cannot add"));
}
