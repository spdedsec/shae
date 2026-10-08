use crate::ast::{BindingPattern, BinaryOp, Expr, Literal, Program, Stmt, StmtKind, UnaryOp};
use crate::chunk::Chunk;
use crate::opcode::OpCode;
use crate::value::Value;

#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub depth: usize,
}

pub struct Compiler {
    pub chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: usize,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
        }
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Chunk, String> {
        let len = program.statements.len();
        for (i, stmt) in program.statements.iter().enumerate() {
            let is_last = i + 1 == len;
            self.compile_stmt(stmt, is_last)?;
        }
        if len == 0 {
            self.chunk.write_opcode(OpCode::Nil, 0);
        }
        self.chunk.write_opcode(OpCode::Return, 0);
        Ok(self.chunk)
    }

    pub fn compile(mut self, expr: &Expr) -> Result<Chunk, String> {
        self.compile_expr(expr)?;
        self.chunk.write_opcode(OpCode::Return, 0);
        Ok(self.chunk)
    }

    pub fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    pub fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(local) = self.locals.last() {
            if local.depth > self.scope_depth {
                self.chunk.write_opcode(OpCode::Pop, 0);
                self.locals.pop();
            } else {
                break;
            }
        }
    }

    fn add_local(&mut self, name: String) -> usize {
        self.locals.push(Local {
            name,
            depth: self.scope_depth,
        });
        self.locals.len() - 1
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u8);
            }
        }
        None
    }

    pub fn compile_stmt(&mut self, stmt: &Stmt, is_last: bool) -> Result<(), String> {
        match &stmt.kind {
            StmtKind::Let { pattern, init } => {
                self.compile_expr(init)?;
                self.compile_binding_pattern(pattern)?;
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                self.compile_expr(value)?;
                match target {
                    Expr::Variable { name, span } => {
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.write_opcode(OpCode::SetLocal, span.line);
                            self.chunk.write(slot, span.line);
                        } else {
                            let const_idx = self.chunk.add_constant(Value::String(name.clone()));
                            self.chunk.write_opcode(OpCode::SetGlobal, span.line);
                            self.chunk.write(const_idx as u8, span.line);
                        }
                        if !is_last {
                            self.chunk.write_opcode(OpCode::Pop, span.line);
                        }
                        Ok(())
                    }
                    _ => Err(format!("Unsupported assign target in VM compiler: {:?}", target)),
                }
            }
            StmtKind::Expr(expr) => {
                self.compile_expr(expr)?;
                if !is_last {
                    self.chunk.write_opcode(OpCode::Pop, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.compile_expr(expr)?;
                } else {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                self.chunk.write_opcode(OpCode::Return, stmt.span.line);
                Ok(())
            }
            _ => Err(format!("Statement kind not yet supported in VM compiler: {:?}", stmt.kind)),
        }
    }

    fn compile_binding_pattern(&mut self, pattern: &BindingPattern) -> Result<(), String> {
        match pattern {
            BindingPattern::Ident(name) => {
                self.add_local(name.clone());
                Ok(())
            }
            _ => Err(format!("Destructuring patterns not yet supported in VM compiler: {:?}", pattern)),
        }
    }

    pub fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
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
            Expr::Variable { name, span } => {
                if let Some(slot) = self.resolve_local(name) {
                    self.chunk.write_opcode(OpCode::GetLocal, span.line);
                    self.chunk.write(slot, span.line);
                } else {
                    let const_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk.write_opcode(OpCode::GetGlobal, span.line);
                    self.chunk.write(const_idx as u8, span.line);
                }
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
                    BinaryOp::Mod => self.chunk.write_opcode(OpCode::Mod, span.line),
                    BinaryOp::Eq => self.chunk.write_opcode(OpCode::Equal, span.line),
                    BinaryOp::NotEq => {
                        self.chunk.write_opcode(OpCode::Equal, span.line);
                        self.chunk.write_opcode(OpCode::Not, span.line);
                    }
                    BinaryOp::Gt => self.chunk.write_opcode(OpCode::Greater, span.line),
                    BinaryOp::Lt => self.chunk.write_opcode(OpCode::Less, span.line),
                    BinaryOp::GtEq => {
                        self.chunk.write_opcode(OpCode::Less, span.line);
                        self.chunk.write_opcode(OpCode::Not, span.line);
                    }
                    BinaryOp::LtEq => {
                        self.chunk.write_opcode(OpCode::Greater, span.line);
                        self.chunk.write_opcode(OpCode::Not, span.line);
                    }
                    BinaryOp::BitAnd => self.chunk.write_opcode(OpCode::BitAnd, span.line),
                    BinaryOp::BitOr => self.chunk.write_opcode(OpCode::BitOr, span.line),
                    BinaryOp::BitXor => self.chunk.write_opcode(OpCode::BitXor, span.line),
                    BinaryOp::Shl => self.chunk.write_opcode(OpCode::Shl, span.line),
                    BinaryOp::Shr => self.chunk.write_opcode(OpCode::Shr, span.line),
                    _ => return Err(format!("Unsupported binary op {:?} for VM compilation", op)),
                }
                Ok(())
            }
            Expr::Unary { op, expr: right, span } => {
                self.compile_expr(right)?;
                match op {
                    UnaryOp::Neg => self.chunk.write_opcode(OpCode::Negate, span.line),
                    UnaryOp::Not => self.chunk.write_opcode(OpCode::Not, span.line),
                    UnaryOp::BitNot => self.chunk.write_opcode(OpCode::BitNot, span.line),
                }
                Ok(())
            }
            _ => Err(format!("Unsupported expression in VM compiler: {:?}", expr)),
        }
    }
}
