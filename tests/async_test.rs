use shae::value::Value;

#[test]
fn test_async_fn_and_await_ast() {
    let script = r#"
async fn compute(x) {
    x * 2 + 10
}

let result = await compute(16)
result
"#;
    let val = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val, Value::Int(42));
}

#[test]
fn test_async_fn_and_await_vm() {
    let script = r#"
async fn compute(x) {
    x * 2 + 10
}

let result = await compute(16)
result
"#;
    let val = shae::run_vm(script).expect("VM evaluation should succeed");
    assert_eq!(val, Value::Int(42));
}

#[test]
fn test_async_returns_task_and_explicit_join() {
    let script = r#"
async fn work(val) {
    val + 100
}

let t = work(50)
let res = t.join()
res
"#;
    let val = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val, Value::Int(150));
}

#[test]
fn test_await_non_async_value() {
    let script = r#"
let x = await 42
let y = await "hello"
x
"#;
    let val_ast = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val_ast, Value::Int(42));

    let val_vm = shae::run_vm(script).expect("VM evaluation should succeed");
    assert_eq!(val_vm, Value::Int(42));
}

#[test]
fn test_async_lambda_and_await() {
    let script = r#"
let f = async fn(a, b) {
    a + b
}

let res = await f(20, 22)
res
"#;
    let val_ast = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val_ast, Value::Int(42));

    let val_vm = shae::run_vm(script).expect("VM evaluation should succeed");
    assert_eq!(val_vm, Value::Int(42));
}

#[test]
fn test_concurrent_async_tasks() {
    let script = r#"
async fn step1() {
    10
}

async fn step2() {
    20
}

async fn step3() {
    30
}

let t1 = step1()
let t2 = step2()
let t3 = step3()

let total = (await t1) + (await t2) + (await t3)
total
"#;
    let val_ast = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val_ast, Value::Int(60));

    let val_vm = shae::run_vm(script).expect("VM evaluation should succeed");
    assert_eq!(val_vm, Value::Int(60));
}

#[test]
fn test_async_error_propagation_and_try_catch() {
    let script = r#"
async fn fail() {
    10 + "string"
}

let caught = "none"
try {
    await fail()
} catch err {
    caught = err
}
caught
"#;
    let val_ast = shae::run(script).expect("AST evaluation should succeed");
    match val_ast {
        Value::String(s) => assert!(!s.is_empty() && s != "none"),
        other => panic!("Expected string error message, got {:?}", other),
    }

    let val_vm = shae::run_vm(script).expect("VM evaluation should succeed");
    match val_vm {
        Value::String(s) => assert!(!s.is_empty() && s != "none"),
        other => panic!("Expected string error message, got {:?}", other),
    }
}

#[test]
fn test_async_nested_calls() {
    let script = r#"
async fn inner(x) {
    x * 2
}

async fn outer(y) {
    let v = await inner(y)
    v + 5
}

let res = await outer(10)
res
"#;
    let val_ast = shae::run(script).expect("AST evaluation should succeed");
    assert_eq!(val_ast, Value::Int(25));

    let val_vm = shae::run_vm(script).expect("VM evaluation should succeed");
    assert_eq!(val_vm, Value::Int(25));
}
