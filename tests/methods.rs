use serde_json::json;
use shae::run;

#[test]
fn test_array_methods() {
    let script = r#"
        let arr = [1, 2, 3]
        arr.push(4)
        
        let sum = arr.sum()
        let doubled = arr.map(fn(x) { return x * 2 })
        let evens = arr.filter(fn(x) { return x % 2 == 0 })
        
        let reduced = arr.reduce(fn(acc, cur) { return acc + cur }, 10)
        
        [sum, doubled, evens, reduced]
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([10, [2, 4, 6, 8], [2, 4], 20]));
}

#[test]
fn test_string_methods() {
    let script = r#"
        let s = "  Hello World  "
        let t = s.trim()
        let u = t.upper()
        let l = t.lower()
        let parts = t.split(" ")
        let rep = t.replace("World", "Shae")
        
        [t, u, l, parts, rep]
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(
        val.to_json(),
        json!([
            "Hello World",
            "HELLO WORLD",
            "hello world",
            ["Hello", "World"],
            "Hello Shae"
        ])
    );
}

#[test]
fn test_array_map_filter_no_deadlock_on_mutation() {
    let script = r#"
        let arr = [1, 2, 3]
        // Mutate array inside map callback - previously deadlocked
        let mapped = arr.map(fn(x) {
            arr.push(99)
            return x * 10
        })
        mapped
    "#;
    let val = run(script).expect("Should not deadlock when mutating inside map callback");
    assert_eq!(val.to_json(), json!([10, 20, 30]));
}
