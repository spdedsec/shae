use shae::ast::{BinaryOp, Expr, Literal, Span};
use shae::compiler::Compiler;
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

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
    let chunk = compiler.compile_program(&program).expect("Compilation should succeed");

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
    let chunk = compiler.compile_program(&program).expect("Compilation should succeed");

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
    let chunk = compiler.compile_program(&program).expect("Compilation should succeed");

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
    let chunk = compiler.compile_program(&program).expect("Compilation should succeed");

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
    let chunk = compiler.compile_program(&program).expect("Compilation should succeed");

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
