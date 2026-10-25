use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, StmtKind};
use crate::lexer;
use crate::parser;
use std::collections::HashMap;

pub const WASM_MAGIC: &[u8; 4] = b"\0asm";
pub const WASM_VERSION: &[u8; 4] = &[1, 0, 0, 0];

// Type constants
pub const TYPE_I32: u8 = 0x7F;
pub const TYPE_I64: u8 = 0x7E;
pub const TYPE_F64: u8 = 0x7C;
pub const TYPE_FUNC: u8 = 0x60;

// Opcodes
pub const OP_END: u8 = 0x0B;
pub const OP_RETURN: u8 = 0x0F;
pub const OP_CALL: u8 = 0x10;
pub const OP_LOCAL_GET: u8 = 0x20;
pub const OP_LOCAL_SET: u8 = 0x21;
pub const OP_I32_CONST: u8 = 0x41;
pub const OP_I64_CONST: u8 = 0x42;
pub const OP_I64_EQZ: u8 = 0x50;
pub const OP_I64_EQ: u8 = 0x51;
pub const OP_I64_NE: u8 = 0x52;
pub const OP_I64_LT_S: u8 = 0x53;
pub const OP_I64_GT_S: u8 = 0x55;
pub const OP_I64_LE_S: u8 = 0x57;
pub const OP_I64_GE_S: u8 = 0x59;
pub const OP_I64_ADD: u8 = 0x7C;
pub const OP_I64_SUB: u8 = 0x7D;
pub const OP_I64_MUL: u8 = 0x7E;
pub const OP_I64_DIV_S: u8 = 0x7F;
pub const OP_I64_REM_S: u8 = 0x81;
pub const OP_I64_AND: u8 = 0x83;
pub const OP_I64_OR: u8 = 0x84;
pub const OP_I64_XOR: u8 = 0x85;
pub const OP_I64_SHL: u8 = 0x86;
pub const OP_I64_SHR_S: u8 = 0x87;

pub fn encode_u32_leb128(mut val: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
            bytes.push(byte);
        } else {
            bytes.push(byte);
            break;
        }
    }
    bytes
}

pub fn encode_i64_leb128(mut val: i64) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut more = true;
    while more {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        let sign_bit = (byte & 0x40) != 0;
        if (val == 0 && !sign_bit) || (val == -1 && sign_bit) {
            more = false;
        } else {
            byte |= 0x80;
        }
        bytes.push(byte);
    }
    bytes
}

pub fn encode_section(sec_id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 5 + payload.len());
    out.push(sec_id);
    out.extend_from_slice(&encode_u32_leb128(payload.len() as u32));
    out.extend_from_slice(payload);
    out
}

#[derive(Debug, Clone)]
pub struct WasmFunction {
    pub name: String,
    pub param_count: usize,
    pub has_return: bool,
    pub locals: Vec<u8>,
    pub body: Vec<u8>,
}

#[derive(Default)]
pub struct WasmCompiler {
    pub functions: Vec<WasmFunction>,
    pub func_indices: HashMap<String, u32>,
}

impl WasmCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn compile_source(&mut self, source: &str) -> Result<Vec<u8>, String> {
        let tokens = lexer::tokenize(source).map_err(|e| format!("Lexer error: {:?}", e))?;
        let program = parser::parse(tokens).map_err(|e| format!("Parser error: {:?}", e))?;
        self.compile_program(&program)
    }

    pub fn compile_program(&mut self, program: &Program) -> Result<Vec<u8>, String> {
        // Collect top-level functions
        for stmt in &program.statements {
            if let StmtKind::FnDef {
                name, params, body, ..
            } = &stmt.kind
            {
                let func_idx = self.functions.len() as u32;
                self.func_indices.insert(name.clone(), func_idx);

                let mut locals_map = HashMap::new();
                for (i, p) in params.iter().enumerate() {
                    locals_map.insert(p.name.clone(), i as u32);
                }

                let mut code_body = Vec::new();
                let mut extra_locals = 0u32;
                for s in body.iter() {
                    self.compile_stmt(
                        s,
                        &mut locals_map,
                        &mut extra_locals,
                        &mut code_body,
                        params.len() as u32,
                    )?;
                }
                code_body.push(OP_END);

                self.functions.push(WasmFunction {
                    name: name.clone(),
                    param_count: params.len(),
                    has_return: true,
                    locals: vec![TYPE_I64; extra_locals as usize],
                    body: code_body,
                });
            }
        }

        // Top-level statements become _start / main
        let mut main_locals = HashMap::new();
        let mut main_extra = 0u32;
        let mut main_body = Vec::new();
        for stmt in &program.statements {
            if !matches!(stmt.kind, StmtKind::FnDef { .. }) {
                self.compile_stmt(stmt, &mut main_locals, &mut main_extra, &mut main_body, 0)?;
            }
        }
        main_body.push(OP_END);

        let main_func_idx = self.functions.len() as u32;
        self.func_indices.insert("main".to_string(), main_func_idx);
        self.functions.push(WasmFunction {
            name: "main".to_string(),
            param_count: 0,
            has_return: true,
            locals: vec![TYPE_I64; main_extra as usize],
            body: main_body,
        });

        self.emit_binary()
    }

    fn compile_stmt(
        &self,
        stmt: &Stmt,
        locals: &mut HashMap<String, u32>,
        extra_locals: &mut u32,
        out: &mut Vec<u8>,
        base_slot: u32,
    ) -> Result<(), String> {
        match &stmt.kind {
            StmtKind::Let { pattern, init, .. } => {
                self.compile_expr(init, locals, out)?;
                if let crate::ast::BindingPattern::Ident(name) = pattern {
                    let slot = base_slot + *extra_locals;
                    *extra_locals += 1;
                    locals.insert(name.clone(), slot);
                    out.push(OP_LOCAL_SET);
                    out.extend_from_slice(&encode_u32_leb128(slot));
                }
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                if let Expr::Variable { name, .. } = target {
                    self.compile_expr(value, locals, out)?;
                    if let Some(&slot) = locals.get(name) {
                        out.push(OP_LOCAL_SET);
                        out.extend_from_slice(&encode_u32_leb128(slot));
                    }
                }
                Ok(())
            }
            StmtKind::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.compile_expr(expr, locals, out)?;
                } else {
                    out.push(OP_I64_CONST);
                    out.extend_from_slice(&encode_i64_leb128(0));
                }
                out.push(OP_RETURN);
                Ok(())
            }
            StmtKind::Expr(expr) => self.compile_expr(expr, locals, out),
            _ => Ok(()),
        }
    }

    fn compile_expr(
        &self,
        expr: &Expr,
        locals: &HashMap<String, u32>,
        out: &mut Vec<u8>,
    ) -> Result<(), String> {
        match expr {
            Expr::Literal(Literal::Int(n)) => {
                out.push(OP_I64_CONST);
                out.extend_from_slice(&encode_i64_leb128(*n));
            }
            Expr::Literal(Literal::Bool(b)) => {
                out.push(OP_I64_CONST);
                out.extend_from_slice(&encode_i64_leb128(if *b { 1 } else { 0 }));
            }
            Expr::Literal(Literal::Null) => {
                out.push(OP_I64_CONST);
                out.extend_from_slice(&encode_i64_leb128(0));
            }
            Expr::Variable { name, .. } => {
                if let Some(&slot) = locals.get(name) {
                    out.push(OP_LOCAL_GET);
                    out.extend_from_slice(&encode_u32_leb128(slot));
                } else {
                    out.push(OP_I64_CONST);
                    out.extend_from_slice(&encode_i64_leb128(0));
                }
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                self.compile_expr(left, locals, out)?;
                self.compile_expr(right, locals, out)?;
                match op {
                    BinaryOp::Add => out.push(OP_I64_ADD),
                    BinaryOp::Sub => out.push(OP_I64_SUB),
                    BinaryOp::Mul => out.push(OP_I64_MUL),
                    BinaryOp::Div => out.push(OP_I64_DIV_S),
                    BinaryOp::Mod => out.push(OP_I64_REM_S),
                    BinaryOp::BitAnd => out.push(OP_I64_AND),
                    BinaryOp::BitOr => out.push(OP_I64_OR),
                    BinaryOp::BitXor => out.push(OP_I64_XOR),
                    BinaryOp::Shl => out.push(OP_I64_SHL),
                    BinaryOp::Shr => out.push(OP_I64_SHR_S),
                    BinaryOp::Eq => out.push(OP_I64_EQ),
                    BinaryOp::NotEq => out.push(OP_I64_NE),
                    BinaryOp::Lt => out.push(OP_I64_LT_S),
                    BinaryOp::Gt => out.push(OP_I64_GT_S),
                    BinaryOp::LtEq => out.push(OP_I64_LE_S),
                    BinaryOp::GtEq => out.push(OP_I64_GE_S),
                    _ => out.push(OP_I64_ADD),
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Variable { name, .. } = &**callee {
                    for arg in args {
                        self.compile_expr(arg, locals, out)?;
                    }
                    if let Some(&func_idx) = self.func_indices.get(name) {
                        out.push(OP_CALL);
                        out.extend_from_slice(&encode_u32_leb128(func_idx));
                    }
                }
            }
            _ => {
                out.push(OP_I64_CONST);
                out.extend_from_slice(&encode_i64_leb128(0));
            }
        }
        Ok(())
    }

    pub fn emit_binary(&self) -> Result<Vec<u8>, String> {
        let mut wasm = Vec::new();
        // 1. Header
        wasm.extend_from_slice(WASM_MAGIC);
        wasm.extend_from_slice(WASM_VERSION);

        // 2. Type Section (1)
        let mut type_sec = Vec::new();
        type_sec.extend_from_slice(&encode_u32_leb128(self.functions.len() as u32));
        for func in &self.functions {
            type_sec.push(TYPE_FUNC);
            type_sec.extend_from_slice(&encode_u32_leb128(func.param_count as u32));
            for _ in 0..func.param_count {
                type_sec.push(TYPE_I64);
            }
            type_sec.extend_from_slice(&encode_u32_leb128(1)); // 1 result
            type_sec.push(TYPE_I64);
        }
        wasm.extend_from_slice(&encode_section(1, &type_sec));

        // 3. Function Section (3)
        let mut func_sec = Vec::new();
        func_sec.extend_from_slice(&encode_u32_leb128(self.functions.len() as u32));
        for (i, _) in self.functions.iter().enumerate() {
            func_sec.extend_from_slice(&encode_u32_leb128(i as u32));
        }
        wasm.extend_from_slice(&encode_section(3, &func_sec));

        // 4. Memory Section (5)
        let mut mem_sec = Vec::new();
        mem_sec.extend_from_slice(&encode_u32_leb128(1)); // 1 memory
        mem_sec.push(0); // flags = no max
        mem_sec.extend_from_slice(&encode_u32_leb128(1)); // min 1 page (64KB)
        wasm.extend_from_slice(&encode_section(5, &mem_sec));

        // 5. Export Section (7)
        let mut exp_sec = Vec::new();
        exp_sec.extend_from_slice(&encode_u32_leb128((self.functions.len() + 1) as u32));
        // Export memory
        let mem_name = b"memory";
        exp_sec.extend_from_slice(&encode_u32_leb128(mem_name.len() as u32));
        exp_sec.extend_from_slice(mem_name);
        exp_sec.push(2); // Export Memory
        exp_sec.extend_from_slice(&encode_u32_leb128(0));

        for (i, func) in self.functions.iter().enumerate() {
            let name_bytes = func.name.as_bytes();
            exp_sec.extend_from_slice(&encode_u32_leb128(name_bytes.len() as u32));
            exp_sec.extend_from_slice(name_bytes);
            exp_sec.push(0); // Export Function
            exp_sec.extend_from_slice(&encode_u32_leb128(i as u32));
        }
        wasm.extend_from_slice(&encode_section(7, &exp_sec));

        // 6. Code Section (10)
        let mut code_sec = Vec::new();
        code_sec.extend_from_slice(&encode_u32_leb128(self.functions.len() as u32));
        for func in &self.functions {
            let mut func_buf = Vec::new();
            if !func.locals.is_empty() {
                func_buf.extend_from_slice(&encode_u32_leb128(1));
                func_buf.extend_from_slice(&encode_u32_leb128(func.locals.len() as u32));
                func_buf.push(TYPE_I64);
            } else {
                func_buf.extend_from_slice(&encode_u32_leb128(0));
            }
            func_buf.extend_from_slice(&func.body);

            code_sec.extend_from_slice(&encode_u32_leb128(func_buf.len() as u32));
            code_sec.extend_from_slice(&func_buf);
        }
        wasm.extend_from_slice(&encode_section(10, &code_sec));

        Ok(wasm)
    }
}

/// Lightweight built-in WASM execution engine
pub struct WasmRunner;

impl WasmRunner {
    pub fn execute_function(
        wasm_bytes: &[u8],
        func_name: &str,
        args: &[i64],
    ) -> Result<i64, String> {
        if !wasm_bytes.starts_with(WASM_MAGIC) {
            return Err("Invalid WASM binary: magic mismatch".to_string());
        }

        let mut offset = 8;
        let mut func_idx_opt = None;
        let mut code_payload_opt = None;
        let mut type_payload_opt = None;
        let mut func_types_opt = None;

        while offset < wasm_bytes.len() {
            let sec_id = wasm_bytes[offset];
            offset += 1;
            let (sec_len, leb_len) = read_u32_leb128(&wasm_bytes[offset..])?;
            offset += leb_len;
            let sec_end = offset + sec_len as usize;

            if sec_id == 1 {
                type_payload_opt = Some(&wasm_bytes[offset..sec_end]);
            } else if sec_id == 3 {
                func_types_opt = Some(&wasm_bytes[offset..sec_end]);
            } else if sec_id == 7 {
                // Export section
                let mut cursor = offset;
                let (count, clen) = read_u32_leb128(&wasm_bytes[cursor..])?;
                cursor += clen;
                for _ in 0..count {
                    let (name_len, nlen) = read_u32_leb128(&wasm_bytes[cursor..])?;
                    cursor += nlen;
                    let name =
                        String::from_utf8_lossy(&wasm_bytes[cursor..cursor + name_len as usize]);
                    cursor += name_len as usize;
                    let kind = wasm_bytes[cursor];
                    cursor += 1;
                    let (idx, ilen) = read_u32_leb128(&wasm_bytes[cursor..])?;
                    cursor += ilen;
                    if kind == 0 && name == func_name {
                        func_idx_opt = Some(idx as usize);
                    }
                }
            } else if sec_id == 10 {
                code_payload_opt = Some(&wasm_bytes[offset..sec_end]);
            }
            offset = sec_end;
        }

        let func_idx = func_idx_opt.ok_or_else(|| format!("Export '{}' not found", func_name))?;
        let code_payload = code_payload_opt.ok_or("Code section not found")?;

        // Parse parameter counts from type and function sections
        let mut type_param_counts = Vec::new();
        if let Some(t_data) = type_payload_opt {
            let mut cursor = 0;
            let (num_types, tlen) = read_u32_leb128(&t_data[cursor..])?;
            cursor += tlen;
            for _ in 0..num_types {
                let _form = t_data[cursor];
                cursor += 1;
                let (params_count, plen) = read_u32_leb128(&t_data[cursor..])?;
                cursor += plen;
                cursor += params_count as usize; // skip param types
                let (results_count, rlen) = read_u32_leb128(&t_data[cursor..])?;
                cursor += rlen;
                cursor += results_count as usize; // skip result types
                type_param_counts.push(params_count as usize);
            }
        }

        let mut func_param_counts = Vec::new();
        if let Some(f_data) = func_types_opt {
            let mut cursor = 0;
            let (num_funcs, flen) = read_u32_leb128(&f_data[cursor..])?;
            cursor += flen;
            for _ in 0..num_funcs {
                let (type_idx, tlen) = read_u32_leb128(&f_data[cursor..])?;
                cursor += tlen;
                let p_count = type_param_counts
                    .get(type_idx as usize)
                    .copied()
                    .unwrap_or(0);
                func_param_counts.push(p_count);
            }
        }

        // Collect all function bodies
        let mut cursor = 0;
        let (num_funcs, clen) = read_u32_leb128(&code_payload[cursor..])?;
        cursor += clen;

        let mut all_bodies = Vec::with_capacity(num_funcs as usize);
        for _ in 0..num_funcs as usize {
            let (body_size, blen) = read_u32_leb128(&code_payload[cursor..])?;
            cursor += blen;
            let body_end = cursor + body_size as usize;
            all_bodies.push(&code_payload[cursor..body_end]);
            cursor = body_end;
        }

        if func_idx >= all_bodies.len() {
            return Err("Function index out of bounds in code section".to_string());
        }

        Self::run_body(all_bodies[func_idx], args, &all_bodies, &func_param_counts)
    }

    fn run_body(
        body: &[u8],
        args: &[i64],
        all_bodies: &[&[u8]],
        all_param_counts: &[usize],
    ) -> Result<i64, String> {
        let mut cursor = 0;
        let (local_vec_count, lclen) = read_u32_leb128(&body[cursor..])?;
        cursor += lclen;

        let mut locals = args.to_vec();
        for _ in 0..local_vec_count {
            let (count, clen) = read_u32_leb128(&body[cursor..])?;
            cursor += clen;
            let _val_type = body[cursor];
            cursor += 1;
            for _ in 0..count {
                locals.push(0);
            }
        }

        let mut stack: Vec<i64> = Vec::new();

        while cursor < body.len() {
            let op = body[cursor];
            cursor += 1;

            match op {
                OP_I64_CONST => {
                    let (val, len) = read_i64_leb128(&body[cursor..])?;
                    cursor += len;
                    stack.push(val);
                }
                OP_LOCAL_GET => {
                    let (idx, len) = read_u32_leb128(&body[cursor..])?;
                    cursor += len;
                    let val = locals.get(idx as usize).copied().unwrap_or(0);
                    stack.push(val);
                }
                OP_LOCAL_SET => {
                    let (idx, len) = read_u32_leb128(&body[cursor..])?;
                    cursor += len;
                    let val = stack.pop().unwrap_or(0);
                    if (idx as usize) < locals.len() {
                        locals[idx as usize] = val;
                    }
                }
                OP_CALL => {
                    let (target_idx, len) = read_u32_leb128(&body[cursor..])?;
                    cursor += len;
                    let target = target_idx as usize;
                    if target >= all_bodies.len() {
                        return Err(format!("Call target index out of range: {}", target));
                    }
                    let p_count = all_param_counts.get(target).copied().unwrap_or(0);
                    let mut call_args = vec![0i64; p_count];
                    for p in (0..p_count).rev() {
                        call_args[p] = stack.pop().unwrap_or(0);
                    }
                    let ret = Self::run_body(
                        all_bodies[target],
                        &call_args,
                        all_bodies,
                        all_param_counts,
                    )?;
                    stack.push(ret);
                }
                OP_I64_ADD => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(a.wrapping_add(b));
                }
                OP_I64_SUB => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(a.wrapping_sub(b));
                }
                OP_I64_MUL => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(a.wrapping_mul(b));
                }
                OP_I64_DIV_S => {
                    let b = stack.pop().unwrap_or(1);
                    let a = stack.pop().unwrap_or(0);
                    if b == 0 {
                        return Err("Division by zero in WASM execution".to_string());
                    }
                    stack.push(a / b);
                }
                OP_I64_EQ => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(if a == b { 1 } else { 0 });
                }
                OP_I64_NE => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(if a != b { 1 } else { 0 });
                }
                OP_I64_LT_S => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(if a < b { 1 } else { 0 });
                }
                OP_I64_GT_S => {
                    let b = stack.pop().unwrap_or(0);
                    let a = stack.pop().unwrap_or(0);
                    stack.push(if a > b { 1 } else { 0 });
                }
                OP_RETURN => {
                    return Ok(stack.pop().unwrap_or(0));
                }
                OP_END => {
                    return Ok(stack.pop().unwrap_or(0));
                }
                _ => {}
            }
        }

        Ok(stack.pop().unwrap_or(0))
    }
}

fn read_u32_leb128(bytes: &[u8]) -> Result<(u32, usize), String> {
    let mut res = 0u32;
    let mut shift = 0;
    for (i, &b) in bytes.iter().enumerate() {
        res |= ((b & 0x7F) as u32) << shift;
        if (b & 0x80) == 0 {
            return Ok((res, i + 1));
        }
        shift += 7;
        if shift >= 35 {
            return Err("LEB128 integer overflow".to_string());
        }
    }
    Err("Unexpected EOF in LEB128".to_string())
}

fn read_i64_leb128(bytes: &[u8]) -> Result<(i64, usize), String> {
    let mut res = 0i64;
    let mut shift = 0;
    for (i, &b) in bytes.iter().enumerate() {
        res |= ((b & 0x7F) as i64) << shift;
        shift += 7;
        if (b & 0x80) == 0 {
            if shift < 64 && (b & 0x40) != 0 {
                res |= !0 << shift;
            }
            return Ok((res, i + 1));
        }
    }
    Err("Unexpected EOF in LEB128".to_string())
}
