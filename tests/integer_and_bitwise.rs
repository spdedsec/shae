use shae::run;
use shae::value::Value;

#[test]
fn test_integer_literals() {
    let script = r#"
        let dec = 1_000_000
        let hex = 0xFF
        let bin = 0b1010
        let oct = 0o77
        [dec, hex, bin, oct]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(1_000_000));
            assert_eq!(b[1], Value::Int(255));
            assert_eq!(b[2], Value::Int(10));
            assert_eq!(b[3], Value::Int(63));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_bitwise_operators() {
    let script = r#"
        let and_op = 0b1100 & 0b1010
        let or_op = 0b1100 | 0b1010
        let xor_op = 0b1100 ^ 0b1010
        let not_op = ~0
        let shl_op = 1 << 4
        let shr_op = 16 >> 2
        [and_op, or_op, xor_op, not_op, shl_op, shr_op]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(0b1000));
            assert_eq!(b[1], Value::Int(0b1110));
            assert_eq!(b[2], Value::Int(0b0110));
            assert_eq!(b[3], Value::Int(-1));
            assert_eq!(b[4], Value::Int(16));
            assert_eq!(b[5], Value::Int(4));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_bitwise_precedence() {
    let script = r#"
        // Addition (+) binds tighter than shift (<<): 1 << 2 + 1 => 1 << 3 => 8
        let r1 = 1 << 2 + 1
        // Shift (<<) binds tighter than bitwise AND (&): 1 << 2 & 7 => 4 & 7 => 4
        let r2 = 1 << 2 & 7
        // Bitwise AND (&) binds tighter than bitwise XOR (^): 5 ^ 3 & 1 => 5 ^ 1 => 4
        let r3 = 5 ^ 3 & 1
        // Bitwise XOR (^) binds tighter than bitwise OR (|): 1 | 2 ^ 3 => 1 | 1 => 1
        let r4 = 1 | 2 ^ 3
        // Unary NOT (~) binds tighter than multiplication (*): ~1 * 3 => -2 * 3 => -6
        let r5 = ~1 * 3
        [r1, r2, r3, r4, r5]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(8));
            assert_eq!(b[1], Value::Int(4));
            assert_eq!(b[2], Value::Int(4));
            assert_eq!(b[3], Value::Int(1));
            assert_eq!(b[4], Value::Int(-6));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_mixed_arithmetic_and_division() {
    let script = r#"
        let i_add = 10 + 5
        let f_add = 10 + 2.5
        let i_div = 10 / 2
        let f_div = 10 / 4
        [i_add, f_add, i_div, f_div]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(15));
            assert_eq!(b[1], Value::Float(12.5));
            assert_eq!(b[2], Value::Int(5));
            assert_eq!(b[3], Value::Float(2.5));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_array_indexing_with_integers() {
    let script = r#"
        let items = [10, 20, 30, 40]
        let second = items[1]
        let last = items[-1]
        items[2] = 99
        [second, last, items[2]]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(20));
            assert_eq!(b[1], Value::Int(40));
            assert_eq!(b[2], Value::Int(99));
        }
        _ => panic!("Expected array"),
    }
}
