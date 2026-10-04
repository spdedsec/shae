use shae::run;
use serde_json::json;
use std::fs;

#[test]
fn test_module_system() {
    let script = r#"
        let math = use "tests/test_math.shae"
        math.add(10, 20)
    "#;
    
    // Create test module file
    fs::write("tests/test_math.shae", "
fn add(a, b) {
    return a + b
}
let PI = 3.1415
").unwrap();

    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(30));
    
    // cleanup
    fs::remove_file("tests/test_math.shae").unwrap();
}
