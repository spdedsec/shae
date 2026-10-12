use crate::opcode::OpCode;
use crate::value::{CompiledFunction, Value};
use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, Block, InstBuilder, MachMemFlags,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use cranelift_native;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};

pub type JitFnPtr = unsafe extern "C" fn(*const i64, usize, *mut i64) -> i32;

pub const JIT_THRESHOLD: usize = 20;

#[inline]
fn mem_flags() -> MachMemFlags {
    MachMemFlags::trusted()
}

pub struct JitEngine {
    builder_context: FunctionBuilderContext,
    ctx: Context,
    module: JITModule,
    next_func_id: AtomicUsize,
}

impl JitEngine {
    pub fn new() -> Result<Self, String> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").ok();
        flag_builder.set("is_pic", "false").ok();

        let isa_builder = cranelift_native::builder()
            .map_err(|msg| format!("Host machine is not supported: {}", msg))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| e.to_string())?;

        let builder = JITBuilder::with_isa(isa, default_libcall_names());
        let module = JITModule::new(builder);

        Ok(JitEngine {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
            next_func_id: AtomicUsize::new(1),
        })
    }

    pub fn is_supported_opcode(op: OpCode) -> bool {
        matches!(
            op,
            OpCode::Return
                | OpCode::Constant
                | OpCode::ConstantLong
                | OpCode::Nil
                | OpCode::True
                | OpCode::False
                | OpCode::Pop
                | OpCode::Dup
                | OpCode::GetLocal
                | OpCode::SetLocal
                | OpCode::GetLocalLong
                | OpCode::SetLocalLong
                | OpCode::Equal
                | OpCode::Greater
                | OpCode::Less
                | OpCode::Add
                | OpCode::Subtract
                | OpCode::Multiply
                | OpCode::Divide
                | OpCode::Mod
                | OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::BitXor
                | OpCode::BitNot
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::Not
                | OpCode::Negate
                | OpCode::Jump
                | OpCode::JumpIfFalse
                | OpCode::Loop
        )
    }

    pub fn can_compile(func: &CompiledFunction) -> bool {
        if func.is_async || !func.upvalues.is_empty() {
            return false;
        }

        let chunk = &func.chunk;
        let mut offset = 0;
        while offset < chunk.code.len() {
            let op_byte = chunk.code[offset];
            if op_byte > OpCode::Await as u8 {
                return false;
            }
            let opcode: OpCode = op_byte.into();
            if !Self::is_supported_opcode(opcode) {
                return false;
            }

            match opcode {
                OpCode::Constant => {
                    let idx = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    if chunk
                        .constants
                        .get(idx)
                        .is_some_and(|c| !matches!(c, Value::Int(_) | Value::Bool(_) | Value::Null))
                    {
                        return false;
                    }
                    offset += 2;
                }
                OpCode::ConstantLong => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let idx = (hi << 8) | lo;
                    if chunk
                        .constants
                        .get(idx)
                        .is_some_and(|c| !matches!(c, Value::Int(_) | Value::Bool(_) | Value::Null))
                    {
                        return false;
                    }
                    offset += 3;
                }
                OpCode::GetLocal | OpCode::SetLocal => offset += 2,
                OpCode::GetLocalLong | OpCode::SetLocalLong => offset += 3,
                OpCode::Jump | OpCode::JumpIfFalse | OpCode::Loop => offset += 3,
                _ => offset += 1,
            }
        }

        true
    }

    pub fn compile_function(&mut self, func: &CompiledFunction) -> Result<JitFnPtr, String> {
        if !Self::can_compile(func) {
            return Err("Function contains non-JIT constructs".into());
        }

        let chunk = &func.chunk;
        let arity = func.arity;

        let func_name = format!(
            "shae_jit_fn_{}_{}",
            func.name.as_deref().unwrap_or("anon"),
            self.next_func_id.fetch_add(1, Ordering::SeqCst)
        );

        let pointer_type = self.module.target_config().pointer_type();

        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // Signature: extern "C" fn(args: *const i64, num_args: usize, out_val: *mut i64) -> i32
        self.ctx.func.signature.params.push(AbiParam::new(pointer_type));
        self.ctx.func.signature.params.push(AbiParam::new(pointer_type));
        self.ctx.func.signature.params.push(AbiParam::new(pointer_type));
        self.ctx.func.signature.returns.push(AbiParam::new(types::I32));

        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Export, &self.ctx.func.signature)
            .map_err(|e| format!("Declaration error: {}", e))?;

        let mut builder =
            FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

        // Pre-scan block starts
        let mut block_starts = BTreeSet::new();
        block_starts.insert(0);

        let mut offset = 0;
        let mut max_local = arity;

        while offset < chunk.code.len() {
            let opcode: OpCode = chunk.code[offset].into();
            match opcode {
                OpCode::Constant => offset += 2,
                OpCode::ConstantLong => offset += 3,
                OpCode::GetLocal | OpCode::SetLocal => {
                    let slot = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    if slot > max_local {
                        max_local = slot;
                    }
                    offset += 2;
                }
                OpCode::GetLocalLong | OpCode::SetLocalLong => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let slot = (hi << 8) | lo;
                    if slot > max_local {
                        max_local = slot;
                    }
                    offset += 3;
                }
                OpCode::Jump => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = offset + 3 + jump;
                    block_starts.insert(target);
                    offset += 3;
                }
                OpCode::JumpIfFalse => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = offset + 3 + jump;
                    block_starts.insert(target);
                    block_starts.insert(offset + 3);
                    offset += 3;
                }
                OpCode::Loop => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = (offset + 3).saturating_sub(jump);
                    block_starts.insert(target);
                    offset += 3;
                }
                OpCode::Return => {
                    offset += 1;
                }
                _ => offset += 1,
            }
        }

        block_starts.retain(|&idx| idx < chunk.code.len());

        // Create Cranelift blocks for all block starts
        let mut blocks: HashMap<usize, Block> = HashMap::new();
        for &start in &block_starts {
            blocks.insert(start, builder.create_block());
        }

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let args_ptr = builder.block_params(entry_block)[0];
        let num_args = builder.block_params(entry_block)[1];
        let out_ptr = builder.block_params(entry_block)[2];

        // Declare unified local variables and evaluation stack slots
        let total_vars = (max_local + 500).max(1000);
        let mut vars: Vec<Variable> = Vec::with_capacity(total_vars);
        for _ in 0..total_vars {
            vars.push(builder.declare_var(types::I64));
        }
        let var = |idx: usize| vars[idx];

        // Deopt bailout block
        let bailout_block = builder.create_block();
        let div_zero_block = builder.create_block();

        // Setup parameters and local variables in entry
        let zero_const = builder.ins().iconst(types::I64, 0);
        builder.def_var(var(0), zero_const);

        for i in 0..arity {
            let offset_val = (i * 8) as i32;
            let loaded = builder.ins().load(
                types::I64,
                mem_flags(),
                args_ptr,
                offset_val,
            );
            builder.def_var(var(1 + i), loaded);
        }

        for i in (1 + arity)..total_vars {
            builder.def_var(var(i), zero_const);
        }

        // Check arity
        let expected_arity = builder.ins().iconst(pointer_type, arity as i64);
        let arity_match = builder.ins().icmp(IntCC::Equal, num_args, expected_arity);

        let first_block = blocks[&0];
        builder
            .ins()
            .brif(arity_match, first_block, &[], bailout_block, &[]);

        builder.switch_to_block(first_block);

        // Track stack depth per block (stack pointer starts at 1 + arity)
        let mut block_sp: HashMap<usize, usize> = HashMap::new();
        block_sp.insert(0, 1 + arity);

        let mut cur_sp = 1 + arity;
        let mut block_terminated = false;
        offset = 0;

        let code_len = chunk.code.len();
        while offset < code_len {
            if offset != 0 {
                if let Some(&blk) = blocks.get(&offset) {
                    if let Some(&sp) = block_sp.get(&offset) {
                        cur_sp = sp;
                    }
                    if !block_terminated {
                        builder.ins().jump(blk, &[]);
                    }
                    builder.switch_to_block(blk);
                    block_terminated = false;
                }
            }

            if block_terminated {
                let opcode: OpCode = chunk.code[offset].into();
                match opcode {
                    OpCode::Constant => offset += 2,
                    OpCode::ConstantLong => offset += 3,
                    OpCode::GetLocal | OpCode::SetLocal => offset += 2,
                    OpCode::GetLocalLong | OpCode::SetLocalLong => offset += 3,
                    OpCode::Jump | OpCode::JumpIfFalse | OpCode::Loop => offset += 3,
                    _ => offset += 1,
                }
                continue;
            }

            let opcode: OpCode = chunk.code[offset].into();
            match opcode {
                OpCode::Constant => {
                    let idx = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let val = match &chunk.constants[idx] {
                        Value::Int(n) => builder.ins().iconst(types::I64, *n),
                        Value::Bool(b) => {
                            builder.ins().iconst(types::I64, if *b { 1 } else { 0 })
                        }
                        _ => builder.ins().iconst(types::I64, 0),
                    };
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 2;
                }
                OpCode::ConstantLong => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let idx = (hi << 8) | lo;
                    let val = match &chunk.constants[idx] {
                        Value::Int(n) => builder.ins().iconst(types::I64, *n),
                        Value::Bool(b) => {
                            builder.ins().iconst(types::I64, if *b { 1 } else { 0 })
                        }
                        _ => builder.ins().iconst(types::I64, 0),
                    };
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 3;
                }
                OpCode::Nil | OpCode::False => {
                    let val = builder.ins().iconst(types::I64, 0);
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 1;
                }
                OpCode::True => {
                    let val = builder.ins().iconst(types::I64, 1);
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 1;
                }
                OpCode::GetLocal => {
                    let slot = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let val = builder.use_var(var(slot));
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 2;
                }
                OpCode::SetLocal => {
                    let slot = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let val = builder.use_var(var(cur_sp - 1));
                    builder.def_var(var(slot), val);
                    offset += 2;
                }
                OpCode::GetLocalLong => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let slot = (hi << 8) | lo;
                    let val = builder.use_var(var(slot));
                    builder.def_var(var(cur_sp), val);
                    cur_sp += 1;
                    offset += 3;
                }
                OpCode::SetLocalLong => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let slot = (hi << 8) | lo;
                    let val = builder.use_var(var(cur_sp - 1));
                    builder.def_var(var(slot), val);
                    offset += 3;
                }
                OpCode::Pop => {
                    cur_sp = cur_sp.saturating_sub(1);
                    offset += 1;
                }
                OpCode::Dup => {
                    if cur_sp > 0 {
                        let val = builder.use_var(var(cur_sp - 1));
                        builder.def_var(var(cur_sp), val);
                        cur_sp += 1;
                    }
                    offset += 1;
                }
                OpCode::Add => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().iadd(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Subtract => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().isub(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Multiply => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().imul(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Divide => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let is_zero = builder.ins().icmp(IntCC::Equal, b, zero);
                    let safe_div_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_zero, div_zero_block, &[], safe_div_block, &[]);

                    builder.switch_to_block(safe_div_block);
                    builder.seal_block(safe_div_block);
                    let res = builder.ins().sdiv(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Mod => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let is_zero = builder.ins().icmp(IntCC::Equal, b, zero);
                    let safe_mod_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_zero, div_zero_block, &[], safe_mod_block, &[]);

                    builder.switch_to_block(safe_mod_block);
                    builder.seal_block(safe_mod_block);
                    let res = builder.ins().srem(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Negate => {
                    let a = builder.use_var(var(cur_sp - 1));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let res = builder.ins().isub(zero, a);
                    builder.def_var(var(cur_sp - 1), res);
                    offset += 1;
                }
                OpCode::BitAnd => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().band(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::BitOr => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().bor(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::BitXor => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().bxor(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::BitNot => {
                    let a = builder.use_var(var(cur_sp - 1));
                    let res = builder.ins().bnot(a);
                    builder.def_var(var(cur_sp - 1), res);
                    offset += 1;
                }
                OpCode::Shl => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().ishl(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Shr => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let res = builder.ins().sshr(a, b);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Equal => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let cmp = builder.ins().icmp(IntCC::Equal, a, b);
                    let res = builder.ins().uextend(types::I64, cmp);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Less => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let cmp = builder.ins().icmp(IntCC::SignedLessThan, a, b);
                    let res = builder.ins().uextend(types::I64, cmp);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Greater => {
                    let b = builder.use_var(var(cur_sp - 1));
                    let a = builder.use_var(var(cur_sp - 2));
                    let cmp = builder.ins().icmp(IntCC::SignedGreaterThan, a, b);
                    let res = builder.ins().uextend(types::I64, cmp);
                    builder.def_var(var(cur_sp - 2), res);
                    cur_sp -= 1;
                    offset += 1;
                }
                OpCode::Not => {
                    let a = builder.use_var(var(cur_sp - 1));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let cmp = builder.ins().icmp(IntCC::Equal, a, zero);
                    let res = builder.ins().uextend(types::I64, cmp);
                    builder.def_var(var(cur_sp - 1), res);
                    offset += 1;
                }
                OpCode::Jump => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = offset + 3 + jump;
                    block_sp.insert(target, cur_sp);
                    let target_block = blocks[&target];
                    builder.ins().jump(target_block, &[]);
                    block_terminated = true;
                    offset += 3;
                }
                OpCode::JumpIfFalse => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = offset + 3 + jump;
                    let cond = builder.use_var(var(cur_sp - 1));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let is_false = builder.ins().icmp(IntCC::Equal, cond, zero);

                    block_sp.insert(target, cur_sp);
                    block_sp.insert(offset + 3, cur_sp);

                    let target_block = blocks[&target];
                    let next_block = blocks[&(offset + 3)];
                    builder
                        .ins()
                        .brif(is_false, target_block, &[], next_block, &[]);
                    block_terminated = true;
                    offset += 3;
                }
                OpCode::Loop => {
                    let hi = chunk.code.get(offset + 1).copied().unwrap_or(0) as usize;
                    let lo = chunk.code.get(offset + 2).copied().unwrap_or(0) as usize;
                    let jump = (hi << 8) | lo;
                    let target = (offset + 3).saturating_sub(jump);
                    block_sp.insert(target, cur_sp);
                    let target_block = blocks[&target];
                    builder.ins().jump(target_block, &[]);
                    block_terminated = true;
                    offset += 3;
                }
                OpCode::Return => {
                    let ret_val = if cur_sp > 0 {
                        builder.use_var(var(cur_sp - 1))
                    } else {
                        builder.ins().iconst(types::I64, 0)
                    };
                    builder
                        .ins()
                        .store(mem_flags(), ret_val, out_ptr, 0);
                    let status_ok = builder.ins().iconst(types::I32, 0);
                    builder.ins().return_(&[status_ok]);
                    block_terminated = true;
                    offset += 1;
                }
                _ => {
                    // Non-JIT opcode reached: deopt
                    builder.ins().jump(bailout_block, &[]);
                    block_terminated = true;
                    offset += 1;
                }
            }
        }

        if !block_terminated {
            builder.ins().jump(bailout_block, &[]);
        }

        // Seal all blocks
        for &blk in blocks.values() {
            builder.seal_block(blk);
        }

        // Bailout block
        builder.switch_to_block(bailout_block);
        builder.seal_block(bailout_block);
        let status_bailout = builder.ins().iconst(types::I32, 1);
        builder.ins().return_(&[status_bailout]);

        // Div by zero block
        builder.switch_to_block(div_zero_block);
        builder.seal_block(div_zero_block);
        let status_div_zero = builder.ins().iconst(types::I32, 2);
        builder.ins().return_(&[status_div_zero]);

        builder.finalize(self.module.target_config());

        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| format!("Definition error: {}", e))?;

        self.module.clear_context(&mut self.ctx);
        self.module
            .finalize_definitions()
            .map_err(|e| format!("Finalize error: {}", e))?;

        let code_ptr = self.module.get_finalized_function(func_id);
        let fn_ptr: JitFnPtr = unsafe { std::mem::transmute(code_ptr) };
        Ok(fn_ptr)
    }
}
