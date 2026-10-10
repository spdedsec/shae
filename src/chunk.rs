use crate::opcode::OpCode;
use crate::value::{CompiledFunction, UpvalueDesc, Value};
use indexmap::IndexMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, RwLock};

pub const BYTECODE_MAGIC: &[u8; 5] = b"\x7fSHAE";
pub const BYTECODE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<Value>,
    pub lines: Vec<usize>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
        }
    }

    pub fn write(&mut self, byte: u8, line: usize) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub fn write_opcode(&mut self, op: OpCode, line: usize) {
        self.write(op as u8, line);
    }

    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    pub fn disassemble(&self, name: &str) -> String {
        let mut out = format!("== {} ==\n", name);
        if !self.constants.is_empty() {
            out.push_str("Constants:\n");
            for (i, c) in self.constants.iter().enumerate() {
                out.push_str(&format!("  [{:04}] {}\n", i, c.to_display()));
            }
        }
        out.push_str("Disassembly:\n");
        let mut offset = 0;
        while offset < self.code.len() {
            let (instr_str, next_offset) = self.disassemble_instruction(offset);
            out.push_str(&instr_str);
            out.push('\n');
            offset = next_offset;
        }
        out
    }

    pub fn disassemble_instruction(&self, offset: usize) -> (String, usize) {
        let line = self.lines.get(offset).copied().unwrap_or(0);
        let same_line = offset > 0 && self.lines.get(offset - 1) == Some(&line);
        let line_str = if same_line {
            "   |".to_string()
        } else {
            format!("{:4}", line)
        };

        if offset >= self.code.len() {
            return (format!("{:04} {} <EOF>", offset, line_str), offset + 1);
        }

        let op_byte = self.code[offset];
        if op_byte > OpCode::SetLocalLong as u8 {
            return (
                format!("{:04} {} UnknownOp(0x{:02x})", offset, line_str, op_byte),
                offset + 1,
            );
        }

        let opcode: OpCode = op_byte.into();
        match opcode {
            OpCode::Return
            | OpCode::Nil
            | OpCode::True
            | OpCode::False
            | OpCode::Pop
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
            | OpCode::Print
            | OpCode::CloseUpvalue
            | OpCode::Class
            | OpCode::Method
            | OpCode::Invoke
            | OpCode::Inherit
            | OpCode::GetSuper
            | OpCode::SuperInvoke
            | OpCode::IndexGet
            | OpCode::IndexSet
            | OpCode::IndexGetSafe
            | OpCode::ArraySlice
            | OpCode::MatchError
            | OpCode::PopTry
            | OpCode::Dup
            | OpCode::ImportModule
            | OpCode::ImportStar => (
                format!("{:04} {} {:?}", offset, line_str, opcode),
                offset + 1,
            ),
            OpCode::Constant => {
                let const_idx = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let val_str = self
                    .constants
                    .get(const_idx)
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "<invalid>".into());
                (
                    format!(
                        "{:04} {} {:<16} {:4} ({})",
                        offset, line_str, "Constant", const_idx, val_str
                    ),
                    offset + 2,
                )
            }
            OpCode::ConstantLong => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let const_idx = (hi << 8) | lo;
                let val_str = self
                    .constants
                    .get(const_idx)
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "<invalid>".into());
                (
                    format!(
                        "{:04} {} {:<16} {:4} ({})",
                        offset, line_str, "ConstantLong", const_idx, val_str
                    ),
                    offset + 3,
                )
            }
            OpCode::GetLocal => {
                let slot = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} slot {}",
                        offset, line_str, "GetLocal", slot
                    ),
                    offset + 2,
                )
            }
            OpCode::SetLocal => {
                let slot = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} slot {}",
                        offset, line_str, "SetLocal", slot
                    ),
                    offset + 2,
                )
            }
            OpCode::GetLocalLong => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let slot = (hi << 8) | lo;
                (
                    format!(
                        "{:04} {} {:<16} slot {}",
                        offset, line_str, "GetLocalLong", slot
                    ),
                    offset + 3,
                )
            }
            OpCode::SetLocalLong => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let slot = (hi << 8) | lo;
                (
                    format!(
                        "{:04} {} {:<16} slot {}",
                        offset, line_str, "SetLocalLong", slot
                    ),
                    offset + 3,
                )
            }
            OpCode::DefineGlobal | OpCode::GetGlobal | OpCode::SetGlobal => {
                let const_idx = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let val_str = self
                    .constants
                    .get(const_idx)
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "<invalid>".into());
                (
                    format!(
                        "{:04} {} {:<16} {:4} ({})",
                        offset,
                        line_str,
                        format!("{:?}", opcode),
                        const_idx,
                        val_str
                    ),
                    offset + 2,
                )
            }
            OpCode::Jump | OpCode::JumpIfFalse => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let jump = (hi << 8) | lo;
                let target = offset + 3 + jump;
                (
                    format!(
                        "{:04} {} {:<16} +{} -> {:04}",
                        offset,
                        line_str,
                        format!("{:?}", opcode),
                        jump,
                        target
                    ),
                    offset + 3,
                )
            }
            OpCode::Loop => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let jump = (hi << 8) | lo;
                let target = (offset + 3).saturating_sub(jump);
                (
                    format!(
                        "{:04} {} {:<16} -{} -> {:04}",
                        offset, line_str, "Loop", jump, target
                    ),
                    offset + 3,
                )
            }
            OpCode::Call => {
                let argc = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!("{:04} {} {:<16} argc {}", offset, line_str, "Call", argc),
                    offset + 2,
                )
            }
            OpCode::Closure => {
                let const_idx = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                (
                    format!(
                        "{:04} {} {:<16} fn constant [{}]",
                        offset, line_str, "Closure", const_idx
                    ),
                    offset + 2,
                )
            }
            OpCode::GetUpvalue => {
                let slot = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} uv {}",
                        offset, line_str, "GetUpvalue", slot
                    ),
                    offset + 2,
                )
            }
            OpCode::SetUpvalue => {
                let slot = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} uv {}",
                        offset, line_str, "SetUpvalue", slot
                    ),
                    offset + 2,
                )
            }
            OpCode::GetProperty => {
                let name_idx = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let is_safe = self.code.get(offset + 2).copied().unwrap_or(0);
                let val_str = self
                    .constants
                    .get(name_idx)
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "<invalid>".into());
                (
                    format!(
                        "{:04} {} {:<16} [{}] ({}) safe={}",
                        offset, line_str, "GetProperty", name_idx, val_str, is_safe
                    ),
                    offset + 3,
                )
            }
            OpCode::SetProperty => {
                let name_idx = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let val_str = self
                    .constants
                    .get(name_idx)
                    .map(|v| v.to_display())
                    .unwrap_or_else(|| "<invalid>".into());
                (
                    format!(
                        "{:04} {} {:<16} [{}] ({})",
                        offset, line_str, "SetProperty", name_idx, val_str
                    ),
                    offset + 2,
                )
            }
            OpCode::BuildList => {
                let len = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!("{:04} {} {:<16} len {}", offset, line_str, "BuildList", len),
                    offset + 2,
                )
            }
            OpCode::BuildMap => {
                let len = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!("{:04} {} {:<16} len {}", offset, line_str, "BuildMap", len),
                    offset + 2,
                )
            }
            OpCode::ForIter => {
                let seq_slot = self.code.get(offset + 1).copied().unwrap_or(0);
                let hi = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 3).copied().unwrap_or(0) as usize;
                let jump = (hi << 8) | lo;
                let target = offset + 4 + jump;
                (
                    format!(
                        "{:04} {} {:<16} seq_slot {} -> {:04}",
                        offset, line_str, "ForIter", seq_slot, target
                    ),
                    offset + 4,
                )
            }
            OpCode::FormatString => {
                let parts = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} parts {}",
                        offset, line_str, "FormatString", parts
                    ),
                    offset + 2,
                )
            }
            OpCode::MapRest => {
                let fields = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} fields {}",
                        offset, line_str, "MapRest", fields
                    ),
                    offset + 2,
                )
            }
            OpCode::BuildStruct => {
                let fields = self.code.get(offset + 1).copied().unwrap_or(0);
                (
                    format!(
                        "{:04} {} {:<16} fields {}",
                        offset, line_str, "BuildStruct", fields
                    ),
                    offset + 2,
                )
            }
            OpCode::MatchEnum => {
                let field_count = self.code.get(offset + 1).copied().unwrap_or(0);
                let variant_idx = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let enum_idx = self.code.get(offset + 3).copied().unwrap_or(0) as usize;
                (
                    format!(
                        "{:04} {} {:<16} fields={} var=[{}] enum=[{}]",
                        offset, line_str, "MatchEnum", field_count, variant_idx, enum_idx
                    ),
                    offset + 4,
                )
            }
            OpCode::MatchRange => {
                let inclusive = self.code.get(offset + 1).copied().unwrap_or(0) != 0;
                (
                    format!(
                        "{:04} {} {:<16} inclusive={}",
                        offset, line_str, "MatchRange", inclusive
                    ),
                    offset + 2,
                )
            }
            OpCode::PushTry => {
                let hi = self.code.get(offset + 1).copied().unwrap_or(0) as usize;
                let lo = self.code.get(offset + 2).copied().unwrap_or(0) as usize;
                let jump = (hi << 8) | lo;
                let target = offset + 3 + jump;
                (
                    format!(
                        "{:04} {} {:<16} catch -> {:04}",
                        offset, line_str, "PushTry", target
                    ),
                    offset + 3,
                )
            }
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize(&mut buf).expect("in-memory write cannot fail");
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut cursor = io::Cursor::new(bytes);
        Self::deserialize(&mut cursor)
    }

    pub fn serialize<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(BYTECODE_MAGIC)?;
        w.write_all(&BYTECODE_VERSION.to_le_bytes())?;
        w.write_all(&0u16.to_le_bytes())?; // flags / reserved
        self.serialize_inner(w)
    }

    pub fn deserialize<R: Read>(r: &mut R) -> Result<Self, String> {
        let mut magic = [0u8; 5];
        r.read_exact(&mut magic)
            .map_err(|e| format!("Failed to read bytecode magic: {}", e))?;
        if &magic != BYTECODE_MAGIC {
            return Err("Invalid bytecode file: magic header mismatch (expected \\x7fSHAE)".to_string());
        }

        let mut ver_bytes = [0u8; 2];
        r.read_exact(&mut ver_bytes)
            .map_err(|e| format!("Failed to read bytecode version: {}", e))?;
        let version = u16::from_le_bytes(ver_bytes);
        if version != BYTECODE_VERSION {
            return Err(format!(
                "Unsupported bytecode version {} (supported: {})",
                version, BYTECODE_VERSION
            ));
        }

        let mut flags = [0u8; 2];
        r.read_exact(&mut flags)
            .map_err(|e| format!("Failed to read bytecode flags: {}", e))?;

        Self::deserialize_inner(r)
    }

    pub fn serialize_inner<W: Write>(&self, w: &mut W) -> io::Result<()> {
        // 1. Code
        w.write_all(&(self.code.len() as u32).to_le_bytes())?;
        w.write_all(&self.code)?;

        // 2. Lines
        w.write_all(&(self.lines.len() as u32).to_le_bytes())?;
        for &line in &self.lines {
            w.write_all(&(line as u32).to_le_bytes())?;
        }

        // 3. Constants
        w.write_all(&(self.constants.len() as u32).to_le_bytes())?;
        for c in &self.constants {
            Self::serialize_value(c, w)?;
        }
        Ok(())
    }

    pub fn deserialize_inner<R: Read>(r: &mut R) -> Result<Self, String> {
        // 1. Code
        let code_len = read_u32(r)? as usize;
        if code_len > 100_000_000 {
            return Err("Bytecode code segment exceeds 100MB limit".to_string());
        }
        let mut code = vec![0u8; code_len];
        r.read_exact(&mut code)
            .map_err(|e| format!("Failed to read bytecode instructions: {}", e))?;

        // 2. Lines
        let lines_len = read_u32(r)? as usize;
        if lines_len > 100_000_000 {
            return Err("Bytecode line numbers segment exceeds limit".to_string());
        }
        let mut lines = Vec::with_capacity(lines_len);
        for _ in 0..lines_len {
            lines.push(read_u32(r)? as usize);
        }

        // 3. Constants
        let const_len = read_u32(r)? as usize;
        if const_len > 10_000_000 {
            return Err("Bytecode constants pool exceeds limit".to_string());
        }
        let mut constants = Vec::with_capacity(const_len);
        for _ in 0..const_len {
            constants.push(Self::deserialize_value(r)?);
        }

        Ok(Chunk {
            code,
            constants,
            lines,
        })
    }

    fn serialize_value<W: Write>(val: &Value, w: &mut W) -> io::Result<()> {
        match val {
            Value::Null => {
                w.write_all(&[0u8])?;
            }
            Value::Bool(b) => {
                w.write_all(&[1u8, if *b { 1 } else { 0 }])?;
            }
            Value::Int(n) => {
                w.write_all(&[2u8])?;
                w.write_all(&n.to_le_bytes())?;
            }
            Value::Float(n) | Value::Number(n) => {
                w.write_all(&[3u8])?;
                w.write_all(&n.to_bits().to_le_bytes())?;
            }
            Value::String(s) => {
                w.write_all(&[4u8])?;
                let bytes = s.as_bytes();
                w.write_all(&(bytes.len() as u32).to_le_bytes())?;
                w.write_all(bytes)?;
            }
            Value::CompiledFunction(func) => {
                w.write_all(&[5u8])?;
                w.write_all(&(func.arity as u32).to_le_bytes())?;
                if let Some(name) = &func.name {
                    w.write_all(&[1u8])?;
                    let bytes = name.as_bytes();
                    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
                    w.write_all(bytes)?;
                } else {
                    w.write_all(&[0u8])?;
                }
                w.write_all(&(func.upvalues.len() as u32).to_le_bytes())?;
                for uv in &func.upvalues {
                    w.write_all(&[uv.index, if uv.is_local { 1 } else { 0 }])?;
                }
                func.chunk.serialize_inner(w)?;
            }
            Value::StructDef { name, fields } => {
                w.write_all(&[6u8])?;
                let n_bytes = name.as_bytes();
                w.write_all(&(n_bytes.len() as u32).to_le_bytes())?;
                w.write_all(n_bytes)?;
                w.write_all(&(fields.len() as u32).to_le_bytes())?;
                for f in fields {
                    let f_bytes = f.as_bytes();
                    w.write_all(&(f_bytes.len() as u32).to_le_bytes())?;
                    w.write_all(f_bytes)?;
                }
            }
            Value::EnumDef { name, variants } => {
                w.write_all(&[7u8])?;
                let n_bytes = name.as_bytes();
                w.write_all(&(n_bytes.len() as u32).to_le_bytes())?;
                w.write_all(n_bytes)?;
                w.write_all(&(variants.len() as u32).to_le_bytes())?;
                for (vname, f_list) in variants.iter() {
                    let v_bytes = vname.as_bytes();
                    w.write_all(&(v_bytes.len() as u32).to_le_bytes())?;
                    w.write_all(v_bytes)?;
                    w.write_all(&(f_list.len() as u32).to_le_bytes())?;
                    for f in f_list {
                        let f_bytes = f.as_bytes();
                        w.write_all(&(f_bytes.len() as u32).to_le_bytes())?;
                        w.write_all(f_bytes)?;
                    }
                }
            }
            Value::Array(arr) => {
                w.write_all(&[8u8])?;
                let guard = arr.read().unwrap();
                w.write_all(&(guard.len() as u32).to_le_bytes())?;
                for elem in guard.iter() {
                    Self::serialize_value(elem, w)?;
                }
            }
            Value::Map(map) => {
                w.write_all(&[9u8])?;
                let guard = map.read().unwrap();
                w.write_all(&(guard.len() as u32).to_le_bytes())?;
                for (k, v) in guard.iter() {
                    let k_bytes = k.as_bytes();
                    w.write_all(&(k_bytes.len() as u32).to_le_bytes())?;
                    w.write_all(k_bytes)?;
                    Self::serialize_value(v, w)?;
                }
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Cannot serialize non-constant value: {}", other.type_name()),
                ));
            }
        }
        Ok(())
    }

    fn deserialize_value<R: Read>(r: &mut R) -> Result<Value, String> {
        let tag = read_u8(r)?;
        match tag {
            0 => Ok(Value::Null),
            1 => {
                let b = read_u8(r)? != 0;
                Ok(Value::Bool(b))
            }
            2 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)
                    .map_err(|e| format!("Failed to read int: {}", e))?;
                Ok(Value::Int(i64::from_le_bytes(b)))
            }
            3 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)
                    .map_err(|e| format!("Failed to read float: {}", e))?;
                Ok(Value::Float(f64::from_bits(u64::from_le_bytes(b))))
            }
            4 => {
                let s = read_string(r)?;
                Ok(Value::String(s))
            }
            5 => {
                let arity = read_u32(r)? as usize;
                let has_name = read_u8(r)? != 0;
                let name = if has_name {
                    Some(read_string(r)?)
                } else {
                    None
                };
                let uv_len = read_u32(r)? as usize;
                let mut upvalues = Vec::with_capacity(uv_len);
                for _ in 0..uv_len {
                    let index = read_u8(r)?;
                    let is_local = read_u8(r)? != 0;
                    upvalues.push(UpvalueDesc { index, is_local });
                }
                let chunk = Self::deserialize_inner(r)?;
                Ok(Value::CompiledFunction(Arc::new(CompiledFunction {
                    arity,
                    chunk,
                    name,
                    upvalues,
                })))
            }
            6 => {
                let name = read_string(r)?;
                let fields_len = read_u32(r)? as usize;
                let mut fields = Vec::with_capacity(fields_len);
                for _ in 0..fields_len {
                    fields.push(read_string(r)?);
                }
                Ok(Value::StructDef { name, fields })
            }
            7 => {
                let name = read_string(r)?;
                let var_len = read_u32(r)? as usize;
                let mut variants = IndexMap::new();
                for _ in 0..var_len {
                    let vname = read_string(r)?;
                    let f_len = read_u32(r)? as usize;
                    let mut f_list = Vec::with_capacity(f_len);
                    for _ in 0..f_len {
                        f_list.push(read_string(r)?);
                    }
                    variants.insert(vname, f_list);
                }
                Ok(Value::EnumDef {
                    name,
                    variants: Arc::new(variants),
                })
            }
            8 => {
                let arr_len = read_u32(r)? as usize;
                let mut items = Vec::with_capacity(arr_len);
                for _ in 0..arr_len {
                    items.push(Self::deserialize_value(r)?);
                }
                Ok(Value::Array(Arc::new(RwLock::new(items))))
            }
            9 => {
                let map_len = read_u32(r)? as usize;
                let mut map = IndexMap::new();
                for _ in 0..map_len {
                    let key = read_string(r)?;
                    let val = Self::deserialize_value(r)?;
                    map.insert(key, val);
                }
                Ok(Value::Map(Arc::new(RwLock::new(map))))
            }
            other => Err(format!("Corrupt bytecode: unrecognized constant tag {}", other)),
        }
    }
}

fn read_u8<R: Read>(r: &mut R) -> Result<u8, String> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).map_err(|e| format!("Unexpected EOF or read error: {}", e))?;
    Ok(b[0])
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| format!("Unexpected EOF or read error: {}", e))?;
    Ok(u32::from_le_bytes(b))
}

fn read_string<R: Read>(r: &mut R) -> Result<String, String> {
    let len = read_u32(r)? as usize;
    if len > 50_000_000 {
        return Err("String in bytecode exceeds 50MB limit".to_string());
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).map_err(|e| format!("Failed to read string data: {}", e))?;
    String::from_utf8(buf).map_err(|e| format!("Invalid UTF-8 in bytecode string: {}", e))
}
