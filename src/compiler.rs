use crate::ast::{BindingPattern, BinaryOp, Expr, Literal, Program, Stmt, StmtKind, UnaryOp};
use crate::chunk::Chunk;
use crate::opcode::OpCode;
use crate::value::{CompiledFunction, UpvalueDesc, Value};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub depth: usize,
    pub is_captured: bool,
}

#[derive(Debug, Clone)]
struct LoopContext {
    start_ip: usize,
    scope_depth: usize,
    break_jumps: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct UpvalueInfo {
    pub index: u8,
    pub is_local: bool,
}

pub struct Compiler {
    pub chunk: Chunk,
    locals: Vec<Local>,
    upvalues: Vec<UpvalueInfo>,
    scope_depth: usize,
    loops: Vec<LoopContext>,
    enclosing: Option<Box<Compiler>>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            loops: Vec::new(),
            enclosing: None,
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

    pub fn compile_function(
        &mut self,
        name: Option<String>,
        params: &[String],
        body: &[Stmt],
    ) -> Result<CompiledFunction, String> {
        let parent = std::mem::replace(self, Compiler::new());
        let mut child = Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            loops: Vec::new(),
            enclosing: Some(Box::new(parent)),
        };

        let fn_slot_name = name.clone().unwrap_or_default();
        child.add_local(fn_slot_name);

        for param in params {
            child.add_local(param.clone());
        }

        let body_len = body.len();
        for (i, stmt) in body.iter().enumerate() {
            let is_last = i + 1 == body_len;
            child.compile_stmt(stmt, is_last)?;
        }

        if body.is_empty() {
            child.chunk.write_opcode(OpCode::Nil, 0);
        }
        child.chunk.write_opcode(OpCode::Return, 0);

        let upvalue_descs: Vec<UpvalueDesc> = child
            .upvalues
            .iter()
            .map(|u| UpvalueDesc {
                index: u.index,
                is_local: u.is_local,
            })
            .collect();

        let compiled = CompiledFunction {
            arity: params.len(),
            chunk: child.chunk,
            name,
            upvalues: upvalue_descs,
        };

        *self = *child.enclosing.unwrap();

        Ok(compiled)
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        if let Some(ref mut enclosing) = self.enclosing {
            if let Some(local_slot) = enclosing.resolve_local(name) {
                enclosing.locals[local_slot as usize].is_captured = true;
                return Some(self.add_upvalue(local_slot, true));
            }
            if let Some(upvalue_idx) = enclosing.resolve_upvalue(name) {
                return Some(self.add_upvalue(upvalue_idx, false));
            }
        }
        None
    }

    fn add_upvalue(&mut self, index: u8, is_local: bool) -> u8 {
        for (i, uv) in self.upvalues.iter().enumerate() {
            if uv.index == index && uv.is_local == is_local {
                return i as u8;
            }
        }
        self.upvalues.push(UpvalueInfo { index, is_local });
        (self.upvalues.len() - 1) as u8
    }

    pub fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    pub fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(local) = self.locals.last() {
            if local.depth > self.scope_depth {
                if local.is_captured {
                    self.chunk.write_opcode(OpCode::CloseUpvalue, 0);
                } else {
                    self.chunk.write_opcode(OpCode::Pop, 0);
                }
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
            is_captured: false,
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

    fn emit_jump(&mut self, op: OpCode, line: usize) -> usize {
        self.chunk.write_opcode(op, line);
        self.chunk.write(0xff, line);
        self.chunk.write(0xff, line);
        self.chunk.code.len() - 2
    }

    fn patch_jump(&mut self, offset: usize) -> Result<(), String> {
        let jump = self.chunk.code.len() - offset - 2;
        if jump > u16::MAX as usize {
            return Err("Jump offset too large.".into());
        }
        self.chunk.code[offset] = ((jump >> 8) & 0xff) as u8;
        self.chunk.code[offset + 1] = (jump & 0xff) as u8;
        Ok(())
    }

    fn emit_loop(&mut self, loop_start: usize, line: usize) -> Result<(), String> {
        self.chunk.write_opcode(OpCode::Loop, line);
        let offset = self.chunk.code.len() - loop_start + 2;
        if offset > u16::MAX as usize {
            return Err("Loop body too large.".into());
        }
        self.chunk.write(((offset >> 8) & 0xff) as u8, line);
        self.chunk.write((offset & 0xff) as u8, line);
        Ok(())
    }

    fn pop_locals_above(&mut self, depth: usize, line: usize) {
        let mut to_pop = Vec::new();
        for local in self.locals.iter().rev() {
            if local.depth > depth {
                to_pop.push(local.is_captured);
            } else {
                break;
            }
        }
        for is_captured in to_pop {
            if is_captured {
                self.chunk.write_opcode(OpCode::CloseUpvalue, line);
            } else {
                self.chunk.write_opcode(OpCode::Pop, line);
            }
        }
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
                match target {
                    Expr::Variable { name, span } => {
                        self.compile_expr(value)?;
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.write_opcode(OpCode::SetLocal, span.line);
                            self.chunk.write(slot, span.line);
                        } else if let Some(upvalue_slot) = self.resolve_upvalue(name) {
                            self.chunk.write_opcode(OpCode::SetUpvalue, span.line);
                            self.chunk.write(upvalue_slot, span.line);
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
                    Expr::Index {
                        target: idx_target,
                        index,
                        span,
                    } => {
                        self.compile_expr(idx_target)?;
                        self.compile_expr(index)?;
                        self.compile_expr(value)?;
                        self.chunk.write_opcode(OpCode::IndexSet, span.line);
                        if !is_last {
                            self.chunk.write_opcode(OpCode::Pop, span.line);
                        }
                        Ok(())
                    }
                    _ => Err(format!("Unsupported assign target in VM compiler: {:?}", target)),
                }
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.compile_expr(condition)?;
                let then_jump = self.emit_jump(OpCode::JumpIfFalse, stmt.span.line);
                self.chunk.write_opcode(OpCode::Pop, stmt.span.line);

                self.begin_scope();
                for s in then_branch {
                    self.compile_stmt(s, false)?;
                }
                self.end_scope();

                let else_jump = self.emit_jump(OpCode::Jump, stmt.span.line);
                self.patch_jump(then_jump)?;
                self.chunk.write_opcode(OpCode::Pop, stmt.span.line);

                if let Some(else_stmts) = else_branch {
                    self.begin_scope();
                    for s in else_stmts {
                        self.compile_stmt(s, false)?;
                    }
                    self.end_scope();
                }
                self.patch_jump(else_jump)?;

                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::While { condition, body } => {
                let loop_start = self.chunk.code.len();
                let loop_depth = self.scope_depth;
                self.loops.push(LoopContext {
                    start_ip: loop_start,
                    scope_depth: loop_depth,
                    break_jumps: Vec::new(),
                });

                self.compile_expr(condition)?;
                let exit_jump = self.emit_jump(OpCode::JumpIfFalse, stmt.span.line);
                self.chunk.write_opcode(OpCode::Pop, stmt.span.line);

                self.begin_scope();
                for s in body {
                    self.compile_stmt(s, false)?;
                }
                self.end_scope();

                self.emit_loop(loop_start, stmt.span.line)?;
                self.patch_jump(exit_jump)?;
                self.chunk.write_opcode(OpCode::Pop, stmt.span.line);

                let loop_ctx = self.loops.pop().unwrap();
                for break_jump in loop_ctx.break_jumps {
                    self.patch_jump(break_jump)?;
                }

                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::For {
                item,
                iterable,
                body,
            } => {
                self.begin_scope();
                self.compile_expr(iterable)?;
                let seq_slot = self.add_local("(seq)".to_string());

                let const_zero = self.chunk.add_constant(Value::Int(0));
                self.chunk.write_opcode(OpCode::Constant, stmt.span.line);
                self.chunk.write(const_zero as u8, stmt.span.line);
                self.add_local("(iter)".to_string());

                let loop_start = self.chunk.code.len();
                let loop_depth = self.scope_depth;
                self.loops.push(LoopContext {
                    start_ip: loop_start,
                    scope_depth: loop_depth,
                    break_jumps: Vec::new(),
                });

                self.chunk.write_opcode(OpCode::ForIter, stmt.span.line);
                self.chunk.write(seq_slot as u8, stmt.span.line);
                self.chunk.write(0xff, stmt.span.line);
                self.chunk.write(0xff, stmt.span.line);
                let exit_jump = self.chunk.code.len() - 2;

                self.begin_scope();
                self.add_local(item.clone());
                for s in body {
                    self.compile_stmt(s, false)?;
                }
                self.end_scope();

                self.emit_loop(loop_start, stmt.span.line)?;
                self.patch_jump(exit_jump)?;

                let loop_ctx = self.loops.pop().unwrap();
                for break_jump in loop_ctx.break_jumps {
                    self.patch_jump(break_jump)?;
                }

                self.end_scope();

                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::Break => {
                if self.loops.is_empty() {
                    return Err("Cannot 'break' outside of a loop.".into());
                }
                let loop_depth = self.loops.last().unwrap().scope_depth;
                self.pop_locals_above(loop_depth, stmt.span.line);
                let jump = self.emit_jump(OpCode::Jump, stmt.span.line);
                self.loops.last_mut().unwrap().break_jumps.push(jump);
                Ok(())
            }
            StmtKind::Continue => {
                if let Some(loop_ctx) = self.loops.last() {
                    let loop_depth = loop_ctx.scope_depth;
                    let start_ip = loop_ctx.start_ip;
                    self.pop_locals_above(loop_depth, stmt.span.line);
                    self.emit_loop(start_ip, stmt.span.line)?;
                    Ok(())
                } else {
                    Err("Cannot 'continue' outside of a loop.".into())
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
            StmtKind::FnDef { name, params, body } => {
                let compiled = self.compile_function(Some(name.clone()), params, body)?;
                let const_idx = self
                    .chunk
                    .add_constant(Value::CompiledFunction(Arc::new(compiled)));
                self.chunk.write_opcode(OpCode::Closure, stmt.span.line);
                self.chunk.write(const_idx as u8, stmt.span.line);
                if self.scope_depth > 0 {
                    self.add_local(name.clone());
                } else {
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk.write_opcode(OpCode::DefineGlobal, stmt.span.line);
                    self.chunk.write(name_idx as u8, stmt.span.line);
                }
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
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
                } else if let Some(upvalue_slot) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(OpCode::GetUpvalue, span.line);
                    self.chunk.write(upvalue_slot, span.line);
                } else {
                    let const_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk.write_opcode(OpCode::GetGlobal, span.line);
                    self.chunk.write(const_idx as u8, span.line);
                }
                Ok(())
            }
            Expr::Binary { left, op, right, span } => {
                match op {
                    BinaryOp::And => {
                        self.compile_expr(left)?;
                        let end_jump = self.emit_jump(OpCode::JumpIfFalse, span.line);
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                        self.compile_expr(right)?;
                        self.patch_jump(end_jump)?;
                        Ok(())
                    }
                    BinaryOp::Or => {
                        self.compile_expr(left)?;
                        let else_jump = self.emit_jump(OpCode::JumpIfFalse, span.line);
                        let end_jump = self.emit_jump(OpCode::Jump, span.line);
                        self.patch_jump(else_jump)?;
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                        self.compile_expr(right)?;
                        self.patch_jump(end_jump)?;
                        Ok(())
                    }
                    _ => {
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
                }
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
            Expr::Array(elements) => {
                if elements.len() > 255 {
                    return Err("Array literal exceeds maximum 255 elements in VM".into());
                }
                for elem in elements {
                    self.compile_expr(elem)?;
                }
                self.chunk.write_opcode(OpCode::BuildList, 0);
                self.chunk.write(elements.len() as u8, 0);
                Ok(())
            }
            Expr::Index {
                target,
                index,
                span,
            } => {
                self.compile_expr(target)?;
                self.compile_expr(index)?;
                self.chunk.write_opcode(OpCode::IndexGet, span.line);
                Ok(())
            }
            Expr::Map(pairs) => {
                if pairs.len() > 255 {
                    return Err("Map literal exceeds maximum 255 pairs in VM".into());
                }
                for (key, val) in pairs {
                    let const_idx = self.chunk.add_constant(Value::String(key.clone()));
                    self.chunk.write_opcode(OpCode::Constant, 0);
                    self.chunk.write(const_idx as u8, 0);
                    self.compile_expr(val)?;
                }
                self.chunk.write_opcode(OpCode::BuildMap, 0);
                self.chunk.write(pairs.len() as u8, 0);
                Ok(())
            }
            Expr::Call { callee, args, span } => {
                if args.len() > 255 {
                    return Err("Function call cannot exceed 255 arguments".into());
                }
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.write_opcode(OpCode::Call, span.line);
                self.chunk.write(args.len() as u8, span.line);
                Ok(())
            }
            Expr::Lambda { params, body, span } => {
                let compiled = self.compile_function(None, params, body)?;
                let const_idx = self
                    .chunk
                    .add_constant(Value::CompiledFunction(Arc::new(compiled)));
                self.chunk.write_opcode(OpCode::Closure, span.line);
                self.chunk.write(const_idx as u8, span.line);
                Ok(())
            }
            _ => Err(format!("Unsupported expression in VM compiler: {:?}", expr)),
        }
    }
}
