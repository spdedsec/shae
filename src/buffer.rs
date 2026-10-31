use std::sync::{Arc, RwLock};
use crate::value::Value;

#[derive(Debug, Clone)]
pub struct ByteBuffer {
    pub storage: Arc<RwLock<Vec<u8>>>,
    pub offset: usize,
    pub length: usize,
}

impl ByteBuffer {
    pub fn alloc(size: usize) -> Self {
        Self {
            storage: Arc::new(RwLock::new(vec![0u8; size])),
            offset: 0,
            length: size,
        }
    }

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self {
            storage: Arc::new(RwLock::new(bytes)),
            offset: 0,
            length: len,
        }
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        Self::from_vec(slice.to_vec())
    }

    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let clean = hex.trim();
        let clean = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")).unwrap_or(clean);
        if clean.len() % 2 != 0 {
            return Err("Hex string must have an even number of digits".to_string());
        }
        let mut bytes = Vec::with_capacity(clean.len() / 2);
        for i in (0..clean.len()).step_by(2) {
            let byte = u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex byte '{}': {}", &clean[i..i + 2], e))?;
            bytes.push(byte);
        }
        Ok(Self::from_vec(bytes))
    }

    pub fn from_utf8(text: &str) -> Self {
        Self::from_slice(text.as_bytes())
    }

    pub fn concat(buffers: &[ByteBuffer]) -> Self {
        let total_len: usize = buffers.iter().map(|b| b.length).sum();
        let mut combined = Vec::with_capacity(total_len);
        for b in buffers {
            b.with_bytes(|slice| {
                combined.extend_from_slice(slice);
            });
        }
        Self::from_vec(combined)
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let guard = self.storage.read().unwrap();
        let start = self.offset.min(guard.len());
        let end = (self.offset + self.length).min(guard.len());
        f(&guard[start..end])
    }

    pub fn with_bytes_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let mut guard = self.storage.write().unwrap();
        let start = self.offset.min(guard.len());
        let end = (self.offset + self.length).min(guard.len());
        f(&mut guard[start..end])
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.with_bytes(|s| s.to_vec())
    }

    pub fn to_hex(&self) -> String {
        self.with_bytes(|slice| {
            let mut out = String::with_capacity(slice.len() * 2);
            for b in slice {
                use std::fmt::Write;
                let _ = write!(&mut out, "{:02x}", b);
            }
            out
        })
    }

    pub fn to_utf8(&self) -> Result<String, String> {
        self.with_bytes(|slice| String::from_utf8(slice.to_vec()).map_err(|e| e.to_string()))
    }

    pub fn copy(&self) -> Self {
        Self::from_vec(self.to_bytes())
    }

    pub fn slice(&self, start: usize, end: Option<usize>) -> Result<Self, String> {
        if start > self.length {
            return Err(format!(
                "Slice start {} out of bounds for buffer of length {}",
                start, self.length
            ));
        }
        let actual_end = end.unwrap_or(self.length);
        if actual_end < start {
            return Err(format!(
                "Slice end {} cannot be less than start {}",
                actual_end, start
            ));
        }
        if actual_end > self.length {
            return Err(format!(
                "Slice end {} out of bounds for buffer of length {}",
                actual_end, self.length
            ));
        }
        let slice_len = actual_end - start;
        let new_offset = self.offset + start;
        Ok(Self {
            storage: Arc::clone(&self.storage),
            offset: new_offset,
            length: slice_len,
        })
    }

    pub fn get_byte(&self, index: usize) -> Result<u8, String> {
        if index >= self.length {
            return Err(format!(
                "Index {} out of bounds for buffer of length {}",
                index, self.length
            ));
        }
        let guard = self.storage.read().unwrap();
        Ok(guard[self.offset + index])
    }

    pub fn set_byte(&mut self, index: usize, val: u8) -> Result<(), String> {
        if index >= self.length {
            return Err(format!(
                "Index {} out of bounds for buffer of length {}",
                index, self.length
            ));
        }
        let mut guard = self.storage.write().unwrap();
        guard[self.offset + index] = val;
        Ok(())
    }

    fn check_bounds(&self, pos: usize, size: usize) -> Result<(), String> {
        if pos.checked_add(size).map_or(true, |end| end > self.length) {
            Err(format!(
                "Out of bounds: attempt to access {} bytes at offset {} in buffer of length {}",
                size, pos, self.length
            ))
        } else {
            Ok(())
        }
    }

    // --- Endian Reads ---
    pub fn read_u8(&self, pos: usize) -> Result<u8, String> {
        self.check_bounds(pos, 1)?;
        let guard = self.storage.read().unwrap();
        Ok(guard[self.offset + pos])
    }

    pub fn read_i8(&self, pos: usize) -> Result<i8, String> {
        self.check_bounds(pos, 1)?;
        let guard = self.storage.read().unwrap();
        Ok(guard[self.offset + pos] as i8)
    }

    pub fn read_u16_le(&self, pos: usize) -> Result<u16, String> {
        self.check_bounds(pos, 2)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(u16::from_le_bytes([guard[idx], guard[idx + 1]]))
    }

    pub fn read_u16_be(&self, pos: usize) -> Result<u16, String> {
        self.check_bounds(pos, 2)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(u16::from_be_bytes([guard[idx], guard[idx + 1]]))
    }

    pub fn read_i16_le(&self, pos: usize) -> Result<i16, String> {
        self.check_bounds(pos, 2)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(i16::from_le_bytes([guard[idx], guard[idx + 1]]))
    }

    pub fn read_i16_be(&self, pos: usize) -> Result<i16, String> {
        self.check_bounds(pos, 2)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(i16::from_be_bytes([guard[idx], guard[idx + 1]]))
    }

    pub fn read_u32_le(&self, pos: usize) -> Result<u32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(u32::from_le_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_u32_be(&self, pos: usize) -> Result<u32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(u32::from_be_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_i32_le(&self, pos: usize) -> Result<i32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(i32::from_le_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_i32_be(&self, pos: usize) -> Result<i32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(i32::from_be_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_u64_le(&self, pos: usize) -> Result<u64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(u64::from_le_bytes(b))
    }

    pub fn read_u64_be(&self, pos: usize) -> Result<u64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(u64::from_be_bytes(b))
    }

    pub fn read_i64_le(&self, pos: usize) -> Result<i64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(i64::from_le_bytes(b))
    }

    pub fn read_i64_be(&self, pos: usize) -> Result<i64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(i64::from_be_bytes(b))
    }

    pub fn read_f32_le(&self, pos: usize) -> Result<f32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(f32::from_le_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_f32_be(&self, pos: usize) -> Result<f32, String> {
        self.check_bounds(pos, 4)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        Ok(f32::from_be_bytes([guard[idx], guard[idx + 1], guard[idx + 2], guard[idx + 3]]))
    }

    pub fn read_f64_le(&self, pos: usize) -> Result<f64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(f64::from_le_bytes(b))
    }

    pub fn read_f64_be(&self, pos: usize) -> Result<f64, String> {
        self.check_bounds(pos, 8)?;
        let guard = self.storage.read().unwrap();
        let idx = self.offset + pos;
        let mut b = [0u8; 8];
        b.copy_from_slice(&guard[idx..idx + 8]);
        Ok(f64::from_be_bytes(b))
    }

    // --- Endian Writes ---
    pub fn write_u8(&mut self, pos: usize, val: u8) -> Result<(), String> {
        self.check_bounds(pos, 1)?;
        let mut guard = self.storage.write().unwrap();
        guard[self.offset + pos] = val;
        Ok(())
    }

    pub fn write_i8(&mut self, pos: usize, val: i8) -> Result<(), String> {
        self.write_u8(pos, val as u8)
    }

    pub fn write_u16_le(&mut self, pos: usize, val: u16) -> Result<(), String> {
        self.check_bounds(pos, 2)?;
        let bytes = val.to_le_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx] = bytes[0];
        guard[idx + 1] = bytes[1];
        Ok(())
    }

    pub fn write_u16_be(&mut self, pos: usize, val: u16) -> Result<(), String> {
        self.check_bounds(pos, 2)?;
        let bytes = val.to_be_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx] = bytes[0];
        guard[idx + 1] = bytes[1];
        Ok(())
    }

    pub fn write_i16_le(&mut self, pos: usize, val: i16) -> Result<(), String> {
        self.write_u16_le(pos, val as u16)
    }

    pub fn write_i16_be(&mut self, pos: usize, val: i16) -> Result<(), String> {
        self.write_u16_be(pos, val as u16)
    }

    pub fn write_u32_le(&mut self, pos: usize, val: u32) -> Result<(), String> {
        self.check_bounds(pos, 4)?;
        let bytes = val.to_le_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_u32_be(&mut self, pos: usize, val: u32) -> Result<(), String> {
        self.check_bounds(pos, 4)?;
        let bytes = val.to_be_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_i32_le(&mut self, pos: usize, val: i32) -> Result<(), String> {
        self.write_u32_le(pos, val as u32)
    }

    pub fn write_i32_be(&mut self, pos: usize, val: i32) -> Result<(), String> {
        self.write_u32_be(pos, val as u32)
    }

    pub fn write_u64_le(&mut self, pos: usize, val: u64) -> Result<(), String> {
        self.check_bounds(pos, 8)?;
        let bytes = val.to_le_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 8].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_u64_be(&mut self, pos: usize, val: u64) -> Result<(), String> {
        self.check_bounds(pos, 8)?;
        let bytes = val.to_be_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 8].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_i64_le(&mut self, pos: usize, val: i64) -> Result<(), String> {
        self.write_u64_le(pos, val as u64)
    }

    pub fn write_i64_be(&mut self, pos: usize, val: i64) -> Result<(), String> {
        self.write_u64_be(pos, val as u64)
    }

    pub fn write_f32_le(&mut self, pos: usize, val: f32) -> Result<(), String> {
        self.check_bounds(pos, 4)?;
        let bytes = val.to_le_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_f32_be(&mut self, pos: usize, val: f32) -> Result<(), String> {
        self.check_bounds(pos, 4)?;
        let bytes = val.to_be_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_f64_le(&mut self, pos: usize, val: f64) -> Result<(), String> {
        self.check_bounds(pos, 8)?;
        let bytes = val.to_le_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 8].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_f64_be(&mut self, pos: usize, val: f64) -> Result<(), String> {
        self.check_bounds(pos, 8)?;
        let bytes = val.to_be_bytes();
        let mut guard = self.storage.write().unwrap();
        let idx = self.offset + pos;
        guard[idx..idx + 8].copy_from_slice(&bytes);
        Ok(())
    }
}

pub fn is_buffer_method(method: &str) -> bool {
    matches!(
        method,
        "slice"
            | "copy"
            | "to_hex"
            | "toHex"
            | "to_utf8"
            | "toUtf8"
            | "to_bytes"
            | "toBytes"
            | "read_u8"
            | "readU8"
            | "write_u8"
            | "writeU8"
            | "read_i8"
            | "readI8"
            | "write_i8"
            | "writeI8"
            | "read_u16_le"
            | "readU16LE"
            | "read_u16LE"
            | "readU16Le"
            | "read_u16_be"
            | "readU16BE"
            | "read_u16BE"
            | "readU16Be"
            | "write_u16_le"
            | "writeU16LE"
            | "write_u16LE"
            | "writeU16Le"
            | "write_u16_be"
            | "writeU16BE"
            | "write_u16BE"
            | "writeU16Be"
            | "read_i16_le"
            | "readI16LE"
            | "read_i16LE"
            | "readI16Le"
            | "read_i16_be"
            | "readI16BE"
            | "read_i16BE"
            | "readI16Be"
            | "write_i16_le"
            | "writeI16LE"
            | "write_i16LE"
            | "writeI16Le"
            | "write_i16_be"
            | "writeI16BE"
            | "write_i16BE"
            | "writeI16Be"
            | "read_u32_le"
            | "readU32LE"
            | "read_u32LE"
            | "readU32Le"
            | "read_u32_be"
            | "readU32BE"
            | "read_u32BE"
            | "readU32Be"
            | "write_u32_le"
            | "writeU32LE"
            | "write_u32LE"
            | "writeU32Le"
            | "write_u32_be"
            | "writeU32BE"
            | "write_u32BE"
            | "writeU32Be"
            | "read_i32_le"
            | "readI32LE"
            | "read_i32LE"
            | "readI32Le"
            | "read_i32_be"
            | "readI32BE"
            | "read_i32BE"
            | "readI32Be"
            | "write_i32_le"
            | "writeI32LE"
            | "write_i32LE"
            | "writeI32Le"
            | "write_i32_be"
            | "writeI32BE"
            | "write_i32BE"
            | "writeI32Be"
            | "read_u64_le"
            | "readU64LE"
            | "read_u64LE"
            | "readU64Le"
            | "read_u64_be"
            | "readU64BE"
            | "read_u64BE"
            | "readU64Be"
            | "write_u64_le"
            | "writeU64LE"
            | "write_u64LE"
            | "writeU64Le"
            | "write_u64_be"
            | "writeU64BE"
            | "write_u64BE"
            | "writeU64Be"
            | "read_i64_le"
            | "readI64LE"
            | "read_i64LE"
            | "readI64Le"
            | "read_i64_be"
            | "readI64BE"
            | "read_i64BE"
            | "readI64Be"
            | "write_i64_le"
            | "writeI64LE"
            | "write_i64LE"
            | "writeI64Le"
            | "write_i64_be"
            | "writeI64BE"
            | "write_i64BE"
            | "writeI64Be"
            | "read_f32_le"
            | "readF32LE"
            | "read_f32LE"
            | "readF32Le"
            | "read_f32_be"
            | "readF32BE"
            | "read_f32BE"
            | "readF32Be"
            | "write_f32_le"
            | "writeF32LE"
            | "write_f32LE"
            | "writeF32Le"
            | "write_f32_be"
            | "writeF32BE"
            | "write_f32BE"
            | "writeF32Be"
            | "read_f64_le"
            | "readF64LE"
            | "read_f64LE"
            | "readF64Le"
            | "read_f64_be"
            | "readF64BE"
            | "read_f64BE"
            | "readF64Be"
            | "write_f64_le"
            | "writeF64LE"
            | "write_f64LE"
            | "writeF64Le"
            | "write_f64_be"
            | "writeF64BE"
            | "write_f64BE"
            | "writeF64Be"
    )
}

// =========================================================================
// Struct Packing & Layout Engine
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    Pad,                 // 'x'
    Char,                // 'c'
    Int8,                // 'b'
    UInt8,               // 'B'
    Bool,                // '?'
    Int16,               // 'h'
    UInt16,              // 'H'
    Int32,               // 'i' / 'l'
    UInt32,              // 'I' / 'L'
    Int64,               // 'q'
    UInt64,              // 'Q'
    Float32,             // 'f'
    Float64,             // 'd'
    FixedString(usize),  // 's' with byte length
    PascalString(usize), // 'p' with byte length
}

impl FieldType {
    pub fn size(&self) -> usize {
        match self {
            FieldType::Pad | FieldType::Char | FieldType::Int8 | FieldType::UInt8 | FieldType::Bool => 1,
            FieldType::Int16 | FieldType::UInt16 => 2,
            FieldType::Int32 | FieldType::UInt32 | FieldType::Float32 => 4,
            FieldType::Int64 | FieldType::UInt64 | FieldType::Float64 => 8,
            FieldType::FixedString(len) | FieldType::PascalString(len) => *len,
        }
    }

    pub fn consumes_value(&self) -> bool {
        !matches!(self, FieldType::Pad)
    }
}

pub struct StructEngine;

impl StructEngine {
    pub fn parse_format(fmt: &str) -> Result<(Endianness, Vec<FieldType>), String> {
        let mut chars = fmt.chars().peekable();
        let mut endianness = Endianness::Little;

        if let Some(&ch) = chars.peek() {
            match ch {
                '<' => {
                    endianness = Endianness::Little;
                    chars.next();
                }
                '>' | '!' => {
                    endianness = Endianness::Big;
                    chars.next();
                }
                '=' | '@' => {
                    #[cfg(target_endian = "big")]
                    {
                        endianness = Endianness::Big;
                    }
                    #[cfg(not(target_endian = "big"))]
                    {
                        endianness = Endianness::Little;
                    }
                    chars.next();
                }
                _ => {}
            }
        }

        let mut fields = Vec::new();
        while let Some(ch) = chars.next() {
            if ch.is_whitespace() {
                continue;
            }

            let mut count: Option<usize> = None;
            let field_char = if ch.is_ascii_digit() {
                let mut num_str = String::new();
                num_str.push(ch);
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_digit() {
                        num_str.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let parsed_count: usize = num_str
                    .parse()
                    .map_err(|e| format!("Invalid count in struct format: {}", e))?;
                count = Some(parsed_count);

                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_whitespace() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                match chars.next() {
                    Some(c) => c,
                    None => {
                        return Err("Format string ended prematurely after repeat count".to_string());
                    }
                }
            } else {
                ch
            };

            match field_char {
                's' => {
                    let len = count.unwrap_or(1);
                    fields.push(FieldType::FixedString(len));
                }
                'p' => {
                    let len = count.unwrap_or(1);
                    fields.push(FieldType::PascalString(len));
                }
                other => {
                    let repeat = count.unwrap_or(1);
                    let single_field = match other {
                        'x' => FieldType::Pad,
                        'c' => FieldType::Char,
                        'b' => FieldType::Int8,
                        'B' => FieldType::UInt8,
                        '?' => FieldType::Bool,
                        'h' => FieldType::Int16,
                        'H' => FieldType::UInt16,
                        'i' | 'l' => FieldType::Int32,
                        'I' | 'L' => FieldType::UInt32,
                        'q' => FieldType::Int64,
                        'Q' => FieldType::UInt64,
                        'f' => FieldType::Float32,
                        'd' => FieldType::Float64,
                        unknown => {
                            return Err(format!("Unknown struct format specifier '{}'", unknown));
                        }
                    };
                    for _ in 0..repeat {
                        fields.push(single_field.clone());
                    }
                }
            }
        }

        Ok((endianness, fields))
    }

    pub fn calcsize(fmt: &str) -> Result<usize, String> {
        let (_, fields) = Self::parse_format(fmt)?;
        Ok(fields.iter().map(|f| f.size()).sum())
    }

    pub fn pack(fmt: &str, values: &[Value]) -> Result<ByteBuffer, String> {
        let (endianness, fields) = Self::parse_format(fmt)?;
        let total_size: usize = fields.iter().map(|f| f.size()).sum();
        let expected_values = fields.iter().filter(|f| f.consumes_value()).count();
        if values.len() != expected_values {
            return Err(format!(
                "pack expected {} values for format '{}', but got {}",
                expected_values,
                fmt,
                values.len()
            ));
        }

        let mut out = vec![0u8; total_size];
        let mut byte_pos = 0;
        let mut val_idx = 0;

        for field in &fields {
            match field {
                FieldType::Pad => {
                    out[byte_pos] = 0;
                    byte_pos += 1;
                }
                FieldType::Char => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let byte = match val {
                        Value::String(s) => s.as_bytes().first().copied().unwrap_or(0),
                        Value::Int(i) => *i as u8,
                        other => {
                            return Err(format!("Expected char/string for 'c', got {}", other.type_name()));
                        }
                    };
                    out[byte_pos] = byte;
                    byte_pos += 1;
                }
                FieldType::Int8 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)?;
                    let byte = (n as i8) as u8;
                    out[byte_pos] = byte;
                    byte_pos += 1;
                }
                FieldType::UInt8 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)?;
                    out[byte_pos] = n as u8;
                    byte_pos += 1;
                }
                FieldType::Bool => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    out[byte_pos] = if val.is_truthy() { 1 } else { 0 };
                    byte_pos += 1;
                }
                FieldType::Int16 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)? as i16;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 2].copy_from_slice(&b);
                    byte_pos += 2;
                }
                FieldType::UInt16 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)? as u16;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 2].copy_from_slice(&b);
                    byte_pos += 2;
                }
                FieldType::Int32 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)? as i32;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 4].copy_from_slice(&b);
                    byte_pos += 4;
                }
                FieldType::UInt32 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)? as u32;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 4].copy_from_slice(&b);
                    byte_pos += 4;
                }
                FieldType::Int64 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)?;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 8].copy_from_slice(&b);
                    byte_pos += 8;
                }
                FieldType::UInt64 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let n = value_to_i64(val)? as u64;
                    let b = match endianness {
                        Endianness::Little => n.to_le_bytes(),
                        Endianness::Big => n.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 8].copy_from_slice(&b);
                    byte_pos += 8;
                }
                FieldType::Float32 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let f = value_to_f64(val)? as f32;
                    let b = match endianness {
                        Endianness::Little => f.to_le_bytes(),
                        Endianness::Big => f.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 4].copy_from_slice(&b);
                    byte_pos += 4;
                }
                FieldType::Float64 => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let f = value_to_f64(val)?;
                    let b = match endianness {
                        Endianness::Little => f.to_le_bytes(),
                        Endianness::Big => f.to_be_bytes(),
                    };
                    out[byte_pos..byte_pos + 8].copy_from_slice(&b);
                    byte_pos += 8;
                }
                FieldType::FixedString(len) => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let bytes = value_to_bytes(val)?;
                    let copy_len = bytes.len().min(*len);
                    out[byte_pos..byte_pos + copy_len].copy_from_slice(&bytes[..copy_len]);
                    // Pad remaining with null bytes if shorter
                    for p in (byte_pos + copy_len)..(byte_pos + len) {
                        out[p] = 0;
                    }
                    byte_pos += len;
                }
                FieldType::PascalString(len) => {
                    let val = &values[val_idx];
                    val_idx += 1;
                    let bytes = value_to_bytes(val)?;
                    let max_str_len = len.saturating_sub(1);
                    let actual_len = bytes.len().min(max_str_len);
                    out[byte_pos] = actual_len as u8;
                    byte_pos += 1;
                    out[byte_pos..byte_pos + actual_len].copy_from_slice(&bytes[..actual_len]);
                    for p in (byte_pos + actual_len)..(byte_pos + max_str_len) {
                        out[p] = 0;
                    }
                    byte_pos += max_str_len;
                }
            }
        }

        Ok(ByteBuffer::from_vec(out))
    }

    pub fn pack_into(fmt: &str, buf: &mut ByteBuffer, offset: usize, values: &[Value]) -> Result<usize, String> {
        let packed = Self::pack(fmt, values)?;
        let packed_len = packed.len();
        buf.check_bounds(offset, packed_len)?;
        let mut guard = buf.storage.write().unwrap();
        let target_idx = buf.offset + offset;
        packed.with_bytes(|src| {
            guard[target_idx..target_idx + packed_len].copy_from_slice(src);
        });
        Ok(packed_len)
    }

    pub fn unpack(fmt: &str, data: &[u8], offset: usize) -> Result<Vec<Value>, String> {
        let (endianness, fields) = Self::parse_format(fmt)?;
        let total_size: usize = fields.iter().map(|f| f.size()).sum();
        if offset.checked_add(total_size).map_or(true, |end| end > data.len()) {
            return Err(format!(
                "unpack requires a buffer of at least {} bytes at offset {}, but buffer length is {}",
                total_size, offset, data.len()
            ));
        }

        let mut results = Vec::new();
        let mut cur = offset;

        for field in &fields {
            match field {
                FieldType::Pad => {
                    cur += 1;
                }
                FieldType::Char => {
                    let b = data[cur];
                    cur += 1;
                    results.push(Value::String((b as char).to_string()));
                }
                FieldType::Int8 => {
                    let b = data[cur] as i8;
                    cur += 1;
                    results.push(Value::Int(b as i64));
                }
                FieldType::UInt8 => {
                    let b = data[cur];
                    cur += 1;
                    results.push(Value::Int(b as i64));
                }
                FieldType::Bool => {
                    let b = data[cur];
                    cur += 1;
                    results.push(Value::Bool(b != 0));
                }
                FieldType::Int16 => {
                    let b = [data[cur], data[cur + 1]];
                    cur += 2;
                    let val = match endianness {
                        Endianness::Little => i16::from_le_bytes(b),
                        Endianness::Big => i16::from_be_bytes(b),
                    };
                    results.push(Value::Int(val as i64));
                }
                FieldType::UInt16 => {
                    let b = [data[cur], data[cur + 1]];
                    cur += 2;
                    let val = match endianness {
                        Endianness::Little => u16::from_le_bytes(b),
                        Endianness::Big => u16::from_be_bytes(b),
                    };
                    results.push(Value::Int(val as i64));
                }
                FieldType::Int32 => {
                    let b = [data[cur], data[cur + 1], data[cur + 2], data[cur + 3]];
                    cur += 4;
                    let val = match endianness {
                        Endianness::Little => i32::from_le_bytes(b),
                        Endianness::Big => i32::from_be_bytes(b),
                    };
                    results.push(Value::Int(val as i64));
                }
                FieldType::UInt32 => {
                    let b = [data[cur], data[cur + 1], data[cur + 2], data[cur + 3]];
                    cur += 4;
                    let val = match endianness {
                        Endianness::Little => u32::from_le_bytes(b),
                        Endianness::Big => u32::from_be_bytes(b),
                    };
                    results.push(Value::Int(val as i64));
                }
                FieldType::Int64 => {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[cur..cur + 8]);
                    cur += 8;
                    let val = match endianness {
                        Endianness::Little => i64::from_le_bytes(b),
                        Endianness::Big => i64::from_be_bytes(b),
                    };
                    results.push(Value::Int(val));
                }
                FieldType::UInt64 => {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[cur..cur + 8]);
                    cur += 8;
                    let val = match endianness {
                        Endianness::Little => u64::from_le_bytes(b),
                        Endianness::Big => u64::from_be_bytes(b),
                    };
                    results.push(Value::Int(val as i64));
                }
                FieldType::Float32 => {
                    let b = [data[cur], data[cur + 1], data[cur + 2], data[cur + 3]];
                    cur += 4;
                    let val = match endianness {
                        Endianness::Little => f32::from_le_bytes(b),
                        Endianness::Big => f32::from_be_bytes(b),
                    };
                    results.push(Value::Float(val as f64));
                }
                FieldType::Float64 => {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[cur..cur + 8]);
                    cur += 8;
                    let val = match endianness {
                        Endianness::Little => f64::from_le_bytes(b),
                        Endianness::Big => f64::from_be_bytes(b),
                    };
                    results.push(Value::Float(val));
                }
                FieldType::FixedString(len) => {
                    let slice = &data[cur..cur + len];
                    cur += len;
                    let s = String::from_utf8_lossy(slice).into_owned();
                    results.push(Value::String(s));
                }
                FieldType::PascalString(len) => {
                    let p_len = data[cur] as usize;
                    cur += 1;
                    let actual_len = p_len.min(len.saturating_sub(1));
                    let slice = &data[cur..cur + actual_len];
                    cur += len.saturating_sub(1);
                    let s = String::from_utf8_lossy(slice).into_owned();
                    results.push(Value::String(s));
                }
            }
        }

        Ok(results)
    }
}

fn value_to_i64(val: &Value) -> Result<i64, String> {
    match val {
        Value::Int(i) => Ok(*i),
        Value::Float(f) | Value::Number(f) => {
            if f.fract() == 0.0 {
                Ok(*f as i64)
            } else {
                Err(format!("Expected integer, got float {}", f))
            }
        }
        other => Err(format!("Expected integer, got {}", other.type_name())),
    }
}

fn value_to_f64(val: &Value) -> Result<f64, String> {
    match val {
        Value::Float(f) | Value::Number(f) => Ok(*f),
        Value::Int(i) => Ok(*i as f64),
        other => Err(format!("Expected float number, got {}", other.type_name())),
    }
}

fn value_to_bytes(val: &Value) -> Result<Vec<u8>, String> {
    match val {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::ByteBuffer(b) => Ok(b.read().unwrap().to_bytes()),
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut out = Vec::with_capacity(guard.len());
            for v in guard.iter() {
                let n = value_to_i64(v)?;
                if (0..=255).contains(&n) {
                    out.push(n as u8);
                } else {
                    return Err(format!("Byte value {} out of range 0..255", n));
                }
            }
            Ok(out)
        }
        other => Err(format!(
            "Expected string, buffer, or byte array, got {}",
            other.type_name()
        )),
    }
}
