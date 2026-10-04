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
            let b = arr.borrow();
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
