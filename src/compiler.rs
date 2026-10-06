use crate::ast::{Expr, Literal, BinaryOp, UnaryOp};
use crate::chunk::Chunk;
use crate::opcode::OpCode;
use crate::value::Value;

pub struct Compiler {
    pub chunk: Chunk,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
        }
    }

    pub fn compile(mut self, expr: &Expr) -> Result<Chunk, String> {
        self.compile_expr(expr)?;
        self.chunk.write_opcode(OpCode::Return, 0); // Temporary line 0
        Ok(self.chunk)
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Literal(Literal::Int(n)) => {
                let constant = self.chunk.add_constant(Value::Int(*n));
                self.chunk.write_opcode(OpCode::Constant, 0);
                self.chunk.write(constant as u8, 0);
                Ok(())
            }
            Expr::Literal(Literal::Float(n)) | Expr::Literal(Literal::Number(n)) => {
                let constant = self.chunk.add_constant(Value::Float(*n));
                self.chunk.write_opcode(OpCode::Constant, 0);
                self.chunk.write(constant as u8, 0);
                Ok(())
            }
            Expr::Literal(Literal::String(s)) => {
                let constant = self.chunk.add_constant(Value::String(s.clone()));
                self.chunk.write_opcode(OpCode::Constant, 0);
                self.chunk.write(constant as u8, 0);
                Ok(())
            }
            Expr::Literal(Literal::Bool(b)) => {
                if *b {
                    self.chunk.write_opcode(OpCode::True, 0);
                } else {
                    self.chunk.write_opcode(OpCode::False, 0);
                }
                Ok(())
            }
            Expr::Literal(Literal::Null) => {
                self.chunk.write_opcode(OpCode::Nil, 0);
                Ok(())
            }
            Expr::Binary { left, op, right, span } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                match op {
                    BinaryOp::Add => self.chunk.write_opcode(OpCode::Add, span.line),
                    BinaryOp::Sub => self.chunk.write_opcode(OpCode::Subtract, span.line),
                    BinaryOp::Mul => self.chunk.write_opcode(OpCode::Multiply, span.line),
                    BinaryOp::Div => self.chunk.write_opcode(OpCode::Divide, span.line),
                    // TODO: Implement other ops
                    _ => return Err(format!("Unsupported binary op {:?} for VM compilation", op)),
                }
                Ok(())
            }
            Expr::Unary { op, expr: right, span } => {
                self.compile_expr(right)?;
                match op {
                    UnaryOp::Neg => self.chunk.write_opcode(OpCode::Negate, span.line),
                    UnaryOp::Not => self.chunk.write_opcode(OpCode::Not, span.line),
                }
                Ok(())
            }
            _ => Err(format!("Unsupported expression in basic compiler: {:?}", expr)),
        }
    }
}
