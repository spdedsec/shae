use shae::ast::{BinaryOp, Expr, Literal, Span};
use shae::compiler::Compiler;
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};
use std::fs;

#[test]
fn test_compiler_basic_math() {
    let dummy_span = Span { line: 1, col: 1 };

    // (5 + 10) * -2
    let add_expr = Expr::Binary {
        left: Box::new(Expr::Literal(Literal::Number(5.0))),
        op: BinaryOp::Add,
        right: Box::new(Expr::Literal(Literal::Number(10.0))),
        span: dummy_span,
    };

    let neg_expr = Expr::Unary {
        op: shae::ast::UnaryOp::Neg,
        expr: Box::new(Expr::Literal(Literal::Number(2.0))),
        span: dummy_span,
    };

    let final_expr = Expr::Binary {
        left: Box::new(add_expr),
        op: BinaryOp::Mul,
        right: Box::new(neg_expr),
        span: dummy_span,
    };

    let compiler = Compiler::new();
    let chunk = compiler.compile(&final_expr).expect("Failed to compile");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    assert_eq!(result, InterpretResult::Ok(Value::Number(-30.0)));
}

#[test]
fn test_compiler_locals_and_arithmetic() {
    let script = r#"
let a = 15
let b = 25
let c = a + b * 2
c
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation should succeed");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    assert_eq!(result, InterpretResult::Ok(Value::Int(65)));
}

#[test]
fn test_compiler_local_assignment() {
    let script = r#"
let x = 10
x = 50
x + 5
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation should succeed");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    assert_eq!(result, InterpretResult::Ok(Value::Int(55)));
}

#[test]
fn test_compiler_bitwise_and_comparisons() {
    let script = r#"
let x = 0b1100 & 0b1010
let is_greater = x > 5
let bit_shifted = (x << 2) | 1
bit_shifted
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation should succeed");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    // 0b1000 = 8. (8 << 2) | 1 = 32 | 1 = 33
    assert_eq!(result, InterpretResult::Ok(Value::Int(33)));
}

#[test]
fn test_compiler_if_else() {
    let script = r#"
let score = 85
let grade = "F"
if score >= 90 {
    grade = "A"
} else if score >= 80 {
    grade = "B"
} else {
    grade = "C"
}
grade
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation should succeed");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    assert_eq!(result, InterpretResult::Ok(Value::String("B".into())));
}

#[test]
fn test_compiler_while_loop_with_break_and_continue() {
    let script = r#"
let sum = 0
let i = 0
while i < 10 {
    i = i + 1
    if i == 5 {
        continue
    }
    if i > 8 {
        break
    }
    sum = sum + i
}
sum
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation should succeed");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    // i sequence added: 1, 2, 3, 4, (skip 5), 6, 7, 8 (breaks at 9)
    // sum = 1 + 2 + 3 + 4 + 6 + 7 + 8 = 31
    assert_eq!(result, InterpretResult::Ok(Value::Int(31)));
}

#[test]
fn test_compiler_short_circuit_logical_ops() {
    // Test expressions individually in VM
    let s1 = "false and 100";
    let p1 = parse_source(s1).unwrap();
    let r1 = VM::new().interpret(Compiler::new().compile_program(&p1).unwrap());
    assert_eq!(r1, InterpretResult::Ok(Value::Bool(false)));

    let s2 = "true or 200";
    let p2 = parse_source(s2).unwrap();
    let r2 = VM::new().interpret(Compiler::new().compile_program(&p2).unwrap());
    assert_eq!(r2, InterpretResult::Ok(Value::Bool(true)));

    let s3 = "true and 300";
    let p3 = parse_source(s3).unwrap();
    let r3 = VM::new().interpret(Compiler::new().compile_program(&p3).unwrap());
    assert_eq!(r3, InterpretResult::Ok(Value::Int(300)));

    let s4 = "false or 400";
    let p4 = parse_source(s4).unwrap();
    let r4 = VM::new().interpret(Compiler::new().compile_program(&p4).unwrap());
    assert_eq!(r4, InterpretResult::Ok(Value::Int(400)));
}

#[test]
fn test_compiler_for_in_loop() {
    let script = r#"
let sum = 0
for x in [1, 2, 3, 4, 5] {
    sum = sum + x
}
sum
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(15)));
}

#[test]
fn test_compiler_for_in_with_break_and_continue() {
    let script = r#"
let sum = 0
for x in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
    if x == 3 {
        continue
    }
    if x == 7 {
        break
    }
    sum = sum + x
}
sum
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    // x values added: 1 + 2 + (skip 3) + 4 + 5 + 6 = 18 (stops before adding 7)
    assert_eq!(result, InterpretResult::Ok(Value::Int(18)));
}

#[test]
fn test_compiler_array_indexing_and_mutation() {
    let script = r#"
let arr = [10, 20, 30]
arr[1] = 99
let last = arr[-1]
let mid = arr[1]
mid + last
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    // last = arr[-1] = 30, mid = arr[1] = 99, 99 + 30 = 129
    assert_eq!(result, InterpretResult::Ok(Value::Int(129)));
}

#[test]
fn test_compiler_nested_for_loops() {
    let script = r#"
let total = 0
for i in [1, 2, 3] {
    for j in [10, 20] {
        total = total + i * j
    }
}
total
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    // i=1: 10 + 20 = 30
    // i=2: 20 + 40 = 60
    // i=3: 30 + 60 = 90
    // total = 180
    assert_eq!(result, InterpretResult::Ok(Value::Int(180)));
}

#[test]
fn test_compiler_user_function_call() {
    let script = r#"
fn add(a, b) {
    return a + b
}
add(10, 25)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(35)));
}

#[test]
fn test_compiler_nested_function_calls() {
    let script = r#"
fn square(x) {
    return x * x
}
fn sum_of_squares(a, b) {
    return square(a) + square(b)
}
sum_of_squares(3, 4)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(25)));
}

#[test]
fn test_compiler_recursive_function() {
    let script = r#"
fn factorial(n) {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}
factorial(5)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(120)));
}

#[test]
fn test_compiler_lambda_expression() {
    let script = r#"
let double = fn(x) { return x * 2 }
double(21)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(42)));
}

#[test]
fn test_compiler_builtin_function_call() {
    let script = r#"
let arr = [10, 20, 30, 40]
len(arr)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(4)));
}

#[test]
fn test_compiler_arity_mismatch_error() {
    let script = r#"
fn greet(name) {
    return name
}
greet()
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert!(matches!(result, InterpretResult::RuntimeError(_)));
}

#[test]
fn test_compiler_closure_capture_outer_local() {
    let script = r#"
fn make_adder(x) {
    return fn(y) {
        return x + y
    }
}
let add5 = make_adder(5)
let add10 = make_adder(10)
add5(3) + add10(2)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(20)));
}

#[test]
fn test_compiler_closure_mutable_upvalue() {
    let script = r#"
fn make_counter() {
    let count = 0
    return fn() {
        count = count + 1
        return count
    }
}
let counter = make_counter()
let c1 = counter()
let c2 = counter()
let c3 = counter()
c1 * 100 + c2 * 10 + c3
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(123)));
}

#[test]
fn test_compiler_nested_closure_upvalue_chain() {
    let script = r#"
fn f(a) {
    return fn(b) {
        return fn(c) {
            return a + b + c
        }
    }
}
let f1 = f(10)
let f2 = f1(20)
f2(30)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(60)));
}

#[test]
fn test_compiler_closure_shared_mutable_state() {
    let script = r#"
fn make_box() {
    let value = 100
    let inc = fn() {
        value = value + 10
        return value
    }
    let dec = fn() {
        value = value - 5
        return value
    }
    return [inc, dec]
}
let box = make_box()
let inc_fn = box[0]
let dec_fn = box[1]
let r1 = inc_fn()
let r2 = dec_fn()
let r3 = inc_fn()
r1 + r2 + r3
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    // r1: 100 + 10 = 110
    // r2: 110 - 5 = 105
    // r3: 105 + 10 = 115
    // total: 110 + 105 + 115 = 330
    assert_eq!(result, InterpretResult::Ok(Value::Int(330)));
}

#[test]
fn test_compiler_closure_closed_over_block_scope() {
    let script = r#"
fn outer() {
    let getter = null
    if true {
        let secret = 777
        getter = fn() {
            return secret
        }
    }
    return getter()
}
outer()
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(777)));
}

#[test]
fn test_compiler_string_interpolation() {
    let script = r#"
let name = "Shae"
let version = 2
let msg = "Welcome to ${name} v${version}!"
msg
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(
        result,
        InterpretResult::Ok(Value::String("Welcome to Shae v2!".into()))
    );
}

#[test]
fn test_compiler_property_get_and_set() {
    let script = r#"
let user = { name: "Alice", age: 30 }
user.age = 31
user.role = "Admin"
user.name + " is " + str(user.age) + " (" + user.role + ")"
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(
        result,
        InterpretResult::Ok(Value::String("Alice is 31 (Admin)".into()))
    );
}

#[test]
fn test_compiler_safe_property_navigation() {
    let script = r#"
let user = { profile: null }
let title = user.profile?.title
title
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Null));
}

#[test]
fn test_compiler_array_destructuring() {
    let script = r#"
let [a, b, c] = [10, 20, 30]
a + b * c
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(610)));
}

#[test]
fn test_compiler_array_destructuring_with_rest() {
    let script = r#"
let [head, second, ..tail] = [1, 2, 3, 4, 5]
let tlen = tail.len
head + second + tlen
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(1 + 2 + 3)));
}

#[test]
fn test_compiler_object_destructuring() {
    let script = r#"
let user = { name: "Bob", score: 95 }
let { name, score } = user
name + ": " + str(score)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::String("Bob: 95".into())));
}

#[test]
fn test_compiler_object_destructuring_rename_and_rest() {
    let script = r#"
let config = { host: "localhost", port: 8080, secure: true }
let { host: server_host, ..rest_cfg } = config
server_host + ":" + str(rest_cfg.port)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(
        result,
        InterpretResult::Ok(Value::String("localhost:8080".into()))
    );
}

#[test]
fn test_compiler_nested_destructuring() {
    let script = r#"
let payload = ["ok", { code: 200, count: 42 }]
let [status, { code, count }] = payload
status + " " + str(code + count)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::String("ok 242".into())));
}

#[test]
fn test_compiler_struct_def_and_init() {
    let script = r#"
struct User { name, age, is_admin }
let u = User {
    name: "Alice",
    age: 25,
    is_admin: true
}
u.age = 26
u.age
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(26)));
}

#[test]
fn test_compiler_enum_and_pattern_match() {
    let script = r#"
enum Status {
    Pending(),
    Active(days),
    Banned(reason, days)
}

let s1 = Status.Active(42)
let s2 = Status.Banned("spam", 99)
let s3 = Status.Pending()

fn check_status(s) {
    match s {
        Status.Active(d) => "active for " + str(d) + " days",
        Status.Banned(r, d) => "banned for " + str(r),
        Status.Pending() => "waiting",
        _ => "unknown"
    }
}

[check_status(s1), check_status(s2), check_status(s3)]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("active for 42 days".into()));
            assert_eq!(b[1], Value::String("banned for spam".into()));
            assert_eq!(b[2], Value::String("waiting".into()));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_match_arm_guards() {
    let script = r#"
enum Task {
    Priority(level, title),
    Routine(title)
}

let t1 = Task.Priority(10, "Emergency Fix")
let t2 = Task.Priority(2, "Refactor Docs")
let t3 = Task.Routine("Drink Water")

fn handle_task(t) {
    match t {
        Task.Priority(lvl, title) if lvl >= 5 => "URGENT: " + title,
        Task.Priority(lvl, title) => "Normal: " + title,
        Task.Routine(title) => "Routine: " + title
    }
}

[handle_task(t1), handle_task(t2), handle_task(t3)]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("URGENT: Emergency Fix".into()));
            assert_eq!(b[1], Value::String("Normal: Refactor Docs".into()));
            assert_eq!(b[2], Value::String("Routine: Drink Water".into()));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_range_pattern_matching() {
    let script = r#"
fn category(age) {
    match age {
        0..=2 => "toddler",
        3..13 => "child",
        13..=19 => "teen",
        20..=64 => "adult",
        _ => "senior"
    }
}

[category(2), category(3), category(12), category(13), category(19), category(25), category(70)]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("toddler".into()));
            assert_eq!(b[1], Value::String("child".into()));
            assert_eq!(b[2], Value::String("child".into()));
            assert_eq!(b[3], Value::String("teen".into()));
            assert_eq!(b[4], Value::String("teen".into()));
            assert_eq!(b[5], Value::String("adult".into()));
            assert_eq!(b[6], Value::String("senior".into()));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_try_catch_unwinding() {
    let script = r#"
let result = "init"
let error_msg = ""
try {
    let temp = 10 + "string"
    result = "success"
} catch (e) {
    result = "caught"
    error_msg = e
}
[result, error_msg]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("caught".into()));
            match &b[1] {
                Value::String(s) => assert!(s.contains("Operands must be two numbers")),
                other => panic!("Expected string error, got {:?}", other),
            }
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_try_catch_across_function_calls() {
    let script = r#"
fn fail_fn() {
    let x = 1 / 0
    return x
}

let status = "ok"
try {
    fail_fn()
    status = "not reached"
} catch (err) {
    status = "caught: " + err
}
status
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(
        result,
        InterpretResult::Ok(Value::String("caught: Divide by zero.".into()))
    );
}

#[test]
fn test_compiler_non_exhaustive_match_error() {
    let script = r#"
let caught = false
try {
    match 99 {
        1 => "one",
        2 => "two"
    }
} catch (err) {
    caught = true
}
caught
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Bool(true)));
}

#[test]
fn test_compiler_pipeline_operator() {
    let script = r#"
fn double(x) { return x * 2 }
fn add(x, y) { return x + y }

let val = 5 |> double |> add(10)
val
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    assert_eq!(result, InterpretResult::Ok(Value::Int(20)));
}

#[test]
fn test_compiler_null_coalescing() {
    let script = r#"
let a = null ?? "default"
let b = "actual" ?? "fallback"
let c = null ?? null ?? 42
[a, b, c]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("default".into()));
            assert_eq!(b[1], Value::String("actual".into()));
            assert_eq!(b[2], Value::Int(42));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_map_properties_and_methods() {
    let script = r#"
let m = { a: 1, b: 2 }
let l = m.len
let has_a = m.has("a")
let has_z = m.has("z")
let val_b = m.get("b")
let val_d = m.get("d", 99)
[l, has_a, has_z, val_b, val_d]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    match result {
        InterpretResult::Ok(Value::Array(arr)) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(2));
            assert_eq!(b[1], Value::Bool(true));
            assert_eq!(b[2], Value::Bool(false));
            assert_eq!(b[3], Value::Int(2));
            assert_eq!(b[4], Value::Int(99));
        }
        other => panic!("Expected array result, got {:?}", other),
    }
}

#[test]
fn test_compiler_run_vm_api() {
    let source = "let count = 10\ncount * 3 + 2";
    let val = shae::run_vm(source).unwrap();
    assert_eq!(val, Value::Int(32));
}

#[test]
fn test_compiler_large_constant_pool() {
    // Generate script with 320 unique integer constants
    let mut script = String::new();
    script.push_str("let total = 0\n");
    for i in 1..=320 {
        script.push_str(&format!("total = total + {}\n", i));
    }
    script.push_str("total\n");

    let program = parse_source(&script).expect("Parsing should succeed");
    let chunk = Compiler::new()
        .compile_program(&program)
        .expect("Compilation should succeed");

    assert!(
        chunk.constants.len() >= 320,
        "Chunk should have at least 320 constants, got {}",
        chunk.constants.len()
    );

    // Verify opcode stream includes OpCode::ConstantLong (discriminant 62)
    let has_constant_long = chunk
        .code
        .contains(&(shae::opcode::OpCode::ConstantLong as u8));
    assert!(
        has_constant_long,
        "Compiled chunk must emit OpCode::ConstantLong for constants exceeding index 255"
    );

    let mut vm = VM::new();
    let result = vm.interpret(chunk);

    // Sum 1..=320 = 320 * 321 / 2 = 51360
    assert_eq!(result, InterpretResult::Ok(Value::Int(51360)));
}

#[test]
fn test_compiler_large_local_scope() {
    // Generate function with 320 local variables
    let mut script = String::new();
    script.push_str("fn run_large_locals() {\n");
    for i in 0..320 {
        script.push_str(&format!("    let x{} = 1\n", i));
    }
    // Mutate slot 300
    script.push_str("    x300 = 100\n");
    script.push_str("    return x0 + x300 + x319\n");
    script.push_str("}\n");
    script.push_str("run_large_locals()\n");

    let program = parse_source(&script).expect("Parsing should succeed");
    let chunk = Compiler::new()
        .compile_program(&program)
        .expect("Compilation should succeed");

    // Check inner compiled function for GetLocalLong and SetLocalLong
    let mut found_fn = false;
    for c in &chunk.constants {
        if let Value::CompiledFunction(f) = c {
            found_fn = true;
            let has_get_local_long = f
                .chunk
                .code
                .contains(&(shae::opcode::OpCode::GetLocalLong as u8));
            let has_set_local_long = f
                .chunk
                .code
                .contains(&(shae::opcode::OpCode::SetLocalLong as u8));
            assert!(
                has_get_local_long,
                "Inner function chunk must emit OpCode::GetLocalLong"
            );
            assert!(
                has_set_local_long,
                "Inner function chunk must emit OpCode::SetLocalLong"
            );
        }
    }
    assert!(found_fn, "Should have compiled function in constants");

    let mut vm = VM::new();
    let result = vm.interpret(chunk);
    // x0(1) + x300(100) + x319(1) = 102
    assert_eq!(result, InterpretResult::Ok(Value::Int(102)));
}

#[test]
fn test_compiler_combined_large_pool_and_locals() {
    let mut script = String::new();
    for i in 0..300 {
        script.push_str(&format!("let v{} = {}\n", i, i * 2));
    }
    script.push_str("v10 + v200 + v299\n");

    let val = shae::run_vm(&script).expect("VM run should succeed");
    // v10 = 20, v200 = 400, v299 = 598 => 20 + 400 + 598 = 1018
    assert_eq!(val, Value::Int(1018));
}

#[test]
fn test_vm_disassembler_source() {
    let script = r#"
fn calc(x) {
    return x * 2
}
let a = 10
let b = 20
if a < b {
    calc(a) + b
} else {
    a - b
}
"#;
    let disasm = shae::disassemble_source(script).expect("Disassembly should succeed");
    assert!(disasm.contains("== main =="));
    assert!(disasm.contains("Constants:"));
    assert!(disasm.contains("Disassembly:"));
    assert!(disasm.contains("Constant"));
    assert!(disasm.contains("DefineGlobal"));
    assert!(disasm.contains("GetLocal"));
    assert!(disasm.contains("Less"));
    assert!(disasm.contains("JumpIfFalse"));
    assert!(disasm.contains("Return"));
}

#[test]
fn test_vm_relative_module_import() {
    let dir = "tests/temp_vm_relative";
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();

    let helper_path = format!("{}/helper.shae", dir);
    fs::write(
        &helper_path,
        r#"
fn double(x) {
    return x * 2
}
let factor = 3
"#,
    )
    .unwrap();

    let main_path = format!("{}/main.shae", dir);
    fs::write(
        &main_path,
        r#"
let m = use "./helper.shae"
m.double(10) + m.factor
"#,
    )
    .unwrap();

    let result =
        shae::run_file_vm(&main_path).expect("VM run_file_vm should resolve relative import");
    assert_eq!(result, Value::Int(23));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_vm_nested_relative_module_import() {
    let dir = "tests/temp_vm_nested";
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(format!("{}/sub", dir)).unwrap();

    let leaf_path = format!("{}/sub/leaf.shae", dir);
    fs::write(
        &leaf_path,
        r#"
fn triple(x) {
    return x * 3
}
"#,
    )
    .unwrap();

    let mid_path = format!("{}/sub/mid.shae", dir);
    fs::write(
        &mid_path,
        r#"
let leaf = use "./leaf.shae"
fn compute(x) {
    return leaf.triple(x) + 5
}
"#,
    )
    .unwrap();

    let entry_path = format!("{}/entry.shae", dir);
    fs::write(
        &entry_path,
        r#"
let mid = use "./sub/mid.shae"
mid.compute(10)
"#,
    )
    .unwrap();

    let result =
        shae::run_file_vm(&entry_path).expect("Nested relative import should resolve in VM");
    assert_eq!(result, Value::Int(35));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_vm_engine_dispatch_helpers() {
    let script = "let x = 10; let y = 20; x * y";
    let val_ast = shae::run_with_engine(script, false).expect("AST run should succeed");
    let val_vm = shae::run_with_engine(script, true).expect("VM run should succeed");
    assert_eq!(val_ast, val_vm);
    assert_eq!(val_vm, Value::Int(200));

    let math_file = "tests/shae/math_test.shae";
    let res_ast = shae::run_file_with_engine(math_file, false);
    let res_vm = shae::run_file_with_engine(math_file, true);
    assert!(res_ast.is_ok());
    assert!(res_vm.is_ok());
}

#[test]
fn test_mega_test_on_vm() {
    let val =
        shae::run_file_vm("mega_test.shae").expect("mega_test.shae should execute cleanly on VM");
    assert_eq!(val, Value::Null);
    let _ = fs::remove_file("test_io.json");
}

#[test]
fn test_showcase_microservice_on_vm() {
    let val = shae::run_file_vm("examples/microservice/test_service.shae")
        .expect("microservice test_service.shae should execute cleanly on VM");
    assert_eq!(val, Value::Null);
}
