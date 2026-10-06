use shae::run;
use shae::value::Value;

#[test]
fn test_closures_and_higher_order_functions() {
    let script = r#"
        fn make_adder(x) {
            return fn(y) {
                return x + y
            }
        }
        let add5 = make_adder(5)
        add5(10)
    "#;
    let res = run(script).expect("Execution should succeed");
    assert_eq!(res, Value::Number(15.0));
}

#[test]
fn test_array_mutation_and_methods() {
    let script = r#"
        let numbers = [1, 2, 3]
        push(numbers, 4)
        push(numbers, 5)
        let last = pop(numbers)
        let length = len(numbers)
        [length, last, numbers[-1]]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Number(4.0)); // length: 4
            assert_eq!(b[1], Value::Number(5.0)); // popped: 5
            assert_eq!(b[2], Value::Number(4.0)); // negative index: 4
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_loop_break_and_continue() {
    let script = r#"
        let total = 0
        let i = 0
        while i < 10 {
            i += 1
            if i % 2 == 0 {
                continue
            }
            if i > 7 {
                break
            }
            total += i
        }
        total
    "#;
    // Odd numbers under 7: 1, 3, 5, 7 -> sum = 16
    let res = run(script).expect("Execution should succeed");
    assert_eq!(res, Value::Number(16.0));
}

#[test]
fn test_safe_navigation_and_null_coalesce() {
    let script = r#"
        let user = { profile: { name: "Satya" } }
        let bio = user.profile.bio ?? "No bio provided"
        let missing = user?.settings?.theme ?? "dark"
        bio + " | " + missing
    "#;
    let res = run(script).expect("Execution should succeed");
    assert_eq!(res, Value::String("No bio provided | dark".into()));
}

#[test]
fn test_division_by_zero_safety() {
    let script = "let boom = 42 / 0";
    let err = run(script).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Dividing by zero creates black holes"));
}

#[test]
fn test_string_interpolation_and_escaped_braces() {
    let script = r#"
        let name = "Shae"
        let count = 3
        let interpolated = "Hello {name}, you have {count} messages!"
        let escaped = "Literal \{brace\} with {name}"
        [interpolated, escaped]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("Hello Shae, you have 3 messages!".into()));
            assert_eq!(b[1], Value::String("Literal {brace} with Shae".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_truthiness_semantics() {
    let script = r#"
        let results = []
        if 0 { push(results, "bad") } else { push(results, "zero_falsy") }
        if "" { push(results, "bad") } else { push(results, "empty_str_falsy") }
        if [] { push(results, "bad") } else { push(results, "empty_arr_falsy") }
        if {} { push(results, "bad") } else { push(results, "empty_map_falsy") }
        if null { push(results, "bad") } else { push(results, "null_falsy") }
        if 42 { push(results, "num_truthy") }
        if "hello" { push(results, "str_truthy") }
        if [1] { push(results, "arr_truthy") }
        results
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b.len(), 8);
            assert_eq!(b[0], Value::String("zero_falsy".into()));
            assert_eq!(b[1], Value::String("empty_str_falsy".into()));
            assert_eq!(b[2], Value::String("empty_arr_falsy".into()));
            assert_eq!(b[3], Value::String("empty_map_falsy".into()));
            assert_eq!(b[4], Value::String("null_falsy".into()));
            assert_eq!(b[5], Value::String("num_truthy".into()));
            assert_eq!(b[6], Value::String("str_truthy".into()));
            assert_eq!(b[7], Value::String("arr_truthy".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_array_float_index_rejection() {
    let script = r#"
        let arr = [10, 20, 30]
        arr[1.5]
    "#;
    let err = run(script).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Array index must be a whole number"));
}

#[test]
fn test_property_access_semantics() {
    // 1. Strict access errors on missing property
    let strict_script = r#"
        let user = { name: "Alice" }
        user.age
    "#;
    let err = run(strict_script).unwrap_err();
    assert!(err.to_string().contains("Property 'age' not found in map"));

    // 2. Safe navigation returns null
    let safe_script = r#"
        let user = { name: "Alice" }
        user?.age
    "#;
    let res = run(safe_script).expect("Safe navigation should succeed");
    assert_eq!(res, Value::Null);

    // 3. Coalescing falls back leniently
    let coalesce_script = r#"
        let user = { name: "Alice" }
        user.age ?? 25
    "#;
    let res = run(coalesce_script).expect("Coalescing should succeed");
    assert_eq!(res, Value::Number(25.0));
}

