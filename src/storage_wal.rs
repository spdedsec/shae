// Shae Storage Engine: High-Performance Write-Ahead Log (WAL)
// Part of Option 5: Embedded LSM-Tree & High-Speed Storage Engine

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};

// ---------------------------------------------------------------------------
// CRC-32 (IEEE 802.3 standard: polynomial 0xEDB88320)
// ---------------------------------------------------------------------------

fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut c = i as u32;
        for _ in 0..8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
        }
        table[i] = c;
    }
    table
}

static CRC_TABLE: LazyLock<[u32; 256]> = LazyLock::new(make_crc_table);

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        let table_idx = ((crc ^ (b as u32)) & 0xFF) as usize;
        crc = CRC_TABLE[table_idx] ^ (crc >> 8);
    }
    !crc
}

// ---------------------------------------------------------------------------
// WAL Record Types & Framing
// ---------------------------------------------------------------------------

pub const WAL_MAGIC: [u8; 4] = [0x53, 0x48, 0x41, 0x45]; // "SHAE"
pub const WAL_HEADER_SIZE: usize = 25; // 4 (magic) + 4 (crc) + 1 (type) + 8 (lsn) + 4 (klen) + 4 (vlen)
pub const DEFAULT_MAX_SEGMENT_SIZE: usize = 4 * 1024 * 1024; // 4MB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalRecordType {
    Put = 1,
    Delete = 2,
    Checkpoint = 3,
    CommitTx = 4,
    AbortTx = 5,
}

impl WalRecordType {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(WalRecordType::Put),
            2 => Some(WalRecordType::Delete),
            3 => Some(WalRecordType::Checkpoint),
            4 => Some(WalRecordType::CommitTx),
            5 => Some(WalRecordType::AbortTx),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            WalRecordType::Put => "PUT",
            WalRecordType::Delete => "DELETE",
            WalRecordType::Checkpoint => "CHECKPOINT",
            WalRecordType::CommitTx => "COMMIT_TX",
            WalRecordType::AbortTx => "ABORT_TX",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "PUT" => Some(WalRecordType::Put),
            "DELETE" | "DEL" => Some(WalRecordType::Delete),
            "CHECKPOINT" => Some(WalRecordType::Checkpoint),
            "COMMIT" | "COMMIT_TX" => Some(WalRecordType::CommitTx),
            "ABORT" | "ABORT_TX" => Some(WalRecordType::AbortTx),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecord {
    pub lsn: u64,
    pub record_type: WalRecordType,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

impl WalRecord {
    pub fn encode(&self) -> Vec<u8> {
        let key_len = self.key.len() as u32;
        let val_len = self.value.len() as u32;
        let total_size = WAL_HEADER_SIZE + self.key.len() + self.value.len();
        let mut buf = Vec::with_capacity(total_size);

        // Compute CRC over type + lsn + key_len + val_len + key + value
        let mut crc_payload = Vec::with_capacity(1 + 8 + 4 + 4 + self.key.len() + self.value.len());
        crc_payload.push(self.record_type as u8);
        crc_payload.extend_from_slice(&self.lsn.to_le_bytes());
        crc_payload.extend_from_slice(&key_len.to_le_bytes());
        crc_payload.extend_from_slice(&val_len.to_le_bytes());
        crc_payload.extend_from_slice(&self.key);
        crc_payload.extend_from_slice(&self.value);

        let checksum = crc32(&crc_payload);

        // Header: Magic (4B) + CRC (4B) + Type (1B) + LSN (8B) + KeyLen (4B) + ValLen (4B)
        buf.extend_from_slice(&WAL_MAGIC);
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf.push(self.record_type as u8);
        buf.extend_from_slice(&self.lsn.to_le_bytes());
        buf.extend_from_slice(&key_len.to_le_bytes());
        buf.extend_from_slice(&val_len.to_le_bytes());
        buf.extend_from_slice(&self.key);
        buf.extend_from_slice(&self.value);

        buf
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("lsn".to_string(), Value::Int(self.lsn as i64));
        map.insert("type".to_string(), Value::String(self.record_type.as_str().to_string()));
        map.insert(
            "key".to_string(),
            Value::String(String::from_utf8_lossy(&self.key).to_string()),
        );
        map.insert(
            "value".to_string(),
            Value::String(String::from_utf8_lossy(&self.value).to_string()),
        );
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

// ---------------------------------------------------------------------------
// WAL Error Type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum WalError {
    Io(io::Error),
    CorruptedRecord(String),
    InvalidMagic,
    ChecksumMismatch { expected: u32, computed: u32 },
    NotFound(String),
}

impl From<io::Error> for WalError {
    fn from(e: io::Error) -> Self {
        WalError::Io(e)
    }
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::Io(e) => write!(f, "WAL I/O error: {}", e),
            WalError::CorruptedRecord(msg) => write!(f, "WAL corrupted record: {}", msg),
            WalError::InvalidMagic => write!(f, "WAL invalid magic bytes"),
            WalError::ChecksumMismatch { expected, computed } => {
                write!(f, "WAL CRC32 mismatch: expected {:08x}, computed {:08x}", expected, computed)
            }
            WalError::NotFound(s) => write!(f, "WAL not found: {}", s),
        }
    }
}

impl std::error::Error for WalError {}

// ---------------------------------------------------------------------------
// WAL Options & Segments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WalOptions {
    pub max_segment_size: usize,
    pub sync_always: bool,
}

impl Default for WalOptions {
    fn default() -> Self {
        Self {
            max_segment_size: DEFAULT_MAX_SEGMENT_SIZE,
            sync_always: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalSegmentMeta {
    pub path: PathBuf,
    pub start_lsn: u64,
    pub end_lsn: u64,
    pub size: u64,
}

// ---------------------------------------------------------------------------
// WAL Core Implementation
// ---------------------------------------------------------------------------

pub struct Wal {
    pub dir: PathBuf,
    pub options: WalOptions,
    pub next_lsn: u64,
    pub active_file: Option<File>,
    pub active_segment_start_lsn: u64,
    pub active_segment_size: usize,
    pub segments: Vec<WalSegmentMeta>,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(dir: P, options: WalOptions) -> Result<Self, WalError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        // Discover and recover segments to find the highest LSN
        let (records, next_lsn) = Self::recover(&dir)?;
        let mut segments = Self::list_segments(&dir)?;

        let (active_file, active_start_lsn, active_size) = if let Some(last_seg) = segments.last() {
            let path = &last_seg.path;
            let file = OpenOptions::new().create(true).append(true).read(true).open(path)?;
            let meta = file.metadata()?;
            (Some(file), last_seg.start_lsn, meta.len() as usize)
        } else {
            // First segment
            let start_lsn = 1;
            let seg_path = dir.join(format!("wal_{:016x}.seg", start_lsn));
            let file = OpenOptions::new().create(true).append(true).read(true).open(&seg_path)?;
            segments.push(WalSegmentMeta {
                path: seg_path,
                start_lsn,
                end_lsn: start_lsn,
                size: 0,
            });
            (Some(file), start_lsn, 0)
        };

        let _ = records.len();

        Ok(Wal {
            dir,
            options,
            next_lsn,
            active_file,
            active_segment_start_lsn: active_start_lsn,
            active_segment_size: active_size,
            segments,
        })
    }

    pub fn append(&mut self, record_type: WalRecordType, key: &[u8], value: &[u8]) -> Result<u64, WalError> {
        let lsn = self.next_lsn;
        let record = WalRecord {
            lsn,
            record_type,
            key: key.to_vec(),
            value: value.to_vec(),
        };

        let encoded = record.encode();

        // Check if segment rotation is necessary
        if self.active_segment_size + encoded.len() > self.options.max_segment_size
            && self.active_segment_size > 0
        {
            self.rotate_segment()?;
        }

        let file = self.active_file.as_mut().ok_or_else(|| {
            WalError::Io(io::Error::new(io::ErrorKind::Other, "No active WAL file"))
        })?;

        file.write_all(&encoded)?;
        self.active_segment_size += encoded.len();

        if self.options.sync_always {
            file.sync_data()?;
        }

        if let Some(last) = self.segments.last_mut() {
            last.end_lsn = lsn;
            last.size = self.active_segment_size as u64;
        }

        self.next_lsn += 1;
        Ok(lsn)
    }

    pub fn sync(&mut self) -> Result<(), WalError> {
        if let Some(file) = &mut self.active_file {
            file.sync_all()?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), WalError> {
        self.sync()?;
        self.active_file = None;
        Ok(())
    }

    fn rotate_segment(&mut self) -> Result<(), WalError> {
        if let Some(file) = &mut self.active_file {
            file.sync_all()?;
        }

        let new_start_lsn = self.next_lsn;
        let new_seg_path = self.dir.join(format!("wal_{:016x}.seg", new_start_lsn));
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&new_seg_path)?;

        self.active_file = Some(new_file);
        self.active_segment_start_lsn = new_start_lsn;
        self.active_segment_size = 0;

        self.segments.push(WalSegmentMeta {
            path: new_seg_path,
            start_lsn: new_start_lsn,
            end_lsn: new_start_lsn,
            size: 0,
        });

        Ok(())
    }

    pub fn list_segments<P: AsRef<Path>>(dir: P) -> Result<Vec<WalSegmentMeta>, WalError> {
        let mut segments = Vec::new();
        if !dir.as_ref().exists() {
            return Ok(segments);
        }

        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("wal_") && name.ends_with(".seg") {
                        paths.push(path);
                    }
                }
            }
        }

        paths.sort();

        for path in paths {
            let filename = path.file_name().unwrap().to_str().unwrap();
            let hex_str = &filename[4..filename.len() - 4];
            let start_lsn = u64::from_str_radix(hex_str, 16).unwrap_or(0);
            let metadata = fs::metadata(&path)?;
            segments.push(WalSegmentMeta {
                path,
                start_lsn,
                end_lsn: start_lsn,
                size: metadata.len(),
            });
        }

        Ok(segments)
    }

    /// Read all valid records across segments and determine next LSN.
    /// In case of torn writes or partial crashes, corrupt suffixes are safely halted.
    pub fn recover<P: AsRef<Path>>(dir: P) -> Result<(Vec<WalRecord>, u64), WalError> {
        let segments = Self::list_segments(dir)?;
        let mut records = Vec::new();
        let mut max_lsn = 0u64;

        for seg in segments {
            let mut file = match File::open(&seg.path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let mut valid_offset = 0u64;
            loop {
                let mut header = [0u8; WAL_HEADER_SIZE];
                match file.read_exact(&mut header) {
                    Ok(()) => {}
                    Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                        // Clean end-of-file reached
                        break;
                    }
                    Err(e) => return Err(WalError::Io(e)),
                }

                // Verify magic
                if header[0..4] != WAL_MAGIC {
                    // Torn write or corrupted record; truncate or stop here
                    break;
                }

                let checksum = u32::from_le_bytes(header[4..8].try_into().unwrap());
                let rec_type_raw = header[8];
                let rec_type = match WalRecordType::from_u8(rec_type_raw) {
                    Some(t) => t,
                    None => break, // invalid record type
                };

                let lsn = u64::from_le_bytes(header[9..17].try_into().unwrap());
                let key_len = u32::from_le_bytes(header[17..21].try_into().unwrap()) as usize;
                let val_len = u32::from_le_bytes(header[21..25].try_into().unwrap()) as usize;

                let mut key = vec![0u8; key_len];
                if let Err(_) = file.read_exact(&mut key) {
                    break; // Partial write
                }

                let mut value = vec![0u8; val_len];
                if let Err(_) = file.read_exact(&mut value) {
                    break; // Partial write
                }

                // Verify CRC32
                let mut crc_payload = Vec::with_capacity(1 + 8 + 4 + 4 + key_len + val_len);
                crc_payload.push(rec_type_raw);
                crc_payload.extend_from_slice(&header[9..17]); // lsn
                crc_payload.extend_from_slice(&header[17..21]); // klen
                crc_payload.extend_from_slice(&header[21..25]); // vlen
                crc_payload.extend_from_slice(&key);
                crc_payload.extend_from_slice(&value);

                let computed_crc = crc32(&crc_payload);
                if checksum != computed_crc {
                    // Checksum mismatch, stop here cleanly
                    break;
                }

                valid_offset += (WAL_HEADER_SIZE + key_len + val_len) as u64;
                if lsn > max_lsn {
                    max_lsn = lsn;
                }

                records.push(WalRecord {
                    lsn,
                    record_type: rec_type,
                    key,
                    value,
                });
            }

            let _ = valid_offset;
        }

        let next_lsn = if max_lsn == 0 { 1 } else { max_lsn + 1 };
        Ok((records, next_lsn))
    }

    /// Purge segment files that exclusively contain records older than `min_lsn`.
    pub fn purge_before(&mut self, min_lsn: u64) -> Result<usize, WalError> {
        let mut purged_count = 0;
        let mut retained = Vec::new();

        // Never purge active segment
        let active_start = self.active_segment_start_lsn;

        for seg in self.segments.drain(..) {
            if seg.start_lsn < active_start && seg.end_lsn < min_lsn {
                let _ = fs::remove_file(&seg.path);
                purged_count += 1;
            } else {
                retained.push(seg);
            }
        }

        self.segments = retained;
        Ok(purged_count)
    }
}

// ---------------------------------------------------------------------------
// Global Registry for Shae Scripts
// ---------------------------------------------------------------------------

static NEXT_WAL_ID: AtomicUsize = AtomicUsize::new(1);
static WAL_INSTANCES: OnceLock<RwLock<HashMap<usize, Arc<Mutex<Wal>>>>> = OnceLock::new();

fn get_wal_registry() -> &'static RwLock<HashMap<usize, Arc<Mutex<Wal>>>> {
    WAL_INSTANCES.get_or_init(|| RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Standard Library Module (`std:db:wal` or `std:wal`)
// ---------------------------------------------------------------------------

pub fn build_wal_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("open", wal_open),
        ("append", wal_append),
        ("put", wal_put),
        ("delete", wal_delete),
        ("checkpoint", wal_checkpoint),
        ("sync", wal_sync),
        ("close", wal_close),
        ("recover", wal_recover),
        ("purge", wal_purge),
        ("wal_open", wal_open),
        ("wal_append", wal_append),
        ("wal_put", wal_put),
        ("wal_delete", wal_delete),
        ("wal_checkpoint", wal_checkpoint),
        ("wal_sync", wal_sync),
        ("wal_close", wal_close),
        ("wal_recover", wal_recover),
        ("wal_purge", wal_purge),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("wal.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn wal_open(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("wal.open requires at least directory path".to_string()).at(span));
    }

    let dir_str = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("wal.open: expected directory string".to_string()).at(span)),
    };

    let mut options = WalOptions::default();

    if args.len() > 1 {
        if let Value::Map(m) = &args[1] {
            let map = m.read().unwrap();
            if let Some(Value::Int(n)) = map.get("max_segment_size") {
                if *n > 0 {
                    options.max_segment_size = *n as usize;
                }
            }
            if let Some(Value::Bool(b)) = map.get("sync_always") {
                options.sync_always = *b;
            }
        }
    }

    let wal = Wal::open(&dir_str, options).map_err(|e| {
        RuntimeError::new(format!("wal.open failed: {}", e)).at(span)
    })?;

    let id = NEXT_WAL_ID.fetch_add(1, Ordering::SeqCst);
    get_wal_registry()
        .write()
        .unwrap()
        .insert(id, Arc::new(Mutex::new(wal)));

    Ok(Value::Int(id as i64))
}

fn wal_append(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.append", 4, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.append: expected wal id (int)".to_string()).at(span)),
    };

    let type_str = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("wal.append: expected record type string".to_string()).at(span)),
    };

    let rec_type = WalRecordType::from_str_name(&type_str).ok_or_else(|| {
        RuntimeError::new(format!("wal.append: unknown record type '{}'", type_str)).at(span)
    })?;

    let key_bytes = match &args[2] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.append: expected string key".to_string()).at(span)),
    };

    let val_bytes = match &args[3] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.append: expected string value".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.append: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    let lsn = wal.append(rec_type, &key_bytes, &val_bytes).map_err(|e| {
        RuntimeError::new(format!("wal.append failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(lsn as i64))
}

fn wal_put(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.put", 3, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.put: expected wal id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.put: expected string key".to_string()).at(span)),
    };

    let val_bytes = match &args[2] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.put: expected string value".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.put: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    let lsn = wal.append(WalRecordType::Put, &key_bytes, &val_bytes).map_err(|e| {
        RuntimeError::new(format!("wal.put failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(lsn as i64))
}

fn wal_delete(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.delete", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.delete: expected wal id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.delete: expected string key".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.delete: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    let lsn = wal.append(WalRecordType::Delete, &key_bytes, &[]).map_err(|e| {
        RuntimeError::new(format!("wal.delete failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(lsn as i64))
}

fn wal_checkpoint(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.checkpoint", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.checkpoint: expected wal id (int)".to_string()).at(span)),
    };

    let meta = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("wal.checkpoint: expected meta string".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.checkpoint: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    let lsn = wal.append(WalRecordType::Checkpoint, b"checkpoint", &meta).map_err(|e| {
        RuntimeError::new(format!("wal.checkpoint failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(lsn as i64))
}

fn wal_sync(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.sync", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.sync: expected wal id (int)".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.sync: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    wal.sync().map_err(|e| RuntimeError::new(format!("wal.sync failed: {}", e)).at(span))?;

    Ok(Value::Bool(true))
}

fn wal_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.close", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.close: expected wal id (int)".to_string()).at(span)),
    };

    let mut registry = get_wal_registry().write().unwrap();
    if let Some(wal_arc) = registry.remove(&id) {
        let mut wal = wal_arc.lock().unwrap();
        wal.close().map_err(|e| RuntimeError::new(format!("wal.close failed: {}", e)).at(span))?;
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}

fn wal_recover(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.recover", 1, &args, span)?;
    let dir_str = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("wal.recover: expected directory string".to_string()).at(span)),
    };

    let (records, _next_lsn) = Wal::recover(&dir_str).map_err(|e| {
        RuntimeError::new(format!("wal.recover failed: {}", e)).at(span)
    })?;

    let values: Vec<Value> = records.iter().map(|r| r.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(values))))
}

fn wal_purge(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("wal.purge", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("wal.purge: expected wal id (int)".to_string()).at(span)),
    };

    let min_lsn = match &args[1] {
        Value::Int(i) => *i as u64,
        _ => return Err(RuntimeError::new("wal.purge: expected min_lsn (int)".to_string()).at(span)),
    };

    let registry = get_wal_registry().read().unwrap();
    let wal_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("wal.purge: invalid wal id {}", id)).at(span)
    })?;

    let mut wal = wal_arc.lock().unwrap();
    let pruned = wal.purge_before(min_lsn).map_err(|e| {
        RuntimeError::new(format!("wal.purge failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(pruned as i64))
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(prefix: &str) -> Self {
            let p = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), fastrand::u64(..)));
            let _ = std::fs::create_dir_all(&p);
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }


    #[test]
    fn test_crc32_accuracy() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
        assert_eq!(crc32(b"shae-fast-systems-language"), crc32(b"shae-fast-systems-language"));
        assert_ne!(crc32(b"shae1"), crc32(b"shae2"));
    }

    #[test]
    fn test_wal_append_and_recover() {
        let dir = TempDir::new("shae_wal");
        let wal_dir = dir.path().join("wal_test");

        {
            let mut wal = Wal::open(&wal_dir, WalOptions::default()).unwrap();
            let lsn1 = wal.append(WalRecordType::Put, b"user:101", b"Alice").unwrap();
            let lsn2 = wal.append(WalRecordType::Put, b"user:102", b"Bob").unwrap();
            let lsn3 = wal.append(WalRecordType::Delete, b"user:101", b"").unwrap();
            let lsn4 = wal.append(WalRecordType::Checkpoint, b"cp", b"snapshot_0").unwrap();

            assert_eq!(lsn1, 1);
            assert_eq!(lsn2, 2);
            assert_eq!(lsn3, 3);
            assert_eq!(lsn4, 4);

            wal.close().unwrap();
        }

        // Recover
        let (records, next_lsn) = Wal::recover(&wal_dir).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(next_lsn, 5);

        assert_eq!(records[0].lsn, 1);
        assert_eq!(records[0].record_type, WalRecordType::Put);
        assert_eq!(records[0].key, b"user:101");
        assert_eq!(records[0].value, b"Alice");

        assert_eq!(records[1].lsn, 2);
        assert_eq!(records[1].key, b"user:102");
        assert_eq!(records[1].value, b"Bob");

        assert_eq!(records[2].lsn, 3);
        assert_eq!(records[2].record_type, WalRecordType::Delete);
        assert_eq!(records[2].key, b"user:101");

        assert_eq!(records[3].lsn, 4);
        assert_eq!(records[3].record_type, WalRecordType::Checkpoint);
    }

    #[test]
    fn test_wal_segment_rotation_and_purge() {
        let dir = TempDir::new("shae_wal");
        let wal_dir = dir.path().join("wal_rot_test");

        let mut opts = WalOptions::default();
        opts.max_segment_size = 120; // Force rotation after ~2 records

        {
            let mut wal = Wal::open(&wal_dir, opts).unwrap();
            for i in 1..=10 {
                let key = format!("key_{}", i);
                let val = format!("value_data_{:08}", i);
                wal.append(WalRecordType::Put, key.as_bytes(), val.as_bytes()).unwrap();
            }
            assert!(wal.segments.len() > 1, "Segments should have rotated");

            // Purge before LSN 5
            let purged = wal.purge_before(5).unwrap();
            assert!(purged > 0, "Should have purged older segment files");

            wal.close().unwrap();
        }

        let (records, next_lsn) = Wal::recover(&wal_dir).unwrap();
        assert_eq!(next_lsn, 11);
        assert!(records.len() > 0);
    }

    #[test]
    fn test_wal_torn_write_recovery() {
        let dir = TempDir::new("shae_wal");
        let wal_dir = dir.path().join("wal_torn_test");

        let seg_path;
        {
            let mut wal = Wal::open(&wal_dir, WalOptions::default()).unwrap();
            wal.append(WalRecordType::Put, b"k1", b"v1").unwrap();
            wal.append(WalRecordType::Put, b"k2", b"v2").unwrap();
            seg_path = wal.segments[0].path.clone();
            wal.close().unwrap();
        }

        // Corrupt file by appending partial corrupted bytes
        {
            let mut file = OpenOptions::new().append(true).open(&seg_path).unwrap();
            file.write_all(b"SHAE_CORRUPT_BYTES_PARTIAL_CRASH").unwrap();
        }

        // Recover should cleanly succeed up to last intact record (2 records)
        let (records, next_lsn) = Wal::recover(&wal_dir).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].key, b"k1");
        assert_eq!(records[1].key, b"k2");
        assert_eq!(next_lsn, 3);
    }
}
