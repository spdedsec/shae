use crate::opcode::OpCode;
use crate::value::Value;

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
}
