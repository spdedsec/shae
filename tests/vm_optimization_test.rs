use shae::compiler::Compiler;
use shae::lexer::tokenize;
use shae::parser::Parser;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

fn run_shae(src: &str) -> Value {
    let tokens = tokenize(src).expect("Lexing failed");
    let mut parser = Parser::new(tokens);
    let program = parser.parse().expect("Parsing failed");
    let compiler = Compiler::new();
    let chunk = compiler
        .compile_program(&program)
        .expect("Compilation failed");
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(e) => panic!("Runtime error: {}", e),
        InterpretResult::CompileError => panic!("Compile error in VM"),
    }
}

#[test]
fn test_vm_inline_caching_struct() {
    let script = r#"
struct Point { x, y }

let p = Point { x: 10, y: 20 }
let sum = 0
let i = 0
while i < 1000 {
    sum = sum + p.x + p.y
    p.x = p.x + 1
    i = i + 1
}

assert_eq(p.x, 1010)
assert_eq(p.y, 20)
"ic_struct_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("ic_struct_ok".into()));
}

#[test]
fn test_vm_inline_caching_map() {
    let script = r#"
let m = { count: 0, delta: 5 }
let i = 0
while i < 500 {
    m.count = m.count + m.delta
    i = i + 1
}

assert_eq(m.count, 2500)
"ic_map_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("ic_map_ok".into()));
}

#[test]
fn test_vm_tail_call_optimization_deep_recursion() {
    // Normal call stack depth limit in VM is 1024.
    // With TCO, this will run 25,000 iterations in constant stack space.
    let script = r#"
fn count_down(n, acc) {
    if n == 0 {
        return acc
    }
    return count_down(n - 1, acc + 1)
}

let result = count_down(25000, 0)
assert_eq(result, 25000)
"tco_deep_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("tco_deep_ok".into()));
}

#[test]
fn test_vm_tail_call_mutual_recursion() {
    // Test mutual tail recursion across two functions for 10,000 steps
    let script = r#"
fn is_even(n) {
    if n == 0 {
        return true
    }
    return is_odd(n - 1)
}

fn is_odd(n) {
    if n == 0 {
        return false
    }
    return is_even(n - 1)
}

assert(is_even(10000))
assert(is_odd(10001))
"tco_mutual_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("tco_mutual_ok".into()));
}

#[test]
fn test_vm_tail_call_implicit_return() {
    // Test tail call when call is the last statement without explicit 'return'
    let script = r#"
fn sum_to(n, acc) {
    if n == 0 {
        return acc
    }
    sum_to(n - 1, acc + n)
}

let result = sum_to(5000, 0)
// sum of 1..5000 is 5000 * 5001 / 2 = 12502500
assert_eq(result, 12502500)
"tco_implicit_ok"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("tco_implicit_ok".into()));
}
