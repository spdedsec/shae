use shae::run;
use serde_json::json;

#[test]
fn test_pipe_operator() {
    let script = r#"
        fn add(a, b) { a + b }
        fn mult(a, b) { a * b }
        
        let result = 5 |> add(10) |> mult(2)
        
        let arr = [1, 2, 3]
        let sum = arr |> push(4)
        
        [result, arr]
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([30, [1, 2, 3, 4]]));
}
