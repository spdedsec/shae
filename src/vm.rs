use crate::chunk::Chunk;
use crate::opcode::OpCode;
use crate::value::{resolve_index, resolve_int_index, CompiledFunction, IndexError, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub function: Arc<CompiledFunction>,
    pub ip: usize,
    pub slots_offset: usize,
}

pub struct VM {
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: HashMap<String, Value>,
}

#[derive(Debug, PartialEq)]
pub enum InterpretResult {
    Ok(Value),
    CompileError,
    RuntimeError(String),
}

impl VM {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        let mut dummy_env = crate::env::Environment::new();
        crate::builtins::register(&mut dummy_env);
        for (k, v) in dummy_env.export_map() {
            globals.insert(k, v);
        }
        VM {
            frames: Vec::with_capacity(64),
            stack: Vec::with_capacity(256),
            globals,
        }
    }

    pub fn interpret(&mut self, chunk: Chunk) -> InterpretResult {
        let top_fn = Arc::new(CompiledFunction {
            arity: 0,
            chunk,
            name: None,
        });
        self.frames.clear();
        self.frames.push(CallFrame {
            function: top_fn,
            ip: 0,
            slots_offset: 0,
        });
        self.run()
    }

    fn run(&mut self) -> InterpretResult {
        loop {
            if self.frames.is_empty() {
                return InterpretResult::Ok(Value::Null);
            }
            if self.frames.last().unwrap().ip >= self.frames.last().unwrap().function.chunk.code.len() {
                let frame = self.frames.pop().unwrap();
                if self.frames.is_empty() {
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    return InterpretResult::Ok(val);
                }
                let val = self.stack.pop().unwrap_or(Value::Null);
                self.stack.truncate(frame.slots_offset);
                self.stack.push(val);
                continue;
            }

            let instruction: OpCode = self.read_byte().into();
            match instruction {
                OpCode::Return => {
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    let frame = self.frames.pop().unwrap();
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
                        return InterpretResult::RuntimeError(format!(
                            "Stack underflow on local slot {}",
                            slot_idx
                        ));
                    }
                }
                OpCode::SetLocal => {
                    let slot_idx = self.read_byte() as usize;
                    let slot = self.frames.last().unwrap().slots_offset + slot_idx;
                    if let Some(val) = self.stack.last().cloned() {
                        if slot < self.stack.len() {
                            self.stack[slot] = val;
                        } else {
                            return InterpretResult::RuntimeError(format!(
                                "Invalid local slot {} for assignment",
                                slot_idx
                            ));
                        }
                    } else {
                        return InterpretResult::RuntimeError("Stack empty on SetLocal".into());
                    }
                }
                OpCode::DefineGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let val = self.stack.pop().unwrap_or(Value::Null);
                    self.globals.insert(name, val);
                }
                OpCode::GetGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    if let Some(val) = self.globals.get(&name) {
                        self.stack.push(val.clone());
                    } else {
                        return InterpretResult::RuntimeError(format!(
                            "Undefined variable '{}'.",
                            name
                        ));
                    }
                }
                OpCode::SetGlobal => {
                    let name = match self.read_constant() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    if let Some(val) = self.stack.last().cloned() {
                        if self.globals.contains_key(&name) {
                            self.globals.insert(name, val);
                        } else {
                            return InterpretResult::RuntimeError(format!(
                                "Undefined variable '{}'.",
                                name
                            ));
                        }
                    } else {
                        return InterpretResult::RuntimeError("Stack empty on SetGlobal".into());
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
                            return InterpretResult::RuntimeError(
                                "Operands must be numbers for comparison.".into(),
                            )
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
                            return InterpretResult::RuntimeError(
                                "Operands must be numbers for comparison.".into(),
                            )
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
                    _ => return InterpretResult::RuntimeError("Operand must be a number.".into()),
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
                            self.stack.push(Value::String(format!("{}{}", a_str, b_str)));
                        }
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be two numbers or two strings.".into(),
                            )
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
                            return InterpretResult::RuntimeError("Operands must be numbers.".into())
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
                            return InterpretResult::RuntimeError("Operands must be numbers.".into())
                        }
                    }
                }
                OpCode::Divide => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(a_num), Value::Int(b_num)) => {
                            if b_num == 0 {
                                return InterpretResult::RuntimeError("Divide by zero.".into());
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
                            return InterpretResult::RuntimeError("Operands must be numbers.".into())
                        }
                    }
                }
                OpCode::Mod => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            if y == 0 {
                                return InterpretResult::RuntimeError("Modulo by zero.".into());
                            }
                            self.stack.push(Value::Int(x % y));
                        }
                        (Value::Float(x), Value::Float(y)) => {
                            if y == 0.0 {
                                return InterpretResult::RuntimeError("Modulo by zero.".into());
                            }
                            self.stack.push(Value::Float(x % y));
                        }
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be numbers for modulo.".into(),
                            )
                        }
                    }
                }
                OpCode::BitAnd => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x & y)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be integers for bitwise AND.".into(),
                            )
                        }
                    }
                }
                OpCode::BitOr => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x | y)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be integers for bitwise OR.".into(),
                            )
                        }
                    }
                }
                OpCode::BitXor => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x ^ y)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be integers for bitwise XOR.".into(),
                            )
                        }
                    }
                }
                OpCode::BitNot => {
                    let a = self.stack.pop().unwrap();
                    match a {
                        Value::Int(x) => self.stack.push(Value::Int(!x)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operand must be an integer for bitwise NOT.".into(),
                            )
                        }
                    }
                }
                OpCode::Shl => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x << y)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be integers for shift left.".into(),
                            )
                        }
                    }
                }
                OpCode::Shr => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x >> y)),
                        _ => {
                            return InterpretResult::RuntimeError(
                                "Operands must be integers for shift right.".into(),
                            )
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
                    self.stack.push(Value::Array(Arc::new(RwLock::new(elements))));
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
                        Value::Array(arr) => {
                            let borrow = arr.read().unwrap();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, borrow.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, borrow.len())
                                }
                                _ => {
                                    return InterpretResult::RuntimeError(
                                        "Array index must be a number.".into(),
                                    )
                                }
                            };
                            match idx_res {
                                Ok(idx) => self.stack.push(borrow[idx].clone()),
                                Err(IndexError::NotWhole) => {
                                    return InterpretResult::RuntimeError(
                                        "Array index must be a whole number.".into(),
                                    )
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    return InterpretResult::RuntimeError(format!(
                                        "Index out of bounds for array of length {}",
                                        len
                                    ))
                                }
                            }
                        }
                        Value::String(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, chars.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, chars.len())
                                }
                                _ => {
                                    return InterpretResult::RuntimeError(
                                        "String index must be a number.".into(),
                                    )
                                }
                            };
                            match idx_res {
                                Ok(idx) => self.stack.push(Value::String(chars[idx].to_string())),
                                Err(IndexError::NotWhole) => {
                                    return InterpretResult::RuntimeError(
                                        "String index must be a whole number.".into(),
                                    )
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    return InterpretResult::RuntimeError(format!(
                                        "Index out of bounds for string of length {}",
                                        len
                                    ))
                                }
                            }
                        }
                        Value::Map(m) => {
                            if let Value::String(key) = index {
                                let borrow = m.read().unwrap();
                                let val = borrow.get(&key).cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                return InterpretResult::RuntimeError(
                                    "Map key must be a string.".into(),
                                );
                            }
                        }
                        other => {
                            return InterpretResult::RuntimeError(format!(
                                "Cannot index into {}.",
                                other.type_name()
                            ));
                        }
                    }
                }
                OpCode::IndexSet => {
                    let value = self.stack.pop().unwrap();
                    let index = self.stack.pop().unwrap();
                    let target = self.stack.pop().unwrap();
                    match target {
                        Value::Array(arr) => {
                            let mut borrow = arr.write().unwrap();
                            let idx_res = match index {
                                Value::Int(i) => resolve_int_index(i, borrow.len()),
                                Value::Float(f) | Value::Number(f) => {
                                    resolve_index(f, borrow.len())
                                }
                                _ => {
                                    return InterpretResult::RuntimeError(
                                        "Array index must be a number.".into(),
                                    )
                                }
                            };
                            match idx_res {
                                Ok(idx) => {
                                    borrow[idx] = value.clone();
                                    self.stack.push(value);
                                }
                                Err(IndexError::NotWhole) => {
                                    return InterpretResult::RuntimeError(
                                        "Array index must be a whole number.".into(),
                                    )
                                }
                                Err(IndexError::OutOfRange(len)) => {
                                    return InterpretResult::RuntimeError(format!(
                                        "Index out of bounds for array of length {}",
                                        len
                                    ))
                                }
                            }
                        }
                        Value::Map(m) => {
                            if let Value::String(key) = index {
                                m.write().unwrap().insert(key, value.clone());
                                self.stack.push(value);
                            } else {
                                return InterpretResult::RuntimeError(
                                    "Map key must be a string.".into(),
                                );
                            }
                        }
                        other => {
                            return InterpretResult::RuntimeError(format!(
                                "Cannot index into {}.",
                                other.type_name()
                            ));
                        }
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
                                return InterpretResult::RuntimeError(format!(
                                    "Invalid stack slot {} for ForIter sequence",
                                    seq_local
                                ));
                            }
                        };
                        let iter_idx = match self.stack.get(iter_slot) {
                            Some(Value::Int(i)) => *i,
                            _ => {
                                return InterpretResult::RuntimeError(
                                    "Iterator index must be an integer.".into(),
                                );
                            }
                        };
                        match seq {
                            Value::Array(arr) => {
                                let borrow = arr.read().unwrap();
                                if iter_idx >= 0 && (iter_idx as usize) < borrow.len() {
                                    (Some(borrow[iter_idx as usize].clone()), false)
                                } else {
                                    (None, true)
                                }
                            }
                            other => {
                                return InterpretResult::RuntimeError(format!(
                                    "Cannot iterate over {}",
                                    other.type_name()
                                ));
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
                        return InterpretResult::RuntimeError("Stack underflow on Call".into());
                    }
                    let callee_slot = self.stack.len() - 1 - arg_count;
                    let callee = self.stack[callee_slot].clone();
                    match callee {
                        Value::CompiledFunction(func) => {
                            if arg_count != func.arity {
                                return InterpretResult::RuntimeError(format!(
                                    "Expected {} arguments but got {}.",
                                    func.arity, arg_count
                                ));
                            }
                            if self.frames.len() >= 1024 {
                                return InterpretResult::RuntimeError(
                                    "Stack overflow: call stack exceeded maximum depth.".into(),
                                );
                            }
                            self.frames.push(CallFrame {
                                function: func,
                                ip: 0,
                                slots_offset: callee_slot,
                            });
                        }
                        Value::Builtin { name: _, func } => {
                            let args: Vec<Value> = self.stack.drain(callee_slot + 1..).collect();
                            self.stack.pop(); // pop callee
                            let mut evaluator = crate::eval::Evaluator::new();
                            match func(&mut evaluator, args, crate::ast::Span::new(1, 1)) {
                                Ok(val) => self.stack.push(val),
                                Err(e) => return InterpretResult::RuntimeError(e.message),
                            }
                        }
                        other => {
                            return InterpretResult::RuntimeError(format!(
                                "Can only call functions, got {}.",
                                other.type_name()
                            ));
                        }
                    }
                }
                _ => unimplemented!("Opcode {:?} not yet implemented", instruction),
            }
        }
    }

    fn read_byte(&mut self) -> u8 {
        let frame = self.frames.last_mut().unwrap();
        let byte = frame.function.chunk.code[frame.ip];
        frame.ip += 1;
        byte
    }

    fn read_short(&mut self) -> u16 {
        let frame = self.frames.last_mut().unwrap();
        frame.ip += 2;
        ((frame.function.chunk.code[frame.ip - 2] as u16) << 8)
            | (frame.function.chunk.code[frame.ip - 1] as u16)
    }

    fn read_constant(&mut self) -> Value {
        let frame = self.frames.last_mut().unwrap();
        let idx = frame.function.chunk.code[frame.ip] as usize;
        frame.ip += 1;
        frame.function.chunk.constants[idx].clone()
    }
}
