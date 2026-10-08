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
