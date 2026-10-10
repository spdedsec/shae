use crate::ast::{BinaryOp, BindingPattern, Expr, Program, Stmt, StmtKind, UnaryOp};
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
                enclosing.locals[local_slot].is_captured = true;
                return Some(self.add_upvalue(local_slot as u8, true));
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

    fn resolve_local(&self, name: &str) -> Option<usize> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i);
            }
        }
        None
    }

    fn emit_constant(&mut self, val: Value, line: usize) {
        let idx = self.chunk.add_constant(val);
        if idx <= 255 {
            self.chunk.write_opcode(OpCode::Constant, line);
            self.chunk.write(idx as u8, line);
        } else {
            self.chunk.write_opcode(OpCode::ConstantLong, line);
            self.chunk.write(((idx >> 8) & 0xff) as u8, line);
            self.chunk.write((idx & 0xff) as u8, line);
        }
    }

    fn emit_get_local(&mut self, slot: usize, line: usize) {
        if slot <= 255 {
            self.chunk.write_opcode(OpCode::GetLocal, line);
            self.chunk.write(slot as u8, line);
        } else {
            self.chunk.write_opcode(OpCode::GetLocalLong, line);
            self.chunk.write(((slot >> 8) & 0xff) as u8, line);
            self.chunk.write((slot & 0xff) as u8, line);
        }
    }

    fn emit_set_local(&mut self, slot: usize, line: usize) {
        if slot <= 255 {
            self.chunk.write_opcode(OpCode::SetLocal, line);
            self.chunk.write(slot as u8, line);
        } else {
            self.chunk.write_opcode(OpCode::SetLocalLong, line);
            self.chunk.write(((slot >> 8) & 0xff) as u8, line);
            self.chunk.write((slot & 0xff) as u8, line);
        }
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
            StmtKind::Assign { target, value } => match target {
                Expr::Variable { name, span } => {
                    self.compile_expr(value)?;
                    if let Some(slot) = self.resolve_local(name) {
                        self.emit_set_local(slot, span.line);
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
                Expr::Get {
                    target: get_target,
                    property,
                    span,
                    ..
                } => {
                    self.compile_expr(get_target)?;
                    self.compile_expr(value)?;
                    let name_idx = self.chunk.add_constant(Value::String(property.clone()));
                    self.chunk.write_opcode(OpCode::SetProperty, span.line);
                    self.chunk.write(name_idx as u8, span.line);
                    if !is_last {
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                    }
                    Ok(())
                }
                _ => Err(format!(
                    "Unsupported assign target in VM compiler: {:?}",
                    target
                )),
            },
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

                self.emit_constant(Value::Int(0), stmt.span.line);
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
                    self.chunk
                        .write_opcode(OpCode::DefineGlobal, stmt.span.line);
                    self.chunk.write(name_idx as u8, stmt.span.line);
                }
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::StructDef { name, fields } => {
                self.emit_constant(
                    Value::StructDef {
                        name: name.clone(),
                        fields: fields.clone(),
                    },
                    stmt.span.line,
                );
                if self.scope_depth > 0 {
                    self.add_local(name.clone());
                } else {
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk
                        .write_opcode(OpCode::DefineGlobal, stmt.span.line);
                    self.chunk.write(name_idx as u8, stmt.span.line);
                }
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::EnumDef { name, variants } => {
                let mut var_map = indexmap::IndexMap::new();
                for v in variants {
                    var_map.insert(v.name.clone(), v.fields.clone());
                }
                self.emit_constant(
                    Value::EnumDef {
                        name: name.clone(),
                        variants: Arc::new(var_map),
                    },
                    stmt.span.line,
                );
                if self.scope_depth > 0 {
                    self.add_local(name.clone());
                } else {
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk
                        .write_opcode(OpCode::DefineGlobal, stmt.span.line);
                    self.chunk.write(name_idx as u8, stmt.span.line);
                }
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::TryCatch {
                try_body,
                catch_ident,
                catch_body,
            } => {
                let catch_jump = self.emit_jump(OpCode::PushTry, stmt.span.line);

                self.begin_scope();
                for s in try_body {
                    self.compile_stmt(s, false)?;
                }
                self.end_scope();

                self.chunk.write_opcode(OpCode::PopTry, stmt.span.line);
                let exit_jump = self.emit_jump(OpCode::Jump, stmt.span.line);

                self.patch_jump(catch_jump)?;

                self.begin_scope();
                self.add_local(catch_ident.clone());
                for s in catch_body {
                    self.compile_stmt(s, false)?;
                }
                self.end_scope();

                self.patch_jump(exit_jump)?;

                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
            StmtKind::Use { imports, path } => {
                self.emit_constant(Value::String(path.clone()), stmt.span.line);
                self.chunk
                    .write_opcode(OpCode::ImportModule, stmt.span.line);
                let mod_slot = self.add_local("(import_mod)".to_string());
                for item in imports {
                    if item.name == "*" {
                        self.emit_get_local(mod_slot, stmt.span.line);
                        self.chunk.write_opcode(OpCode::ImportStar, stmt.span.line);
                    } else {
                        self.emit_get_local(mod_slot, stmt.span.line);
                        let key_const = self.chunk.add_constant(Value::String(item.name.clone()));
                        self.chunk.write_opcode(OpCode::GetProperty, stmt.span.line);
                        self.chunk.write(key_const as u8, stmt.span.line);
                        self.chunk.write(0, stmt.span.line); // not safe
                        let bind_name = item.alias.as_ref().unwrap_or(&item.name);
                        self.add_local(bind_name.clone());
                    }
                }
                if is_last {
                    self.chunk.write_opcode(OpCode::Nil, stmt.span.line);
                }
                Ok(())
            }
        }
    }

    fn compile_binding_pattern(&mut self, pattern: &BindingPattern) -> Result<(), String> {
        match pattern {
            BindingPattern::Ident(name) => {
                self.add_local(name.clone());
                Ok(())
            }
            BindingPattern::Array { elements, rest } => {
                let arr_slot = self.add_local("(destruct_arr)".to_string());
                for (i, elem) in elements.iter().enumerate() {
                    self.emit_get_local(arr_slot, 0);
                    self.emit_constant(Value::Int(i as i64), 0);
                    self.chunk.write_opcode(OpCode::IndexGetSafe, 0);
                    self.compile_binding_pattern(elem)?;
                }
                if let Some(rest_name) = rest {
                    self.emit_get_local(arr_slot, 0);
                    self.emit_constant(Value::Int(elements.len() as i64), 0);
                    self.chunk.write_opcode(OpCode::ArraySlice, 0);
                    self.add_local(rest_name.clone());
                }
                Ok(())
            }
            BindingPattern::Object { fields, rest } => {
                let obj_slot = self.add_local("(destruct_obj)".to_string());
                for (field_name, opt_sub) in fields {
                    self.emit_get_local(obj_slot, 0);
                    let const_idx = self.chunk.add_constant(Value::String(field_name.clone()));
                    self.chunk.write_opcode(OpCode::GetProperty, 0);
                    self.chunk.write(const_idx as u8, 0);
                    self.chunk.write(1, 0); // is_safe = 1
                    if let Some(sub) = opt_sub {
                        self.compile_binding_pattern(sub)?;
                    } else {
                        self.add_local(field_name.clone());
                    }
                }
                if let Some(rest_name) = rest {
                    self.emit_get_local(obj_slot, 0);
                    for (field_name, _) in fields {
                        self.emit_constant(Value::String(field_name.clone()), 0);
                    }
                    self.chunk.write_opcode(OpCode::MapRest, 0);
                    self.chunk.write(fields.len() as u8, 0);
                    self.add_local(rest_name.clone());
                }
                Ok(())
            }
        }
    }

    fn compile_literal(&mut self, lit: &crate::ast::Literal, line: usize) -> Result<(), String> {
        use crate::ast::Literal;
        match lit {
            Literal::Int(n) => {
                self.emit_constant(Value::Int(*n), line);
            }
            Literal::Float(n) | Literal::Number(n) => {
                self.emit_constant(Value::Float(*n), line);
            }
            Literal::String(s) => {
                self.emit_constant(Value::String(s.clone()), line);
            }
            Literal::Bool(b) => {
                if *b {
                    self.chunk.write_opcode(OpCode::True, line);
                } else {
                    self.chunk.write_opcode(OpCode::False, line);
                }
            }
            Literal::Null => {
                self.chunk.write_opcode(OpCode::Nil, line);
            }
        }
        Ok(())
    }

    pub fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Literal(lit) => {
                self.compile_literal(lit, 0)?;
                Ok(())
            }
            Expr::Variable { name, span } => {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_get_local(slot, span.line);
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
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => match op {
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
                BinaryOp::Coalesce => {
                    self.compile_expr(left)?;
                    self.chunk.write_opcode(OpCode::Dup, span.line);
                    self.chunk.write_opcode(OpCode::Nil, span.line);
                    self.chunk.write_opcode(OpCode::Equal, span.line);
                    let not_null_jump = self.emit_jump(OpCode::JumpIfFalse, span.line);
                    // Left was null:
                    self.chunk.write_opcode(OpCode::Pop, span.line);
                    self.chunk.write_opcode(OpCode::Pop, span.line);
                    self.compile_expr(right)?;
                    let end_jump = self.emit_jump(OpCode::Jump, span.line);
                    // Left was not null:
                    self.patch_jump(not_null_jump)?;
                    self.chunk.write_opcode(OpCode::Pop, span.line);
                    self.patch_jump(end_jump)?;
                    Ok(())
                }
                BinaryOp::Pipe => {
                    match &**right {
                        Expr::Call {
                            callee,
                            args,
                            span: call_span,
                        } => {
                            if args.len() + 1 > 255 {
                                return Err("Argument count exceeds 255 in pipeline call".into());
                            }
                            self.compile_expr(callee)?;
                            self.compile_expr(left)?;
                            for arg in args {
                                self.compile_expr(arg)?;
                            }
                            self.chunk.write_opcode(OpCode::Call, call_span.line);
                            self.chunk.write((args.len() + 1) as u8, call_span.line);
                        }
                        _ => {
                            self.compile_expr(right)?;
                            self.compile_expr(left)?;
                            self.chunk.write_opcode(OpCode::Call, span.line);
                            self.chunk.write(1, span.line);
                        }
                    }
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
                        _ => {
                            return Err(format!(
                                "Unsupported binary op {:?} for VM compilation",
                                op
                            ));
                        }
                    }
                    Ok(())
                }
            },
            Expr::Unary {
                op,
                expr: right,
                span,
            } => {
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
                    self.emit_constant(Value::String(key.clone()), 0);
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
            Expr::Get {
                target,
                property,
                safe,
                span,
            } => {
                self.compile_expr(target)?;
                let name_idx = self.chunk.add_constant(Value::String(property.clone()));
                self.chunk.write_opcode(OpCode::GetProperty, span.line);
                self.chunk.write(name_idx as u8, span.line);
                self.chunk.write(if *safe { 1 } else { 0 }, span.line);
                Ok(())
            }
            Expr::Interpolated(parts) => {
                if parts.len() > 255 {
                    return Err("Interpolated string exceeds maximum 255 segments in VM".into());
                }
                for part in parts {
                    match part {
                        crate::ast::InterpPart::Text(s) => {
                            self.emit_constant(Value::String(s.clone()), 0);
                        }
                        crate::ast::InterpPart::Expr(e) => {
                            self.compile_expr(e)?;
                        }
                    }
                }
                self.chunk.write_opcode(OpCode::FormatString, 0);
                self.chunk.write(parts.len() as u8, 0);
                Ok(())
            }
            Expr::StructInit { name, fields, span } => {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_get_local(slot, span.line);
                } else if let Some(upvalue_slot) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(OpCode::GetUpvalue, span.line);
                    self.chunk.write(upvalue_slot, span.line);
                } else {
                    let const_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.chunk.write_opcode(OpCode::GetGlobal, span.line);
                    self.chunk.write(const_idx as u8, span.line);
                }
                for (f_name, f_expr) in fields {
                    self.emit_constant(Value::String(f_name.clone()), span.line);
                    self.compile_expr(f_expr)?;
                }
                self.chunk.write_opcode(OpCode::BuildStruct, span.line);
                self.chunk.write(fields.len() as u8, span.line);
                Ok(())
            }
            Expr::Match { target, arms, span } => {
                self.compile_expr(target)?;
                let target_slot = self.add_local("(match_target)".to_string());
                let mut end_jumps = Vec::new();

                for arm in arms {
                    let arm_start_locals = self.locals.len();
                    let mut arm_cond_fails = Vec::new();
                    let mut arm_guard_fails = Vec::new();

                    self.compile_match_pattern(
                        &arm.pattern,
                        target_slot,
                        &mut arm_cond_fails,
                        &mut arm_guard_fails,
                        arm_start_locals,
                        span.line,
                    )?;

                    if let Some(guard) = &arm.guard {
                        self.compile_expr(guard)?;
                        let bound_count = self.locals.len() - arm_start_locals;
                        let guard_fail = self.emit_jump(OpCode::JumpIfFalse, span.line);
                        arm_guard_fails.push((guard_fail, bound_count));
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                    }

                    self.compile_expr(&arm.body)?;
                    self.emit_set_local(target_slot, span.line);
                    self.chunk.write_opcode(OpCode::Pop, span.line);

                    let bound_count = self.locals.len() - arm_start_locals;
                    for _ in 0..bound_count {
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                        self.locals.pop();
                    }

                    let end_jump = self.emit_jump(OpCode::Jump, span.line);
                    end_jumps.push(end_jump);

                    let mut next_arm_jump = None;
                    if !arm_guard_fails.is_empty() {
                        for (fail_jump, bound_count) in arm_guard_fails {
                            self.patch_jump(fail_jump)?;
                            self.chunk.write_opcode(OpCode::Pop, span.line);
                            for _ in 0..bound_count {
                                self.chunk.write_opcode(OpCode::Pop, span.line);
                            }
                        }
                        next_arm_jump = Some(self.emit_jump(OpCode::Jump, span.line));
                    }

                    for cond_fail in arm_cond_fails {
                        self.patch_jump(cond_fail)?;
                        self.chunk.write_opcode(OpCode::Pop, span.line);
                    }

                    if let Some(j) = next_arm_jump {
                        self.patch_jump(j)?;
                    }

                    while self.locals.len() > arm_start_locals {
                        self.locals.pop();
                    }
                }

                self.chunk.write_opcode(OpCode::MatchError, span.line);

                for end_jump in end_jumps {
                    self.patch_jump(end_jump)?;
                }

                self.locals.pop();
                Ok(())
            }
            Expr::Use { path, span } => {
                self.compile_expr(path)?;
                self.chunk.write_opcode(OpCode::ImportModule, span.line);
                Ok(())
            }
        }
    }

    fn compile_match_pattern(
        &mut self,
        pattern: &crate::ast::Pattern,
        target_slot: usize,
        cond_fails: &mut Vec<usize>,
        guard_fails: &mut Vec<(usize, usize)>,
        arm_start_locals: usize,
        line: usize,
    ) -> Result<(), String> {
        use crate::ast::Pattern;
        match pattern {
            Pattern::Wildcard => Ok(()),
            Pattern::Variable(name) => {
                self.emit_get_local(target_slot, line);
                self.add_local(name.clone());
                Ok(())
            }
            Pattern::Literal(lit) => {
                self.emit_get_local(target_slot, line);
                self.compile_literal(lit, line)?;
                self.chunk.write_opcode(OpCode::Equal, line);
                let fail_jump = self.emit_jump(OpCode::JumpIfFalse, line);
                cond_fails.push(fail_jump);
                self.chunk.write_opcode(OpCode::Pop, line);
                Ok(())
            }
            Pattern::Range {
                start,
                end,
                inclusive,
            } => {
                self.emit_get_local(target_slot, line);
                self.compile_literal(start, line)?;
                self.compile_literal(end, line)?;
                self.chunk.write_opcode(OpCode::MatchRange, line);
                self.chunk.write(if *inclusive { 1 } else { 0 }, line);
                let fail_jump = self.emit_jump(OpCode::JumpIfFalse, line);
                cond_fails.push(fail_jump);
                self.chunk.write_opcode(OpCode::Pop, line);
                Ok(())
            }
            Pattern::Enum {
                enum_name,
                variant_name,
                fields,
            } => {
                self.emit_get_local(target_slot, line);
                let enum_const = match enum_name {
                    Some(e) => self.chunk.add_constant(Value::String(e.clone())),
                    None => self.chunk.add_constant(Value::Null),
                };
                let variant_const = self.chunk.add_constant(Value::String(variant_name.clone()));
                self.chunk.write_opcode(OpCode::MatchEnum, line);
                self.chunk.write(fields.len() as u8, line);
                self.chunk.write(variant_const as u8, line);
                self.chunk.write(enum_const as u8, line);

                let enum_fail = self.emit_jump(OpCode::JumpIfFalse, line);
                cond_fails.push(enum_fail);
                self.chunk.write_opcode(OpCode::Pop, line);

                for (i, f_pat) in fields.iter().enumerate() {
                    self.emit_get_local(target_slot, line);
                    self.emit_constant(Value::Int(i as i64), line);
                    self.chunk.write_opcode(OpCode::IndexGet, line);

                    match f_pat {
                        Pattern::Wildcard => {
                            self.chunk.write_opcode(OpCode::Pop, line);
                        }
                        Pattern::Variable(name) => {
                            self.add_local(name.clone());
                        }
                        Pattern::Literal(lit) => {
                            self.compile_literal(lit, line)?;
                            self.chunk.write_opcode(OpCode::Equal, line);
                            let bound_count = self.locals.len() - arm_start_locals;
                            let lit_fail = self.emit_jump(OpCode::JumpIfFalse, line);
                            guard_fails.push((lit_fail, bound_count));
                            self.chunk.write_opcode(OpCode::Pop, line);
                        }
                        other => {
                            return Err(format!(
                                "Nested enum pattern not yet supported in VM: {:?}",
                                other
                            ));
                        }
                    }
                }
                Ok(())
            }
        }
    }
}
