use shae::run;
use shae::value::Value;

#[test]
fn test_array_destructuring() {
    let script = r#"
        let [a, b, c] = [10, 20, 30]
        [a, b, c]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(10));
            assert_eq!(b[1], Value::Int(20));
            assert_eq!(b[2], Value::Int(30));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_array_destructuring_with_rest() {
    let script = r#"
        let numbers = [1, 2, 3, 4, 5]
        let [head, second, ..tail] = numbers
        [head, second, tail]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(1));
            assert_eq!(b[1], Value::Int(2));
            match &b[2] {
                Value::Array(tail_arr) => {
                    let tb = tail_arr.read().unwrap();
                    assert_eq!(tb.len(), 3);
                    assert_eq!(tb[0], Value::Int(3));
                    assert_eq!(tb[1], Value::Int(4));
                    assert_eq!(tb[2], Value::Int(5));
                }
                _ => panic!("Expected tail to be array"),
            }
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_map_destructuring() {
    let script = r#"
        let user = {
            name: "Alice",
            role: "Admin",
            active: true
        }
        let { name, role, active } = user
        [name, role, active]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("Alice".into()));
            assert_eq!(b[1], Value::String("Admin".into()));
            assert_eq!(b[2], Value::Bool(true));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_struct_destructuring() {
    let script = r#"
        struct Config { host, port, debug }
        let cfg = Config { host: "localhost", port: 8080, debug: false }
        let { host, port, debug } = cfg
        [host, port, debug]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("localhost".into()));
            assert_eq!(b[1], Value::Int(8080));
            assert_eq!(b[2], Value::Bool(false));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_object_destructuring_rename_and_rest() {
    let script = r#"
        let data = {
            id: 101,
            title: "Shae Language",
            author: "Antigravity",
            stars: 9000
        }
        let { id: doc_id, ..details } = data
        [doc_id, details["title"], details["stars"]]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(101));
            assert_eq!(b[1], Value::String("Shae Language".into()));
            assert_eq!(b[2], Value::Int(9000));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_nested_destructuring() {
    let script = r#"
        let payload = [
            "HTTP/2",
            { status: 200, message: "OK" }
        ]
        let [proto, { status, message }] = payload
        [proto, status, message]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("HTTP/2".into()));
            assert_eq!(b[1], Value::Int(200));
            assert_eq!(b[2], Value::String("OK".into()));
        }
        _ => panic!("Expected array"),
    }
}

