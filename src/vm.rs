use crate::chunk::Chunk;
use crate::gc::{GcHeap, GcStats};
use crate::opcode::OpCode;
use crate::value::{
    Closure, CompiledFunction, IndexError, Upvalue, UpvalueLocation, Value, resolve_index,
    resolve_int_index,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone)]
pub struct TryHandler {
    pub catch_ip: usize,
    pub stack_len: usize,
}

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub closure: Arc<Closure>,
    pub ip: usize,
    pub slots_offset: usize,
    pub try_handlers: Vec<TryHandler>,
}

#[derive(Debug, Clone)]
pub struct InlineCacheEntry {
    pub property: String,
    pub index: usize,
}

pub struct VM {
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: HashMap<String, Value>,
    pub module_globals: Option<Arc<RwLock<HashMap<String, Value>>>>,
    pub open_upvalues: Vec<Arc<RwLock<Upvalue>>>,
    pub heap: GcHeap,
    pub current_file: Option<std::path::PathBuf>,
    pub embedded_archive: Option<Arc<HashMap<String, String>>>,
    pub embedded_bytecode: Option<Arc<HashMap<String, Vec<u8>>>>,
    pub prop_get_ic: HashMap<(usize, usize), InlineCacheEntry>,
    pub prop_set_ic: HashMap<(usize, usize), InlineCacheEntry>,
    pub profiler: Option<crate::profiler::Profiler>,
    pub debug_session: Option<Arc<Mutex<crate::debugger::DebugSession>>>,
    pub jit_enabled: bool,
    pub jit_engine: Option<Arc<Mutex<crate::jit::JitEngine>>>,
}

#[derive(Debug, PartialEq)]
pub enum InterpretResult {
    Ok(Value),
    CompileError,
    RuntimeError(String),
}

fn to_f64_val(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(n) | Value::Number(n) => Some(*n),
        _ => None,
    }
}

impl VM {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        let mut dummy_env = crate::env::Environment::new();
        crate::builtins::register(&mut dummy_env);
        for (k, v) in dummy_env.export_map() {
            globals.insert(k, v);
        }
        globals.insert(
            "gc".to_string(),
            Value::Builtin {
                name: "gc".to_string(),
                func: |_ev, _args, _span| Ok(Value::Int(0)),
            },
        );
        globals.insert(
            "jit".to_string(),
            Value::Builtin {
                name: "jit".to_string(),
                func: |_ev, _args, _span| Ok(Value::String("enabled".to_string())),
            },
        );
        let jit_engine = crate::jit::JitEngine::new()
            .ok()
            .map(|e| Arc::new(Mutex::new(e)));
        VM {
            frames: Vec::with_capacity(64),
            stack: Vec::with_capacity(256),
            globals,
            module_globals: None,
            open_upvalues: Vec::new(),
            heap: GcHeap::new(),
            current_file: None,
            embedded_archive: None,
            embedded_bytecode: None,
            prop_get_ic: HashMap::new(),
            prop_set_ic: HashMap::new(),
            profiler: None,
            debug_session: None,
            jit_enabled: true,
            jit_engine,
        }
    }

    pub fn set_jit_enabled(&mut self, enabled: bool) {
        self.jit_enabled = enabled;
    }

    pub fn compile_function_jit(
        &mut self,
        func: &CompiledFunction,
    ) -> Result<crate::jit::JitFnPtr, String> {
        if let Some(engine) = &self.jit_engine {
            let mut eng = engine.lock().map_err(|e| e.to_string())?;
            let ptr = eng.compile_function(func)?;
            func.set_jit_fn(ptr);
            Ok(ptr)
        } else {
            Err("JIT engine not available".into())
        }
    }

    fn try_exec_jit(
        &mut self,
        func: &Arc<CompiledFunction>,
        callee_slot: usize,
    ) -> Option<Result<Value, String>> {
        if !self.jit_enabled || func.is_async || !func.upvalues.is_empty() {
            return None;
        }

        let invocations = func.record_invocation();
        if func.get_jit_fn().is_none()
            && invocations >= crate::jit::JIT_THRESHOLD
            && crate::jit::JitEngine::can_compile(func)
        {
            if let Some(engine) = &self.jit_engine {
                if let Ok(mut eng) = engine.lock() {
                    if let Ok(fn_ptr) = eng.compile_function(func) {
                        func.set_jit_fn(fn_ptr);
                    }
                }
            }
        }

        if let Some(jit_fn) = func.get_jit_fn() {
            let args_slice = &self.stack[callee_slot + 1..];
            let is_float = func.is_float_candidate();
            if is_float {
                let is_all_num = args_slice
                    .iter()
                    .all(|v| matches!(v, Value::Int(_) | Value::Float(_) | Value::Number(_)));
                if is_all_num {
                    let raw_args: Vec<i64> = args_slice
                        .iter()
                        .map(|v| match v {
                            Value::Int(i) => (*i as f64).to_bits() as i64,
                            Value::Float(f) | Value::Number(f) => f.to_bits() as i64,
                            _ => 0,
                        })
                        .collect();
                    let mut out_res: i64 = 0;
                    let status = unsafe { jit_fn(raw_args.as_ptr(), raw_args.len(), &mut out_res) };
                    if status == 3 {
                        return Some(Ok(Value::Float(f64::from_bits(out_res as u64))));
                    } else if status == 0 {
                        return Some(Ok(Value::Int(out_res)));
                    } else if status == 2 {
                        return Some(Err("Division by zero in native JIT execution".into()));
                    }
                }
            } else {
                let mut int_args: Vec<i64> = Vec::with_capacity(args_slice.len());
                let mut array_buffers: Vec<Vec<i64>> = Vec::new();
                let mut can_run = true;

                for v in args_slice {
                    match v {
                        Value::Int(i) => int_args.push(*i),
                        Value::Array(arr) => {
                            if let Ok(guard) = arr.read() {
                                if guard.iter().all(|elem| matches!(elem, Value::Int(_))) {
                                    let mut buf = Vec::with_capacity(1 + guard.len());
                                    buf.push(guard.len() as i64);
                                    for elem in guard.iter() {
                                        if let Value::Int(n) = elem {
                                            buf.push(*n);
                                        }
                                    }
                                    array_buffers.push(buf);
                                    int_args.push(array_buffers.last().unwrap().as_ptr() as i64);
                                } else {
                                    can_run = false;
                                    break;
                                }
                            } else {
                                can_run = false;
                                break;
                            }
                        }
                        Value::GcArray(r) => {
                            if let Some(guard) = self.heap.as_array(*r) {
                                if guard.iter().all(|elem| matches!(elem, Value::Int(_))) {
                                    let mut buf = Vec::with_capacity(1 + guard.len());
                                    buf.push(guard.len() as i64);
                                    for elem in guard.iter() {
                                        if let Value::Int(n) = elem {
                                            buf.push(*n);
                                        }
                                    }
                                    array_buffers.push(buf);
                                    int_args.push(array_buffers.last().unwrap().as_ptr() as i64);
                                } else {
                                    can_run = false;
                                    break;
                                }
                            } else {
                                can_run = false;
                                break;
                            }
                        }
                        _ => {
                            can_run = false;
                            break;
                        }
                    }
                }

                if can_run {
                    let mut out_res: i64 = 0;
                    let status = unsafe { jit_fn(int_args.as_ptr(), int_args.len(), &mut out_res) };
                    if status == 0 {
                        return Some(Ok(Value::Int(out_res)));
                    } else if status == 2 {
                        return Some(Err("Division by zero in native JIT execution".into()));
                    }
                }
            }
        }

        None
    }

    pub fn collect_garbage(&mut self) -> GcStats {
        let stack = &self.stack;
        let globals = &self.globals;
        let open_upvalues = &self.open_upvalues;
        let frames = &self.frames;

        self.heap.collect_garbage(|heap, gray_stack| {
            for val in stack {
                heap.mark_value(val, gray_stack);
            }
            for val in globals.values() {
                heap.mark_value(val, gray_stack);
            }
            if let Some(mg) = &self.module_globals {
                for val in mg.read().unwrap().values() {
                    heap.mark_value(val, gray_stack);
                }
            }
            for uv in open_upvalues {
                let guard = uv.read().unwrap();
                if let UpvalueLocation::Closed(ref val) = guard.location {
                    heap.mark_value(val, gray_stack);
                }
            }
            for frame in frames {
                if let Some(mg) = &frame.closure.module_globals {
                    for val in mg.read().unwrap().values() {
                        heap.mark_value(val, gray_stack);
                    }
                }
                for uv in &frame.closure.upvalues {
                    let guard = uv.read().unwrap();
                    if let UpvalueLocation::Closed(ref val) = guard.location {
                        heap.mark_value(val, gray_stack);
                    }
                }
            }
        })
    }

    pub fn collect_minor(&mut self) -> GcStats {
        let stack = &self.stack;
        let globals = &self.globals;
        let open_upvalues = &self.open_upvalues;
        let frames = &self.frames;

        self.heap.collect_minor(|heap, gray_stack| {
            for val in stack {
                heap.mark_value(val, gray_stack);
            }
            for val in globals.values() {
                heap.mark_value(val, gray_stack);
            }
            if let Some(mg) = &self.module_globals {
                for val in mg.read().unwrap().values() {
                    heap.mark_value(val, gray_stack);
                }
            }
            for uv in open_upvalues {
                let guard = uv.read().unwrap();
                if let UpvalueLocation::Closed(ref val) = guard.location {
                    heap.mark_value(val, gray_stack);
                }
            }
            for frame in frames {
                if let Some(mg) = &frame.closure.module_globals {
                    for val in mg.read().unwrap().values() {
                        heap.mark_value(val, gray_stack);
                    }
                }
                for uv in &frame.closure.upvalues {
                    let guard = uv.read().unwrap();
                    if let UpvalueLocation::Closed(ref val) = guard.location {
                        heap.mark_value(val, gray_stack);
                    }
                }
            }
        })
    }

    pub fn collect_major(&mut self) -> GcStats {
        self.collect_garbage()
    }

    pub fn interpret(&mut self, chunk: Chunk) -> InterpretResult {
        if self.module_globals.is_none() {
            self.module_globals = Some(Arc::new(RwLock::new(HashMap::new())));
        }
        let top_fn = Arc::new(CompiledFunction::new(0, chunk, None, Vec::new(), false));
        let top_closure = Arc::new(Closure {
            function: top_fn,
            upvalues: Vec::new(),
            module_globals: self.module_globals.clone(),
        });
        self.frames.clear();
        self.open_upvalues.clear();
        self.frames.push(CallFrame {
            closure: top_closure,
            ip: 0,
            slots_offset: 0,
            try_handlers: Vec::new(),
        });
        if let Some(prof) = &mut self.profiler {
            prof.enter_function("<main>");
        }
        self.run()
    }

    pub fn execute_closure(&mut self, closure: Arc<Closure>, args: Vec<Value>) -> InterpretResult {
        if args.len() != closure.function.arity {
            return InterpretResult::RuntimeError(format!(
                "Expected {} arguments but got {}.",
                closure.function.arity,
                args.len()
            ));
        }
        self.stack.clear();
        self.frames.clear();
        self.open_upvalues.clear();
        self.module_globals = closure.module_globals.clone();
        self.stack.push(Value::Closure(closure.clone()));
        for arg in args {
            self.stack.push(arg);
        }
        if let Some(prof) = &mut self.profiler {
            let name = closure.function.name.as_deref().unwrap_or("<anonymous>");
            prof.enter_function(name);
        }
        self.frames.push(CallFrame {
            closure,
            ip: 0,
            slots_offset: 0,
            try_handlers: Vec::new(),
        });
        self.run()
    }

    fn capture_upvalue(&mut self, slot: usize) -> Arc<RwLock<Upvalue>> {
        for uv in &self.open_upvalues {
            if let UpvalueLocation::Open(s) = uv.read().unwrap().location {
                if s == slot {
                    return uv.clone();
                }
            }
        }
        let created = Arc::new(RwLock::new(Upvalue::new(slot)));
        self.open_upvalues.push(created.clone());
        created
    }

    fn close_upvalues(&mut self, last_slot: usize) {
        for upvalue in &self.open_upvalues {
            let mut uv = upvalue.write().unwrap();
            if let UpvalueLocation::Open(slot) = uv.location {
                if slot >= last_slot {
                    uv.location = UpvalueLocation::Closed(self.stack[slot].clone());
                }
            }
        }
        self.open_upvalues
            .retain(|uv| matches!(uv.read().unwrap().location, UpvalueLocation::Open(_)));
    }

    fn close_upvalues_in_value(&mut self, val: &Value) {
        match val {
            Value::Closure(c) => {
                for uv in &c.upvalues {
                    let mut guard = uv.write().unwrap();
                    let captured = if let UpvalueLocation::Open(slot) = guard.location {
                        if slot < self.stack.len() {
                            let v = self.stack[slot].clone();
                            guard.location = UpvalueLocation::Closed(v.clone());
                            Some(v)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    drop(guard);
                    if let Some(v) = captured {
                        self.close_upvalues_in_value(&v);
                    }
                }
            }
            Value::Array(arr) => {
                let items = arr.read().unwrap().clone();
                for item in &items {
                    self.close_upvalues_in_value(item);
                }
            }
            Value::Map(map) => {
                let values: Vec<Value> = map.read().unwrap().values().cloned().collect();
                for item in &values {
                    self.close_upvalues_in_value(item);
                }
            }
            Value::StructInstance { fields, .. } => {
                let values: Vec<Value> = fields.read().unwrap().values().cloned().collect();
                for item in &values {
                    self.close_upvalues_in_value(item);
                }
            }
            Value::BoundMethod { object, .. } => {
                let rec = object.as_ref().clone();
                self.close_upvalues_in_value(&rec);
            }
            Value::EnumInstance { values, .. } => {
                let vals = values.clone();
                for item in &vals {
                    self.close_upvalues_in_value(item);
                }
            }
            _ => {}
        }
    }

    fn handle_runtime_error(&mut self, msg: String) -> Option<InterpretResult> {
        while let Some(mut frame) = self.frames.pop() {
            if let Some(handler) = frame.try_handlers.pop() {
                frame.ip = handler.catch_ip;
                self.close_upvalues(handler.stack_len);
                self.stack.truncate(handler.stack_len);
                self.stack.push(Value::String(msg));
                self.frames.push(frame);
                return None;
            }
            self.close_upvalues(frame.slots_offset);
            self.stack.truncate(frame.slots_offset);
        }
        Some(InterpretResult::RuntimeError(msg))
    }

    fn run(&mut self) -> InterpretResult {
        macro_rules! runtime_error {
            ($self:ident, $msg:expr $(,)?) => {
                if let Some(err) = $self.handle_runtime_error($msg.to_string()) {
                    return err;
                } else {
                    continue;
                }
            };
            ($msg:expr $(,)?) => {
                if let Some(err) = self.handle_runtime_error($msg.to_string()) {
                    return err;
                } else {
                    continue;
                }
            };
        }

        loop {
            if self.frames.is_empty() {
                return InterpretResult::Ok(Value::Null);
            }
            if self.frames.last().unwrap().ip
                >= self
                    .frames
                    .last()
                    .unwrap()
                    .closure
                    .function
                    .chunk
                    .code
                    .len()
            {
                let frame = self.frames.pop().unwrap();
                if let Some(prof) = &mut self.profiler {
                    let name = frame
                        .closure
                        .function
                        .name
                        .as_deref()
                        .unwrap_or("<anonymous>");
                    prof.exit_function(name);
                }
                self.close_upvalues(frame.slots_offset);
                if self.frames.is_empty() {
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    return InterpretResult::Ok(val);
                }
                let val = self.stack.pop().unwrap_or(Value::Null);
                self.stack.truncate(frame.slots_offset);
                self.stack.push(val);
                continue;
            }

            let frame = self.frames.last().unwrap();
            let current_ip = frame.ip;
            let current_line = if current_ip < frame.closure.function.chunk.lines.len() {
                frame.closure.function.chunk.lines[current_ip]
            } else {
                0
            };

            if let Some(dbg) = &self.debug_session {
                let file_str = self
                    .current_file
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<script>".to_string());
                let should_pause = {
                    let mut session = dbg.lock().unwrap();
                    session.should_stop(&file_str, current_line, self.frames.len())
                };
                if let Some(reason) = should_pause {
                    let _ = reason;
                }
            }

            let instruction: OpCode = self.read_byte().into();

            if let Some(prof) = &mut self.profiler {
                prof.on_opcode(instruction);
            }

            match instruction {
                OpCode::Return => {
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    let frame = self.frames.pop().unwrap();
                    if let Some(prof) = &mut self.profiler {
                        let name = frame
                            .closure
                            .function
                            .name
                            .as_deref()
                            .unwrap_or("<anonymous>");
                        prof.exit_function(name);
                    }
                    self.close_upvalues(frame.slots_offset);
                    if self.frames.is_empty() {
                        return InterpretResult::Ok(val);
                    }
                    self.stack.truncate(frame.slots_offset);
                    self.stack.push(val);
                }
                OpCode::Constant => {
                    let constant = self.read_constant();
                    self.stack.push(constant);
                }
                OpCode::ConstantLong => {
                    let constant = self.read_constant_long();
                    self.stack.push(constant);
                }
                OpCode::Nil => {
                    self.stack.push(Value::Null);
                }
                OpCode::True => {
                    self.stack.push(Value::Bool(true));
                }
                OpCode::False => {
                    self.stack.push(Value::Bool(false));
                }
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::GetLocal => {
                    let slot_idx = self.read_byte() as usize;
                    let slot = self.frames.last().unwrap().slots_offset + slot_idx;
                    if slot < self.stack.len() {
                        self.stack.push(self.stack[slot].clone());
                    } else {
                        runtime_error!(self, format!("Stack underflow on local slot {}", slot_idx));
                    }
                }
                OpCode::GetLocalLong => {
                    let slot_idx = self.read_short() as usize;
                    let slot = self.frames.last().unwrap().slots_offset + slot_idx;
                    if slot < self.stack.len() {
                        self.stack.push(self.stack[slot].clone());
                    } else {
                        runtime_error!(self, format!("Stack underflow on local slot {}", slot_idx));
                    }
                }
                OpCode::SetLocal => {
                    let slot_idx = self.read_byte() as usize;
                    let slot = self.frames.last().unwrap().slots_offset + slot_idx;
                    if let Some(val) = self.stack.last().cloned() {
                        if slot < self.stack.len() {
                            self.stack[slot] = val;
                        } else {
                            runtime_error!(
                                self,
                                format!("Invalid local slot {} for assignment", slot_idx)
                            );
                        }
                    } else {
                        runtime_error!(self, "Stack empty on SetLocal");
                    }
                }
                OpCode::SetLocalLong => {
                    let slot_idx = self.read_short() as usize;
                    let slot = self.frames.last().unwrap().slots_offset + slot_idx;
                    if let Some(val) = self.stack.last().cloned() {
                        if slot < self.stack.len() {
                            self.stack[slot] = val;
                        } else {
                            runtime_error!(
                                self,
                                format!("Invalid local slot {} for assignment", slot_idx)
                            );
                        }
                    } else {
                        runtime_error!(self, "Stack empty on SetLocalLong");
                    }
                }
                OpCode::DefineGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    if let Some(frame) = self.frames.last() {
                        if let Some(mg) = &frame.closure.module_globals {
                            mg.write().unwrap().insert(name.clone(), val.clone());
                        }
                    } else if let Some(mg) = &self.module_globals {
                        mg.write().unwrap().insert(name.clone(), val.clone());
                    }
                    self.globals.insert(name, val);
                }
                OpCode::GetGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let val_opt = self
                        .frames
                        .last()
                        .and_then(|f| f.closure.module_globals.as_ref())
                        .and_then(|mg| mg.read().unwrap().get(&name).cloned())
                        .or_else(|| self.globals.get(&name).cloned());

                    if let Some(val) = val_opt {
                        self.stack.push(val);
                    } else {
                        runtime_error!(self, format!("Undefined variable '{}'.", name));
                    }
                }
                OpCode::SetGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    if let Some(val) = self.stack.last().cloned() {
                        let mut found = false;
                        if let Some(frame) = self.frames.last() {
                            if let Some(mg) = &frame.closure.module_globals {
                                if mg.read().unwrap().contains_key(&name) {
                                    mg.write().unwrap().insert(name.clone(), val.clone());
                                    found = true;
                                }
                            }
                        }
                        if self.globals.contains_key(&name) {
                            self.globals.insert(name.clone(), val);
                            found = true;
                        }
                        if !found {
                            runtime_error!(self, format!("Undefined variable '{}'.", name));
                        }
                    } else {
                        runtime_error!(self, "Stack empty on SetGlobal");
                    }
                }
                OpCode::Equal => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(Value::Bool(a == b));
                }
                OpCode::Greater => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x > y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x > y)),
                        (Value::Int(x), Value::Float(y)) => {
                            self.stack.push(Value::Bool((x as f64) > y))
                        }
                        (Value::Float(x), Value::Int(y)) => {
                            self.stack.push(Value::Bool(x > (y as f64)))
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers for comparison.",);
                        }
                    }
                }
                OpCode::Less => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x < y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x < y)),
                        (Value::Int(x), Value::Float(y)) => {
                            self.stack.push(Value::Bool((x as f64) < y))
                        }
                        (Value::Float(x), Value::Int(y)) => {
                            self.stack.push(Value::Bool(x < (y as f64)))
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers for comparison.",);
                        }
                    }
                }
                OpCode::Not => {
                    let val = self.stack.pop().unwrap();
                    self.stack.push(Value::Bool(!val.is_truthy()));
                }
                OpCode::Negate => match self.stack.pop() {
                    Some(Value::Int(n)) => self.stack.push(Value::Int(-n)),
                    Some(Value::Float(n)) | Some(Value::Number(n)) => {
                        self.stack.push(Value::Float(-n))
                    }
                    _ => runtime_error!(self, "Operand must be a number."),
                },
                OpCode::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Int(a_num + b_num));
                        }
                        (Value::Int(a_num), Value::Float(b_num))
                        | (Value::Int(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Float(a_num as f64 + b_num));
                        }
                        (Value::Float(a_num), Value::Int(b_num))
                        | (Value::Number(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Float(a_num + b_num as f64));
                        }
                        (Value::Float(a_num), Value::Float(b_num))
                        | (Value::Number(a_num), Value::Number(b_num))
                        | (Value::Float(a_num), Value::Number(b_num))
                        | (Value::Number(a_num), Value::Float(b_num)) => {
                            self.stack.push(Value::Float(a_num + b_num));
                        }
                        (Value::String(a_str), Value::String(b_str)) => {
                            self.stack
                                .push(Value::String(format!("{}{}", a_str, b_str)));
                        }
                        _ => {
                            runtime_error!(self, "Operands must be two numbers or two strings.",);
                        }
                    }
                }
                OpCode::Subtract => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Int(a_num - b_num))
                        }
                        (Value::Int(a_num), Value::Float(b_num))
                        | (Value::Int(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Float(a_num as f64 - b_num))
                        }
                        (Value::Float(a_num), Value::Int(b_num))
                        | (Value::Number(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Float(a_num - b_num as f64))
                        }
                        (Value::Float(a_num), Value::Float(b_num))
                        | (Value::Number(a_num), Value::Number(b_num))
                        | (Value::Float(a_num), Value::Number(b_num))
                        | (Value::Number(a_num), Value::Float(b_num)) => {
                            self.stack.push(Value::Float(a_num - b_num))
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers.",);
                        }
                    }
                }
                OpCode::Multiply => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Int(a_num * b_num))
                        }
                        (Value::Int(a_num), Value::Float(b_num))
                        | (Value::Int(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Float(a_num as f64 * b_num))
                        }
                        (Value::Float(a_num), Value::Int(b_num))
                        | (Value::Number(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Float(a_num * b_num as f64))
                        }
                        (Value::Float(a_num), Value::Float(b_num))
                        | (Value::Number(a_num), Value::Number(b_num))
                        | (Value::Float(a_num), Value::Number(b_num))
                        | (Value::Number(a_num), Value::Float(b_num)) => {
                            self.stack.push(Value::Float(a_num * b_num))
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers.",);
                        }
                    }
                }
                OpCode::Divide => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(a_num), Value::Int(b_num)) => {
                            if b_num == 0 {
                                runtime_error!(self, "Divide by zero.");
                            }
                            if a_num % b_num == 0 {
                                self.stack.push(Value::Int(a_num / b_num));
                            } else {
                                self.stack.push(Value::Float(a_num as f64 / b_num as f64));
                            }
                        }
                        (Value::Int(a_num), Value::Float(b_num))
                        | (Value::Int(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Float(a_num as f64 / b_num));
                        }
                        (Value::Float(a_num), Value::Int(b_num))
                        | (Value::Number(a_num), Value::Int(b_num)) => {
                            self.stack.push(Value::Float(a_num / b_num as f64));
                        }
                        (Value::Float(a_num), Value::Float(b_num))
                        | (Value::Number(a_num), Value::Number(b_num))
                        | (Value::Float(a_num), Value::Number(b_num))
                        | (Value::Number(a_num), Value::Float(b_num)) => {
                            self.stack.push(Value::Float(a_num / b_num));
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers.",);
                        }
                    }
                }
                OpCode::Mod => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            if y == 0 {
                                runtime_error!(self, "Modulo by zero.");
                            }
                            self.stack.push(Value::Int(x % y));
                        }
                        (Value::Float(x), Value::Float(y)) => {
                            if y == 0.0 {
                                runtime_error!(self, "Modulo by zero.");
                            }
                            self.stack.push(Value::Float(x % y));
                        }
                        _ => {
                            runtime_error!(self, "Operands must be numbers for modulo.",);
                        }
                    }
                }
                OpCode::BitAnd => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x & y)),
                        _ => {
                            runtime_error!(self, "Operands must be integers for bitwise AND.",);
                        }
                    }
                }
                OpCode::BitOr => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x | y)),
                        _ => {
                            runtime_error!(self, "Operands must be integers for bitwise OR.",);
                        }
                    }
                }
                OpCode::BitXor => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x ^ y)),
                        _ => {
                            runtime_error!(self, "Operands must be integers for bitwise XOR.",);
                        }
                    }
                }
                OpCode::BitNot => {
                    let a = self.stack.pop().unwrap();
                    match a {
                        Value::Int(x) => self.stack.push(Value::Int(!x)),
                        _ => {
                            runtime_error!(self, "Operand must be an integer for bitwise NOT.",);
                        }
                    }
                }
                OpCode::Shl => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x << y)),
                        _ => {
                            runtime_error!(self, "Operands must be integers for shift left.",);
                        }
                    }
                }
                OpCode::Shr => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x >> y)),
                        _ => {
                            runtime_error!(self, "Operands must be integers for shift right.",);
                        }
                    }
                }
                OpCode::Jump => {
                    let offset = self.read_short();
                    self.frames.last_mut().unwrap().ip += offset as usize;
                }
                OpCode::JumpIfFalse => {
                    let offset = self.read_short();
                    let condition = self.stack.last().map(|v| v.is_truthy()).unwrap_or(false);
                    if !condition {
                        self.frames.last_mut().unwrap().ip += offset as usize;
                    }
                }
                OpCode::Loop => {
                    let offset = self.read_short();
                    self.frames.last_mut().unwrap().ip -= offset as usize;
                }
                OpCode::BuildList => {
                    let count = self.read_byte() as usize;
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.stack.pop().unwrap());
                    }
                    elements.reverse();
                    self.stack
                        .push(Value::Array(Arc::new(RwLock::new(elements))));
                }
                OpCode::BuildMap => {
                    let count = self.read_byte() as usize;
                    let mut map = indexmap::IndexMap::with_capacity(count);
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let val = self.stack.pop().unwrap();
                        let key = match self.stack.pop().unwrap() {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        pairs.push((key, val));
                    }
                    pairs.reverse();
                    for (k, v) in pairs {
                        map.insert(k, v);
                    }
                    self.stack.push(Value::Map(Arc::new(RwLock::new(map))));
                }
                OpCode::IndexGet => {
                    let index = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::GcArray(r) => {
                            let len = self.heap.as_array(r).map(|a| a.len()).unwrap_or(0);
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, len),
                                Value::Float(f) | Value::Number(f) => resolve_index(f, len),
                                _ => {
                                    runtime_error!(self, "Array index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => {
                                    let val = self.heap.as_array(r).unwrap()[idx].clone();
                                    self.stack.push(val);
                                }
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "Array index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!("Index out of bounds for array of length {}", len)
                                    );
                                }
                            }
                        }
                        Value::Array(arr) => {
                            let borrow = arr.read().unwrap();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, borrow.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, borrow.len())
                                }
                                _ => {
                                    runtime_error!(self, "Array index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => self.stack.push(borrow[idx].clone()),
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "Array index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!("Index out of bounds for array of length {}", len)
                                    );
                                }
                            }
                        }
                        Value::String(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, chars.len()),
                                Value::Float(f) | Value::Number(f) => resolve_index(f, chars.len()),
                                _ => {
                                    runtime_error!(self, "String index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => self.stack.push(Value::String(chars[idx].to_string())),
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "String index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!("Index out of bounds for string of length {}", len)
                                    );
                                }
                            }
                        }
                        Value::GcMap(r) => {
                            let key = match index {
                                Value::String(s) => s,
                                Value::GcString(sr) => {
                                    self.heap.as_string(sr).unwrap_or("").to_string()
                                }
                                other => other.to_string(),
                            };
                            let val = self
                                .heap
                                .as_map(r)
                                .and_then(|m| m.get(&key))
                                .cloned()
                                .unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        Value::Map(m) => {
                            if let Value::String(key) = index {
                                let borrow = m.read().unwrap();
                                let val = borrow.get(&key).cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                runtime_error!(self, "Map key must be a string.",);
                            }
                        }
                        Value::EnumInstance { ref values, .. } => {
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, values.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, values.len())
                                }
                                _ => {
                                    runtime_error!(self, "Enum index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => self.stack.push(values[idx].clone()),
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "Enum index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!(
                                            "Index out of bounds for enum variant of length {}",
                                            len
                                        )
                                    );
                                }
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("Cannot index into {}.", other.type_name())
                            );
                        }
                    }
                }
                OpCode::IndexSet => {
                    let value = self.stack.pop().unwrap();
                    let index = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::GcArray(r) => {
                            let len = self.heap.as_array(r).map(|a| a.len()).unwrap_or(0);
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, len),
                                Value::Float(f) | Value::Number(f) => resolve_index(f, len),
                                _ => {
                                    runtime_error!(self, "Array index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => {
                                    self.heap.as_array_mut(r).unwrap()[idx] = value.clone();
                                    self.heap.write_barrier(r, &value);
                                    self.stack.push(value);
                                }
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "Array index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!("Index out of bounds for array of length {}", len)
                                    );
                                }
                            }
                        }
                        Value::Array(arr) => {
                            let mut borrow = arr.write().unwrap();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, borrow.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, borrow.len())
                                }
                                _ => {
                                    runtime_error!(self, "Array index must be a number.",);
                                }
                            };
                            match idx_res {
                                Ok(idx) => {
                                    borrow[idx] = value.clone();
                                    self.stack.push(value);
                                }
                                Err(IndexError::NotWhole) => {
                                    runtime_error!(self, "Array index must be a whole number.",);
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    runtime_error!(
                                        self,
                                        format!("Index out of bounds for array of length {}", len)
                                    );
                                }
                            }
                        }
                        Value::GcMap(r) => {
                            let key = match index {
                                Value::String(s) => s,
                                Value::GcString(sr) => {
                                    self.heap.as_string(sr).unwrap_or("").to_string()
                                }
                                other => other.to_string(),
                            };
                            self.heap.as_map_mut(r).unwrap().insert(key, value.clone());
                            self.heap.write_barrier(r, &value);
                            self.stack.push(value);
                        }
                        Value::Map(m) => {
                            if let Value::String(key) = index {
                                m.write().unwrap().insert(key, value.clone());
                                self.stack.push(value);
                            } else {
                                runtime_error!(self, "Map key must be a string.",);
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("Cannot index into {}.", other.type_name())
                            );
                        }
                    }
                }
                OpCode::GetProperty => {
                    let callsite = (
                        Arc::as_ptr(&self.frames.last().unwrap().closure.function) as usize,
                        self.frames.last().unwrap().ip - 1,
                    );
                    let name_val = self.read_constant();
                    let is_safe = self.read_byte() != 0;
                    let property = match name_val {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::Null => {
                            if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(self, "Cannot read property of null",);
                            }
                        }
                        Value::Map(ref m) => {
                            let map = m.read().unwrap();
                            if let Some(entry) = self.prop_get_ic.get(&callsite) {
                                if let Some((k, v)) = map.get_index(entry.index) {
                                    if k == &property {
                                        self.stack.push(v.clone());
                                        continue;
                                    }
                                }
                            }
                            if let Some(idx) = map.get_index_of(&property) {
                                let v = map.get_index(idx).unwrap().1.clone();
                                self.prop_get_ic.insert(
                                    callsite,
                                    InlineCacheEntry {
                                        property: property.clone(),
                                        index: idx,
                                    },
                                );
                                self.stack.push(v);
                            } else if property == "len" {
                                self.stack.push(Value::Int(map.len() as i64));
                            } else if matches!(
                                property.as_str(),
                                "keys" | "values" | "has" | "contains" | "get" | "delete"
                            ) || (map.contains_key("_sqlite_handle")
                                && matches!(
                                    property.as_str(),
                                    "execute" | "query" | "close" | "changes" | "last_insert_id"
                                ))
                                || (map.contains_key("_is_set")
                                    && matches!(
                                        property.as_str(),
                                        "add"
                                            | "has"
                                            | "contains"
                                            | "delete"
                                            | "size"
                                            | "to_array"
                                            | "clear"
                                            | "union"
                                            | "intersect"
                                            | "difference"
                                    ))
                            {
                                self.stack.push(Value::BoundMethod {
                                    object: Box::new(target.clone()),
                                    method: property,
                                });
                            } else if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Property '{}' not found in map", property)
                                );
                            }
                        }
                        Value::GcMap(r) => {
                            let val = self.heap.as_map(r).and_then(|m| m.get(&property).cloned());
                            if let Some(v) = val {
                                self.stack.push(v);
                            } else if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Property '{}' not found in map", property)
                                );
                            }
                        }
                        Value::StructInstance {
                            ref name,
                            ref fields,
                        } => {
                            let fields_read = fields.read().unwrap();
                            if let Some(entry) = self.prop_get_ic.get(&callsite) {
                                if let Some((k, v)) = fields_read.get_index(entry.index) {
                                    if k == &property {
                                        self.stack.push(v.clone());
                                        continue;
                                    }
                                }
                            }
                            if let Some(idx) = fields_read.get_index_of(&property) {
                                let v = fields_read.get_index(idx).unwrap().1.clone();
                                self.prop_get_ic.insert(
                                    callsite,
                                    InlineCacheEntry {
                                        property: property.clone(),
                                        index: idx,
                                    },
                                );
                                self.stack.push(v);
                            } else if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Struct '{}' has no field '{}'", name, property)
                                );
                            }
                        }
                        Value::EnumDef {
                            ref name,
                            ref variants,
                        } => {
                            if let Some(params) = variants.get(&property) {
                                self.stack.push(Value::EnumConstructor {
                                    enum_name: name.clone(),
                                    variant_name: property,
                                    params: params.clone(),
                                });
                            } else if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Enum '{}' has no variant '{}'", name, property)
                                );
                            }
                        }
                        Value::Array(ref a) => {
                            if property == "len" {
                                self.stack.push(Value::Int(a.read().unwrap().len() as i64));
                            } else if property == "first" {
                                let val = a.read().unwrap().first().cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else if property == "last" {
                                let val = a.read().unwrap().last().cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else if matches!(
                                property.as_str(),
                                "push"
                                    | "pop"
                                    | "map"
                                    | "filter"
                                    | "reduce"
                                    | "sum"
                                    | "sort"
                                    | "find"
                                    | "some"
                                    | "every"
                                    | "flat"
                                    | "join"
                                    | "reverse"
                                    | "slice"
                            ) {
                                self.stack.push(Value::BoundMethod {
                                    object: Box::new(target.clone()),
                                    method: property,
                                });
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Unknown array property '{}'", property)
                                );
                            }
                        }
                        Value::GcArray(r) => {
                            let len = self.heap.as_array(r).map(|a| a.len()).unwrap_or(0);
                            if property == "len" {
                                self.stack.push(Value::Int(len as i64));
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Unknown array property '{}'", property)
                                );
                            }
                        }
                        Value::String(ref s) => {
                            if property == "len" {
                                self.stack.push(Value::Int(s.chars().count() as i64));
                            } else if matches!(
                                property.as_str(),
                                "trim"
                                    | "upper"
                                    | "lower"
                                    | "toUpper"
                                    | "toLower"
                                    | "split"
                                    | "replace"
                                    | "starts_with"
                                    | "ends_with"
                                    | "startsWith"
                                    | "endsWith"
                                    | "contains"
                                    | "indexOf"
                                    | "slice"
                                    | "chars"
                                    | "lines"
                                    | "graphemes"
                                    | "grapheme_len"
                                    | "graphemeLen"
                                    | "byte_len"
                                    | "byteLen"
                                    | "runes"
                                    | "codepoints"
                                    | "is_ascii"
                                    | "isAscii"
                                    | "slice_graphemes"
                                    | "sliceGraphemes"
                            ) {
                                self.stack.push(Value::BoundMethod {
                                    object: Box::new(target.clone()),
                                    method: property,
                                });
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Unknown string property '{}'", property)
                                );
                            }
                        }
                        Value::FfiLibrary(ref lib) => {
                            if matches!(property.as_str(), "fn" | "symbol" | "get" | "close") {
                                self.stack.push(Value::BoundMethod {
                                    object: Box::new(target.clone()),
                                    method: property,
                                });
                            } else if property == "path" {
                                self.stack.push(Value::String(lib.path.clone()));
                            } else if is_safe {
                                self.stack.push(Value::Null);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Unknown FFI library property '{}'", property)
                                );
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!(
                                    "Cannot read property '{}' of {}",
                                    property,
                                    other.type_name()
                                )
                            );
                        }
                    }
                }
                OpCode::SetProperty => {
                    let callsite = (
                        Arc::as_ptr(&self.frames.last().unwrap().closure.function) as usize,
                        self.frames.last().unwrap().ip - 1,
                    );
                    let name_val = self.read_constant();
                    let property = match name_val {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let value = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::Map(ref m) => {
                            let mut map = m.write().unwrap();
                            if let Some(entry) = self.prop_set_ic.get(&callsite) {
                                if let Some((k, v_mut)) = map.get_index_mut(entry.index) {
                                    if k == &property {
                                        *v_mut = value.clone();
                                        self.stack.push(value);
                                        continue;
                                    }
                                }
                            }
                            let idx = match map.get_index_of(&property) {
                                Some(i) => {
                                    *map.get_index_mut(i).unwrap().1 = value.clone();
                                    i
                                }
                                None => {
                                    map.insert(property.clone(), value.clone());
                                    map.len() - 1
                                }
                            };
                            self.prop_set_ic.insert(
                                callsite,
                                InlineCacheEntry {
                                    property: property.clone(),
                                    index: idx,
                                },
                            );
                            self.stack.push(value);
                        }
                        Value::GcMap(r) => {
                            if let Some(map) = self.heap.as_map_mut(r) {
                                map.insert(property, value.clone());
                                self.heap.write_barrier(r, &value);
                            }
                            self.stack.push(value);
                        }
                        Value::StructInstance {
                            ref name,
                            ref fields,
                        } => {
                            let mut fields_write = fields.write().unwrap();
                            if let Some(entry) = self.prop_set_ic.get(&callsite) {
                                if let Some((k, v_mut)) = fields_write.get_index_mut(entry.index) {
                                    if k == &property {
                                        *v_mut = value.clone();
                                        self.stack.push(value);
                                        continue;
                                    }
                                }
                            }
                            if let Some(idx) = fields_write.get_index_of(&property) {
                                *fields_write.get_index_mut(idx).unwrap().1 = value.clone();
                                self.prop_set_ic.insert(
                                    callsite,
                                    InlineCacheEntry {
                                        property: property.clone(),
                                        index: idx,
                                    },
                                );
                                self.stack.push(value);
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Struct '{}' has no field '{}'", name, property)
                                );
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!(
                                    "Cannot set property '{}' on {}",
                                    property,
                                    other.type_name()
                                )
                            );
                        }
                    }
                }
                OpCode::FormatString => {
                    let count = self.read_byte() as usize;
                    let mut parts = Vec::with_capacity(count);
                    for _ in 0..count {
                        parts.push(self.stack.pop().unwrap());
                    }
                    parts.reverse();
                    let mut res = String::new();
                    for part in parts {
                        match part {
                            Value::String(s) => res.push_str(&s),
                            other => res.push_str(&other.to_display()),
                        }
                    }
                    self.stack.push(Value::String(res));
                }
                OpCode::IndexGetSafe => {
                    let index = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::GcArray(r) => {
                            let len = self.heap.as_array(r).map(|a| a.len()).unwrap_or(0);
                            let idx_opt = match index {
                                Value::Int(i) if i >= 0 && (i as usize) < len => Some(i as usize),
                                Value::Float(f) | Value::Number(f)
                                    if f >= 0.0 && (f as usize) < len =>
                                {
                                    Some(f as usize)
                                }
                                _ => None,
                            };
                            match idx_opt {
                                Some(idx) => {
                                    let val = self.heap.as_array(r).unwrap()[idx].clone();
                                    self.stack.push(val);
                                }
                                None => self.stack.push(Value::Null),
                            }
                        }
                        Value::Array(ref arr) => {
                            let borrow = arr.read().unwrap();
                            let len = borrow.len();
                            let idx_opt = match index {
                                Value::Int(i) if i >= 0 && (i as usize) < len => Some(i as usize),
                                Value::Float(f) | Value::Number(f)
                                    if f >= 0.0 && (f as usize) < len =>
                                {
                                    Some(f as usize)
                                }
                                _ => None,
                            };
                            match idx_opt {
                                Some(idx) => self.stack.push(borrow[idx].clone()),
                                None => self.stack.push(Value::Null),
                            }
                        }
                        Value::EnumInstance { ref values, .. } => {
                            let idx_opt = match index {
                                Value::Int(i) if i >= 0 && (i as usize) < values.len() => {
                                    Some(i as usize)
                                }
                                Value::Float(f) | Value::Number(f)
                                    if f >= 0.0 && (f as usize) < values.len() =>
                                {
                                    Some(f as usize)
                                }
                                _ => None,
                            };
                            match idx_opt {
                                Some(idx) => self.stack.push(values[idx].clone()),
                                None => self.stack.push(Value::Null),
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!(
                                    "Cannot destructure non-array {} as array",
                                    other.type_name()
                                )
                            );
                        }
                    }
                }
                OpCode::ArraySlice => {
                    let start_val = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    let start_idx = match start_val {
                        Value::Int(i) if i >= 0 => i as usize,
                        Value::Float(f) | Value::Number(f) if f >= 0.0 => f as usize,
                        _ => 0,
                    };
                    match target {
                        Value::Array(ref a) => {
                            let borrow = a.read().unwrap();
                            let slice_vals = if start_idx < borrow.len() {
                                borrow[start_idx..].to_vec()
                            } else {
                                Vec::new()
                            };
                            self.stack
                                .push(Value::Array(Arc::new(RwLock::new(slice_vals))));
                        }
                        Value::GcArray(r) => {
                            let borrow = self.heap.as_array(r).cloned().unwrap_or_default();
                            let slice_vals = if start_idx < borrow.len() {
                                borrow[start_idx..].to_vec()
                            } else {
                                Vec::new()
                            };
                            self.stack
                                .push(Value::Array(Arc::new(RwLock::new(slice_vals))));
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("Cannot slice non-array {}", other.type_name())
                            );
                        }
                    }
                }
                OpCode::MapRest => {
                    let count = self.read_byte() as usize;
                    let mut extracted_keys = std::collections::HashSet::with_capacity(count);
                    for _ in 0..count {
                        let k_val = self.stack.pop().unwrap();
                        match k_val {
                            Value::String(s) => {
                                extracted_keys.insert(s);
                            }
                            other => {
                                extracted_keys.insert(other.to_string());
                            }
                        }
                    }
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::Map(ref m) => {
                            let borrow = m.read().unwrap();
                            let mut rest_map = indexmap::IndexMap::new();
                            for (k, v) in borrow.iter() {
                                if !extracted_keys.contains(k) {
                                    rest_map.insert(k.clone(), v.clone());
                                }
                            }
                            self.stack.push(Value::Map(Arc::new(RwLock::new(rest_map))));
                        }
                        Value::GcMap(r) => {
                            let borrow = self.heap.as_map(r).cloned().unwrap_or_default();
                            let mut rest_map = indexmap::IndexMap::new();
                            for (k, v) in borrow.iter() {
                                if !extracted_keys.contains(k) {
                                    rest_map.insert(k.clone(), v.clone());
                                }
                            }
                            self.stack.push(Value::Map(Arc::new(RwLock::new(rest_map))));
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!(
                                    "Cannot destructure non-map {} with rest",
                                    other.type_name()
                                )
                            );
                        }
                    }
                }
                OpCode::BuildStruct => {
                    let field_count = self.read_byte() as usize;
                    let mut provided_fields = indexmap::IndexMap::new();
                    let mut provided_names = std::collections::HashSet::new();
                    for _ in 0..field_count {
                        let val = self.stack.pop().unwrap();
                        let name_val = self.stack.pop().unwrap();
                        let name_str = match name_val {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        provided_names.insert(name_str.clone());
                        provided_fields.insert(name_str, val);
                    }
                    let def_val = self.stack.pop().unwrap();
                    match def_val {
                        Value::StructDef {
                            name,
                            fields: def_fields,
                        } => {
                            if field_count != def_fields.len() {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Struct '{}' expects {} fields, but got {}.",
                                        name,
                                        def_fields.len(),
                                        field_count
                                    )
                                );
                            }
                            for (f_name, _) in provided_fields.iter() {
                                if !def_fields.contains(f_name) {
                                    runtime_error!(
                                        self,
                                        format!("Struct '{}' has no field '{}'.", name, f_name)
                                    );
                                }
                            }
                            if provided_names.len() != def_fields.len() {
                                runtime_error!(
                                    self,
                                    format!("Struct '{}' initialization is missing fields.", name)
                                );
                            }
                            let mut ordered_fields = indexmap::IndexMap::new();
                            for f in &def_fields {
                                if let Some(v) = provided_fields.shift_remove(f) {
                                    ordered_fields.insert(f.clone(), v);
                                }
                            }
                            self.stack.push(Value::StructInstance {
                                name,
                                fields: Arc::new(RwLock::new(ordered_fields)),
                            });
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("'{}' is not a struct.", other.type_name())
                            );
                        }
                    }
                }
                OpCode::MatchEnum => {
                    let field_count = self.read_byte() as usize;
                    let variant_const = self.read_constant();
                    let variant_name = match variant_const {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let enum_const = self.read_constant();
                    let enum_name_opt = match enum_const {
                        Value::String(s) => Some(s),
                        _ => None,
                    };
                    let target = self.stack.pop().unwrap();
                    let matched = match target {
                        Value::EnumInstance {
                            enum_name: ref v_enum,
                            variant_name: ref v_variant,
                            ref values,
                        } => {
                            let name_ok = match &enum_name_opt {
                                Some(e) => e == v_enum && &variant_name == v_variant,
                                None => &variant_name == v_variant,
                            };
                            name_ok && values.len() == field_count
                        }
                        _ => false,
                    };
                    self.stack.push(Value::Bool(matched));
                }
                OpCode::MatchRange => {
                    let inclusive = self.read_byte() != 0;
                    let end = self.stack.pop().unwrap();
                    let start = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    let matched = match (&target, &start, &end) {
                        (Value::Int(v), Value::Int(s), Value::Int(e)) => {
                            if inclusive {
                                *v >= *s && *v <= *e
                            } else {
                                *v >= *s && *v < *e
                            }
                        }
                        (Value::String(v), Value::String(s), Value::String(e)) => {
                            if inclusive {
                                v >= s && v <= e
                            } else {
                                v >= s && v < e
                            }
                        }
                        _ => {
                            let v_num = to_f64_val(&target);
                            let s_num = to_f64_val(&start);
                            let e_num = to_f64_val(&end);
                            if let (Some(v), Some(s), Some(e)) = (v_num, s_num, e_num) {
                                if inclusive {
                                    v >= s && v <= e
                                } else {
                                    v >= s && v < e
                                }
                            } else {
                                false
                            }
                        }
                    };
                    self.stack.push(Value::Bool(matched));
                }
                OpCode::MatchError => {
                    runtime_error!(self, "Non-exhaustive match. No pattern matched the value.");
                }
                OpCode::PushTry => {
                    let offset = self.read_short();
                    let catch_ip = self.frames.last().unwrap().ip + offset as usize;
                    let stack_len = self.stack.len();
                    self.frames
                        .last_mut()
                        .unwrap()
                        .try_handlers
                        .push(TryHandler {
                            catch_ip,
                            stack_len,
                        });
                }
                OpCode::PopTry => {
                    self.frames.last_mut().unwrap().try_handlers.pop();
                }
                OpCode::Dup => {
                    let top = match self.stack.last().cloned() {
                        Some(v) => v,
                        None => {
                            runtime_error!(self, "Stack underflow on Dup".to_string());
                        }
                    };
                    self.stack.push(top);
                }
                OpCode::ImportModule => {
                    let path_val = match self.stack.pop() {
                        Some(v) => v,
                        None => {
                            runtime_error!(self, "Stack underflow on ImportModule".to_string());
                        }
                    };
                    let path_str = match path_val {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let mut evaluator = crate::eval::Evaluator::new();
                    evaluator.current_file = self.current_file.clone();
                    evaluator.embedded_archive = self.embedded_archive.clone();
                    evaluator.embedded_bytecode = self.embedded_bytecode.clone();
                    match evaluator.load_module(&path_str, crate::ast::Span::new(1, 1)) {
                        Ok(mod_val) => self.stack.push(mod_val),
                        Err(e) => {
                            runtime_error!(self, e.message);
                        }
                    }
                }
                OpCode::ImportStar => {
                    let mod_val = match self.stack.pop() {
                        Some(v) => v,
                        None => {
                            runtime_error!(self, "Stack underflow on ImportStar".to_string());
                        }
                    };
                    if let Value::Map(m) = mod_val {
                        let map = m.read().unwrap();
                        for (k, v) in map.iter() {
                            self.globals.insert(k.clone(), v.clone());
                        }
                    } else {
                        runtime_error!(self, "Import star requires a module map".to_string());
                    }
                }
                OpCode::ForIter => {
                    let seq_local = self.read_byte() as usize;
                    let offset = self.read_short();
                    let slots_offset = self.frames.last().unwrap().slots_offset;
                    let seq_slot = slots_offset + seq_local;
                    let iter_slot = seq_slot + 1;
                    let (next_val, is_done) = {
                        let seq = match self.stack.get(seq_slot) {
                            Some(s) => s,
                            None => {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Invalid stack slot {} for ForIter sequence",
                                        seq_local
                                    )
                                );
                            }
                        };
                        let iter_idx = match self.stack.get(iter_slot) {
                            Some(Value::Int(i)) => *i,
                            _ => {
                                runtime_error!(self, "Iterator index must be an integer.",);
                            }
                        };
                        match seq {
                            Value::GcArray(r) => {
                                let arr = self.heap.as_array(*r).unwrap();
                                if iter_idx >= 0 && (iter_idx as usize) < arr.len() {
                                    (Some(arr[iter_idx as usize].clone()), false)
                                } else {
                                    (None, true)
                                }
                            }
                            Value::Array(arr) => {
                                let borrow = arr.read().unwrap();
                                if iter_idx >= 0 && (iter_idx as usize) < borrow.len() {
                                    (Some(borrow[iter_idx as usize].clone()), false)
                                } else {
                                    (None, true)
                                }
                            }
                            other => {
                                runtime_error!(
                                    self,
                                    format!("Cannot iterate over {}", other.type_name())
                                );
                            }
                        }
                    };
                    if is_done {
                        self.frames.last_mut().unwrap().ip += offset as usize;
                    } else {
                        if let Some(Value::Int(i)) = self.stack.get_mut(iter_slot) {
                            *i += 1;
                        }
                        self.stack.push(next_val.unwrap());
                    }
                }
                OpCode::Call => {
                    let arg_count = self.read_byte() as usize;
                    if self.stack.len() < 1 + arg_count {
                        runtime_error!(self, "Stack underflow on Call");
                    }
                    let callee_slot = self.stack.len() - 1 - arg_count;
                    let callee = self.stack[callee_slot].clone();
                    match callee {
                        Value::Closure(closure) => {
                            if arg_count != closure.function.arity {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Expected {} arguments but got {}.",
                                        closure.function.arity, arg_count
                                    )
                                );
                            }
                            if closure.function.is_async {
                                let args: Vec<Value> =
                                    self.stack.drain(callee_slot + 1..).collect();
                                self.stack.pop(); // pop callee
                                let current_file = self.current_file.clone();
                                let globals = self.globals.clone();
                                let handle = crate::async_rt::spawn_task(move || {
                                    let mut vm = VM::new();
                                    vm.current_file = current_file;
                                    vm.globals = globals;
                                    match vm.execute_closure(closure, args) {
                                        InterpretResult::Ok(val) => Ok(val),
                                        InterpretResult::RuntimeError(msg) => {
                                            Err(crate::eval::RuntimeError::new(msg))
                                        }
                                        InterpretResult::CompileError => {
                                            Err(crate::eval::RuntimeError::new(
                                                "Compile error in VM".into(),
                                            ))
                                        }
                                    }
                                });
                                self.stack
                                    .push(Value::Task(Arc::new(Mutex::new(Some(handle)))));
                                continue;
                            }
                            if let Some(res) = self.try_exec_jit(&closure.function, callee_slot) {
                                match res {
                                    Ok(out_res) => {
                                        self.stack.truncate(callee_slot);
                                        self.stack.push(out_res);
                                        continue;
                                    }
                                    Err(msg) => {
                                        runtime_error!(self, msg);
                                    }
                                }
                            }
                            if self.frames.len() >= 1024 {
                                runtime_error!(
                                    self,
                                    "Stack overflow: call stack exceeded maximum depth.",
                                );
                            }
                            if let Some(prof) = &mut self.profiler {
                                let name =
                                    closure.function.name.as_deref().unwrap_or("<anonymous>");
                                prof.enter_function(name);
                            }
                            self.frames.push(CallFrame {
                                closure,
                                ip: 0,
                                slots_offset: callee_slot,
                                try_handlers: Vec::new(),
                            });
                        }
                        Value::CompiledFunction(func) => {
                            if arg_count != func.arity {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Expected {} arguments but got {}.",
                                        func.arity, arg_count
                                    )
                                );
                            }
                            let mod_globals = self
                                .frames
                                .last()
                                .and_then(|f| f.closure.module_globals.clone())
                                .or_else(|| self.module_globals.clone());
                            let closure = Arc::new(Closure {
                                function: func,
                                upvalues: Vec::new(),
                                module_globals: mod_globals,
                            });
                            if closure.function.is_async {
                                let args: Vec<Value> =
                                    self.stack.drain(callee_slot + 1..).collect();
                                self.stack.pop(); // pop callee
                                let current_file = self.current_file.clone();
                                let globals = self.globals.clone();
                                let handle = crate::async_rt::spawn_task(move || {
                                    let mut vm = VM::new();
                                    vm.current_file = current_file;
                                    vm.globals = globals;
                                    match vm.execute_closure(closure, args) {
                                        InterpretResult::Ok(val) => Ok(val),
                                        InterpretResult::RuntimeError(msg) => {
                                            Err(crate::eval::RuntimeError::new(msg))
                                        }
                                        InterpretResult::CompileError => {
                                            Err(crate::eval::RuntimeError::new(
                                                "Compile error in VM".into(),
                                            ))
                                        }
                                    }
                                });
                                self.stack
                                    .push(Value::Task(Arc::new(Mutex::new(Some(handle)))));
                                continue;
                            }
                            if let Some(res) = self.try_exec_jit(&closure.function, callee_slot) {
                                match res {
                                    Ok(out_res) => {
                                        self.stack.truncate(callee_slot);
                                        self.stack.push(out_res);
                                        continue;
                                    }
                                    Err(msg) => {
                                        runtime_error!(self, msg);
                                    }
                                }
                            }
                            if self.frames.len() >= 1024 {
                                runtime_error!(
                                    self,
                                    "Stack overflow: call stack exceeded maximum depth.",
                                );
                            }
                            if let Some(prof) = &mut self.profiler {
                                let name =
                                    closure.function.name.as_deref().unwrap_or("<anonymous>");
                                prof.enter_function(name);
                            }
                            self.frames.push(CallFrame {
                                closure,
                                ip: 0,
                                slots_offset: callee_slot,
                                try_handlers: Vec::new(),
                            });
                        }
                        Value::Builtin { name, func } => {
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop callee
                            for arg in &args {
                                self.close_upvalues_in_value(arg);
                            }
                            self.open_upvalues.retain(|uv| {
                                matches!(uv.read().unwrap().location, UpvalueLocation::Open(_))
                            });
                            if name == "jit" {
                                let action = match args.first() {
                                    Some(Value::String(s)) => s.as_str(),
                                    Some(Value::GcString(r)) => {
                                        self.heap.as_string(*r).unwrap_or("")
                                    }
                                    _ => "status",
                                };
                                let res = match action {
                                    "status" => {
                                        if self.jit_enabled {
                                            Value::String("enabled".into())
                                        } else {
                                            Value::String("disabled".into())
                                        }
                                    }
                                    "enable" => {
                                        self.jit_enabled = true;
                                        Value::Bool(true)
                                    }
                                    "disable" => {
                                        self.jit_enabled = false;
                                        Value::Bool(false)
                                    }
                                    "compile" => {
                                        if let Some(target) = args.get(1) {
                                            let func_opt = match target {
                                                Value::CompiledFunction(f) => Some(f.clone()),
                                                Value::Closure(c) => Some(c.function.clone()),
                                                _ => None,
                                            };
                                            if let Some(func) = func_opt {
                                                match self.compile_function_jit(&func) {
                                                    Ok(_) => Value::Bool(true),
                                                    Err(_) => Value::Bool(false),
                                                }
                                            } else {
                                                Value::Bool(false)
                                            }
                                        } else {
                                            Value::Bool(false)
                                        }
                                    }
                                    _ => Value::Null,
                                };
                                self.stack.push(res);
                                continue;
                            }
                            if name == "gc" {
                                let is_minor = match args.first() {
                                    Some(Value::String(s)) => s == "minor",
                                    Some(Value::GcString(r)) => self
                                        .heap
                                        .as_string(*r)
                                        .map(|s| s == "minor")
                                        .unwrap_or(false),
                                    _ => false,
                                };
                                let stats = if is_minor {
                                    self.collect_minor()
                                } else {
                                    self.collect_garbage()
                                };
                                self.stack.push(Value::Int(stats.freed_objects as i64));
                                continue;
                            }
                            if name == "len" && args.len() == 1 {
                                match &args[0] {
                                    Value::GcArray(r) => {
                                        let len =
                                            self.heap.as_array(*r).map(|a| a.len()).unwrap_or(0);
                                        self.stack.push(Value::Int(len as i64));
                                        continue;
                                    }
                                    Value::GcMap(r) => {
                                        let len =
                                            self.heap.as_map(*r).map(|m| m.len()).unwrap_or(0);
                                        self.stack.push(Value::Int(len as i64));
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                            let mut evaluator = crate::eval::Evaluator::new();
                            evaluator.current_file = self.current_file.clone();
                            for (k, v) in &self.globals {
                                evaluator
                                    .global_env
                                    .write()
                                    .unwrap()
                                    .define(k.clone(), v.clone());
                            }
                            match func(&mut evaluator, args, crate::ast::Span::new(1, 1)) {
                                Ok(val) => self.stack.push(val),
                                Err(e) => runtime_error!(self, e.message),
                            }
                        }
                        Value::Function { .. }
                        | Value::BoundMethod { .. }
                        | Value::FfiFunction(_) => {
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop callee
                            self.close_upvalues_in_value(&callee);
                            for arg in &args {
                                self.close_upvalues_in_value(arg);
                            }
                            self.open_upvalues.retain(|uv| {
                                matches!(uv.read().unwrap().location, UpvalueLocation::Open(_))
                            });
                            let mut evaluator = crate::eval::Evaluator::new();
                            evaluator.current_file = self.current_file.clone();
                            for (k, v) in &self.globals {
                                evaluator
                                    .global_env
                                    .write()
                                    .unwrap()
                                    .define(k.clone(), v.clone());
                            }
                            match evaluator.call_value(&callee, args, crate::ast::Span::new(1, 1)) {
                                Ok(val) => self.stack.push(val),
                                Err(e) => runtime_error!(self, e.message),
                            }
                        }
                        Value::EnumConstructor {
                            enum_name,
                            variant_name,
                            params,
                        } => {
                            if arg_count != params.len() {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Enum variant {}.{} expects {} arguments, got {}",
                                        enum_name,
                                        variant_name,
                                        params.len(),
                                        arg_count
                                    )
                                );
                            }
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop constructor
                            self.stack.push(Value::EnumInstance {
                                enum_name,
                                variant_name,
                                values: args,
                            });
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("Can only call functions, got {}.", other.type_name())
                            );
                        }
                    }
                }
                OpCode::TailCall => {
                    let arg_count = self.read_byte() as usize;
                    if self.stack.len() < 1 + arg_count {
                        runtime_error!(self, "Stack underflow on TailCall");
                    }
                    let callee_slot = self.stack.len() - 1 - arg_count;
                    let callee = self.stack[callee_slot].clone();

                    let has_try = !self.frames.last().unwrap().try_handlers.is_empty();

                    match callee {
                        Value::Closure(closure) => {
                            if arg_count != closure.function.arity {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Expected {} arguments but got {}.",
                                        closure.function.arity, arg_count
                                    )
                                );
                            }
                            if closure.function.is_async {
                                let args: Vec<Value> =
                                    self.stack.drain(callee_slot + 1..).collect();
                                self.stack.pop(); // pop callee
                                let current_file = self.current_file.clone();
                                let globals = self.globals.clone();
                                let handle = crate::async_rt::spawn_task(move || {
                                    let mut vm = VM::new();
                                    vm.current_file = current_file;
                                    vm.globals = globals;
                                    match vm.execute_closure(closure, args) {
                                        InterpretResult::Ok(val) => Ok(val),
                                        InterpretResult::RuntimeError(msg) => {
                                            Err(crate::eval::RuntimeError::new(msg))
                                        }
                                        InterpretResult::CompileError => {
                                            Err(crate::eval::RuntimeError::new(
                                                "Compile error in VM".into(),
                                            ))
                                        }
                                    }
                                });
                                let val = Value::Task(Arc::new(Mutex::new(Some(handle))));
                                if has_try {
                                    self.stack.push(val);
                                } else {
                                    let frame = self.frames.pop().unwrap();
                                    self.close_upvalues(frame.slots_offset);
                                    if self.frames.is_empty() {
                                        return InterpretResult::Ok(val);
                                    }
                                    self.stack.truncate(frame.slots_offset);
                                    self.stack.push(val);
                                }
                                continue;
                            }
                            if let Some(res) = self.try_exec_jit(&closure.function, callee_slot) {
                                match res {
                                    Ok(out_res) => {
                                        let val = out_res;
                                        if has_try {
                                            self.stack.truncate(callee_slot);
                                            self.stack.push(val);
                                        } else {
                                            let frame = self.frames.pop().unwrap();
                                            self.close_upvalues(frame.slots_offset);
                                            if self.frames.is_empty() {
                                                return InterpretResult::Ok(val);
                                            }
                                            self.stack.truncate(frame.slots_offset);
                                            self.stack.push(val);
                                        }
                                        continue;
                                    }
                                    Err(msg) => {
                                        runtime_error!(self, msg);
                                    }
                                }
                            }
                            if has_try {
                                if self.frames.len() >= 1024 {
                                    runtime_error!(
                                        self,
                                        "Stack overflow: call stack exceeded maximum depth.",
                                    );
                                }
                                self.frames.push(CallFrame {
                                    closure,
                                    ip: 0,
                                    slots_offset: callee_slot,
                                    try_handlers: Vec::new(),
                                });
                            } else {
                                let current_slots_offset = self.frames.last().unwrap().slots_offset;
                                self.close_upvalues(current_slots_offset);

                                let call_slice =
                                    self.stack[callee_slot..callee_slot + 1 + arg_count].to_vec();
                                for (i, val) in call_slice.into_iter().enumerate() {
                                    let target_idx = current_slots_offset + i;
                                    if target_idx < self.stack.len() {
                                        self.stack[target_idx] = val;
                                    } else {
                                        self.stack.push(val);
                                    }
                                }
                                self.stack.truncate(current_slots_offset + 1 + arg_count);

                                let frame = self.frames.last_mut().unwrap();
                                frame.closure = closure;
                                frame.ip = 0;
                            }
                        }
                        Value::CompiledFunction(func) => {
                            if arg_count != func.arity {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Expected {} arguments but got {}.",
                                        func.arity, arg_count
                                    )
                                );
                            }
                            let mod_globals = self
                                .frames
                                .last()
                                .and_then(|f| f.closure.module_globals.clone())
                                .or_else(|| self.module_globals.clone());
                            let closure = Arc::new(Closure {
                                function: func,
                                upvalues: Vec::new(),
                                module_globals: mod_globals,
                            });
                            if closure.function.is_async {
                                let args: Vec<Value> =
                                    self.stack.drain(callee_slot + 1..).collect();
                                self.stack.pop(); // pop callee
                                let current_file = self.current_file.clone();
                                let globals = self.globals.clone();
                                let handle = crate::async_rt::spawn_task(move || {
                                    let mut vm = VM::new();
                                    vm.current_file = current_file;
                                    vm.globals = globals;
                                    match vm.execute_closure(closure, args) {
                                        InterpretResult::Ok(val) => Ok(val),
                                        InterpretResult::RuntimeError(msg) => {
                                            Err(crate::eval::RuntimeError::new(msg))
                                        }
                                        InterpretResult::CompileError => {
                                            Err(crate::eval::RuntimeError::new(
                                                "Compile error in VM".into(),
                                            ))
                                        }
                                    }
                                });
                                let val = Value::Task(Arc::new(Mutex::new(Some(handle))));
                                if has_try {
                                    self.stack.push(val);
                                } else {
                                    let frame = self.frames.pop().unwrap();
                                    self.close_upvalues(frame.slots_offset);
                                    if self.frames.is_empty() {
                                        return InterpretResult::Ok(val);
                                    }
                                    self.stack.truncate(frame.slots_offset);
                                    self.stack.push(val);
                                }
                                continue;
                            }
                            if let Some(res) = self.try_exec_jit(&closure.function, callee_slot) {
                                match res {
                                    Ok(out_res) => {
                                        let val = out_res;
                                        if has_try {
                                            self.stack.truncate(callee_slot);
                                            self.stack.push(val);
                                        } else {
                                            let frame = self.frames.pop().unwrap();
                                            self.close_upvalues(frame.slots_offset);
                                            if self.frames.is_empty() {
                                                return InterpretResult::Ok(val);
                                            }
                                            self.stack.truncate(frame.slots_offset);
                                            self.stack.push(val);
                                        }
                                        continue;
                                    }
                                    Err(msg) => {
                                        runtime_error!(self, msg);
                                    }
                                }
                            }
                            if has_try {
                                if self.frames.len() >= 1024 {
                                    runtime_error!(
                                        self,
                                        "Stack overflow: call stack exceeded maximum depth.",
                                    );
                                }
                                self.frames.push(CallFrame {
                                    closure,
                                    ip: 0,
                                    slots_offset: callee_slot,
                                    try_handlers: Vec::new(),
                                });
                            } else {
                                let current_slots_offset = self.frames.last().unwrap().slots_offset;
                                self.close_upvalues(current_slots_offset);

                                let call_slice =
                                    self.stack[callee_slot..callee_slot + 1 + arg_count].to_vec();
                                for (i, val) in call_slice.into_iter().enumerate() {
                                    let target_idx = current_slots_offset + i;
                                    if target_idx < self.stack.len() {
                                        self.stack[target_idx] = val;
                                    } else {
                                        self.stack.push(val);
                                    }
                                }
                                self.stack.truncate(current_slots_offset + 1 + arg_count);

                                let frame = self.frames.last_mut().unwrap();
                                frame.closure = closure;
                                frame.ip = 0;
                            }
                        }
                        Value::Builtin { name, func } => {
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop callee
                            for arg in &args {
                                self.close_upvalues_in_value(arg);
                            }
                            self.open_upvalues.retain(|uv| {
                                matches!(uv.read().unwrap().location, UpvalueLocation::Open(_))
                            });
                            if name == "jit" {
                                let action = match args.first() {
                                    Some(Value::String(s)) => s.as_str(),
                                    Some(Value::GcString(r)) => {
                                        self.heap.as_string(*r).unwrap_or("")
                                    }
                                    _ => "status",
                                };
                                let res = match action {
                                    "status" => {
                                        if self.jit_enabled {
                                            Value::String("enabled".into())
                                        } else {
                                            Value::String("disabled".into())
                                        }
                                    }
                                    "enable" => {
                                        self.jit_enabled = true;
                                        Value::Bool(true)
                                    }
                                    "disable" => {
                                        self.jit_enabled = false;
                                        Value::Bool(false)
                                    }
                                    "compile" => {
                                        if let Some(target) = args.get(1) {
                                            let func_opt = match target {
                                                Value::CompiledFunction(f) => Some(f.clone()),
                                                Value::Closure(c) => Some(c.function.clone()),
                                                _ => None,
                                            };
                                            if let Some(func) = func_opt {
                                                match self.compile_function_jit(&func) {
                                                    Ok(_) => Value::Bool(true),
                                                    Err(_) => Value::Bool(false),
                                                }
                                            } else {
                                                Value::Bool(false)
                                            }
                                        } else {
                                            Value::Bool(false)
                                        }
                                    }
                                    _ => Value::Null,
                                };
                                if has_try {
                                    self.stack.push(res);
                                } else {
                                    let frame = self.frames.pop().unwrap();
                                    self.close_upvalues(frame.slots_offset);
                                    if self.frames.is_empty() {
                                        return InterpretResult::Ok(res);
                                    }
                                    self.stack.truncate(frame.slots_offset);
                                    self.stack.push(res);
                                }
                                continue;
                            }
                            if name == "gc" {
                                let is_minor = match args.first() {
                                    Some(Value::String(s)) => s == "minor",
                                    Some(Value::GcString(r)) => self
                                        .heap
                                        .as_string(*r)
                                        .map(|s| s == "minor")
                                        .unwrap_or(false),
                                    _ => false,
                                };
                                let stats = if is_minor {
                                    self.collect_minor()
                                } else {
                                    self.collect_garbage()
                                };
                                let val = Value::Int(stats.freed_objects as i64);
                                if has_try {
                                    self.stack.push(val);
                                } else {
                                    let frame = self.frames.pop().unwrap();
                                    self.close_upvalues(frame.slots_offset);
                                    if self.frames.is_empty() {
                                        return InterpretResult::Ok(val);
                                    }
                                    self.stack.truncate(frame.slots_offset);
                                    self.stack.push(val);
                                }
                                continue;
                            }
                            if name == "len" && args.len() == 1 {
                                let len_res = match &args[0] {
                                    Value::GcArray(r) => Some(Value::Int(
                                        self.heap.as_array(*r).map(|a| a.len()).unwrap_or(0) as i64,
                                    )),
                                    Value::GcMap(r) => Some(Value::Int(
                                        self.heap.as_map(*r).map(|m| m.len()).unwrap_or(0) as i64,
                                    )),
                                    _ => None,
                                };
                                if let Some(val) = len_res {
                                    if has_try {
                                        self.stack.push(val);
                                    } else {
                                        let frame = self.frames.pop().unwrap();
                                        self.close_upvalues(frame.slots_offset);
                                        if self.frames.is_empty() {
                                            return InterpretResult::Ok(val);
                                        }
                                        self.stack.truncate(frame.slots_offset);
                                        self.stack.push(val);
                                    }
                                    continue;
                                }
                            }
                            let mut evaluator = crate::eval::Evaluator::new();
                            evaluator.current_file = self.current_file.clone();
                            for (k, v) in &self.globals {
                                evaluator
                                    .global_env
                                    .write()
                                    .unwrap()
                                    .define(k.clone(), v.clone());
                            }
                            match func(&mut evaluator, args, crate::ast::Span::new(1, 1)) {
                                Ok(val) => {
                                    if has_try {
                                        self.stack.push(val);
                                    } else {
                                        let frame = self.frames.pop().unwrap();
                                        self.close_upvalues(frame.slots_offset);
                                        if self.frames.is_empty() {
                                            return InterpretResult::Ok(val);
                                        }
                                        self.stack.truncate(frame.slots_offset);
                                        self.stack.push(val);
                                    }
                                }
                                Err(e) => runtime_error!(self, e.message),
                            }
                        }
                        Value::Function { .. }
                        | Value::BoundMethod { .. }
                        | Value::FfiFunction(_) => {
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop callee
                            self.close_upvalues_in_value(&callee);
                            for arg in &args {
                                self.close_upvalues_in_value(arg);
                            }
                            self.open_upvalues.retain(|uv| {
                                matches!(uv.read().unwrap().location, UpvalueLocation::Open(_))
                            });
                            let mut evaluator = crate::eval::Evaluator::new();
                            evaluator.current_file = self.current_file.clone();
                            for (k, v) in &self.globals {
                                evaluator
                                    .global_env
                                    .write()
                                    .unwrap()
                                    .define(k.clone(), v.clone());
                            }
                            match evaluator.call_value(&callee, args, crate::ast::Span::new(1, 1)) {
                                Ok(val) => {
                                    if has_try {
                                        self.stack.push(val);
                                    } else {
                                        let frame = self.frames.pop().unwrap();
                                        self.close_upvalues(frame.slots_offset);
                                        if self.frames.is_empty() {
                                            return InterpretResult::Ok(val);
                                        }
                                        self.stack.truncate(frame.slots_offset);
                                        self.stack.push(val);
                                    }
                                }
                                Err(e) => runtime_error!(self, e.message),
                            }
                        }
                        Value::EnumConstructor {
                            enum_name,
                            variant_name,
                            params,
                        } => {
                            if arg_count != params.len() {
                                runtime_error!(
                                    self,
                                    format!(
                                        "Enum variant {}.{} expects {} arguments, got {}",
                                        enum_name,
                                        variant_name,
                                        params.len(),
                                        arg_count
                                    )
                                );
                            }
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop constructor
                            let val = Value::EnumInstance {
                                enum_name,
                                variant_name,
                                values: args,
                            };
                            if has_try {
                                self.stack.push(val);
                            } else {
                                let frame = self.frames.pop().unwrap();
                                self.close_upvalues(frame.slots_offset);
                                if self.frames.is_empty() {
                                    return InterpretResult::Ok(val);
                                }
                                self.stack.truncate(frame.slots_offset);
                                self.stack.push(val);
                            }
                        }
                        other => {
                            runtime_error!(
                                self,
                                format!("Can only call functions, got {}.", other.type_name())
                            );
                        }
                    }
                }
                OpCode::Closure => {
                    let const_idx = self.read_byte() as usize;
                    let func = match self.frames.last().unwrap().closure.function.chunk.constants
                        [const_idx]
                        .clone()
                    {
                        Value::CompiledFunction(f) => f,
                        _ => {
                            runtime_error!(self, "Expected compiled function for closure",);
                        }
                    };
                    let mut upvalues = Vec::with_capacity(func.upvalues.len());
                    let current_slots_offset = self.frames.last().unwrap().slots_offset;
                    for desc in &func.upvalues {
                        if desc.is_local {
                            let slot = current_slots_offset + desc.index as usize;
                            upvalues.push(self.capture_upvalue(slot));
                        } else {
                            let parent_upvalue = self.frames.last().unwrap().closure.upvalues
                                [desc.index as usize]
                                .clone();
                            upvalues.push(parent_upvalue);
                        }
                    }
                    let mod_globals = self
                        .frames
                        .last()
                        .and_then(|f| f.closure.module_globals.clone())
                        .or_else(|| self.module_globals.clone());
                    let closure = Arc::new(Closure {
                        function: func,
                        upvalues,
                        module_globals: mod_globals,
                    });
                    self.stack.push(Value::Closure(closure));
                }
                OpCode::GetUpvalue => {
                    let slot = self.read_byte() as usize;
                    let upvalue = self.frames.last().unwrap().closure.upvalues[slot].clone();
                    let val = match &upvalue.read().unwrap().location {
                        UpvalueLocation::Open(stack_idx) => {
                            if *stack_idx < self.stack.len() {
                                self.stack[*stack_idx].clone()
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Invalid upvalue stack index {}", stack_idx)
                                );
                            }
                        }
                        UpvalueLocation::Closed(v) => v.clone(),
                    };
                    self.stack.push(val);
                }
                OpCode::SetUpvalue => {
                    let slot = self.read_byte() as usize;
                    let val = match self.stack.last().cloned() {
                        Some(v) => v,
                        None => {
                            runtime_error!(self, "Stack empty on SetUpvalue",);
                        }
                    };
                    let upvalue = self.frames.last().unwrap().closure.upvalues[slot].clone();
                    let mut uv = upvalue.write().unwrap();
                    match &mut uv.location {
                        UpvalueLocation::Open(stack_idx) => {
                            if *stack_idx < self.stack.len() {
                                self.stack[*stack_idx] = val;
                            } else {
                                runtime_error!(
                                    self,
                                    format!("Invalid upvalue stack index {}", stack_idx)
                                );
                            }
                        }
                        UpvalueLocation::Closed(v) => {
                            *v = val;
                        }
                    }
                }
                OpCode::CloseUpvalue => {
                    let top_slot = self.stack.len() - 1;
                    self.close_upvalues(top_slot);
                    self.stack.pop();
                }
                OpCode::Await => {
                    let val = match self.stack.pop() {
                        Some(v) => v,
                        None => {
                            runtime_error!(self, "Stack underflow on Await");
                        }
                    };
                    match val {
                        Value::Task(t) => {
                            let mut guard = t.lock().unwrap();
                            if let Some(handle) = guard.take() {
                                match handle.join() {
                                    Ok(Ok(resolved)) => {
                                        self.stack.push(resolved);
                                    }
                                    Ok(Err(e)) => {
                                        runtime_error!(self, e.message);
                                    }
                                    Err(_) => {
                                        runtime_error!(self, "Async task panicked");
                                    }
                                }
                            } else {
                                runtime_error!(self, "Task already awaited or joined");
                            }
                        }
                        other => {
                            self.stack.push(other);
                        }
                    }
                }
                _ => unimplemented!("Opcode {:?} not yet implemented", instruction),
            }
        }
    }

    fn read_byte(&mut self) -> u8 {
        let frame = self.frames.last_mut().unwrap();
        let byte = frame.closure.function.chunk.code[frame.ip];
        frame.ip += 1;
        byte
    }

    fn read_short(&mut self) -> u16 {
        let frame = self.frames.last_mut().unwrap();
        frame.ip += 2;
        ((frame.closure.function.chunk.code[frame.ip - 2] as u16) << 8)
            | (frame.closure.function.chunk.code[frame.ip - 1] as u16)
    }

    fn read_constant(&mut self) -> Value {
        let frame = self.frames.last_mut().unwrap();
        let idx = frame.closure.function.chunk.code[frame.ip] as usize;
        frame.ip += 1;
        frame.closure.function.chunk.constants[idx].clone()
    }

    fn read_constant_long(&mut self) -> Value {
        let idx = self.read_short() as usize;
        let frame = self.frames.last().unwrap();
        frame.closure.function.chunk.constants[idx].clone()
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}
