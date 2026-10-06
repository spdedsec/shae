use shae::ast::{Expr, Literal, BinaryOp, Span};
use shae::compiler::Compiler;
use shae::vm::{VM, InterpretResult};
use shae::value::Value;

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
