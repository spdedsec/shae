// Shae Storage Engine: High-Performance Log-Structured Merge-Tree (LSM-Tree)
// Part of Option 5: Embedded LSM-Tree & High-Speed Storage Engine

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

// ---------------------------------------------------------------------------
// Bloom Filter: Double-Hashing with Murmur3 + FNV-1a
// ---------------------------------------------------------------------------

fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn murmur3_hash(data: &[u8]) -> u64 {
    let mut h = 0x5bd1e995u64;
    for &b in data {
        h = h.wrapping_add(b as u64);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1e995);
        h ^= h >> 15;
    }
    h | 1 // ensure odd
}

#[derive(Debug, Clone)]
pub struct BloomFilter {
    pub bit_count: usize,
    pub num_hashes: usize,
    pub bits: Vec<u8>,
}

impl BloomFilter {
    pub fn new(expected_entries: usize, bits_per_key: usize) -> Self {
        let entries = if expected_entries == 0 { 16 } else { expected_entries };
        let bit_count = entries * bits_per_key;
        let byte_count = (bit_count + 7) / 8;
        let num_hashes = ((bits_per_key as f64) * 0.693).round() as usize;
        let num_hashes = num_hashes.max(1).min(16);

        BloomFilter {
            bit_count,
            num_hashes,
            bits: vec![0u8; byte_count],
        }
    }

    pub fn add(&mut self, key: &[u8]) {
        if self.bit_count == 0 {
            return;
        }
        let h1 = fnv1a_hash(key);
        let h2 = murmur3_hash(key);

        for i in 0..self.num_hashes {
            let combined = h1.wrapping_add((i as u64).wrapping_mul(h2));
            let bit_idx = (combined % (self.bit_count as u64)) as usize;
            let byte_idx = bit_idx / 8;
            let offset = bit_idx % 8;
            if byte_idx < self.bits.len() {
                self.bits[byte_idx] |= 1 << offset;
            }
        }
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        if self.bit_count == 0 || self.bits.is_empty() {
            return true;
        }
        let h1 = fnv1a_hash(key);
        let h2 = murmur3_hash(key);

        for i in 0..self.num_hashes {
            let combined = h1.wrapping_add((i as u64).wrapping_mul(h2));
            let bit_idx = (combined % (self.bit_count as u64)) as usize;
            let byte_idx = bit_idx / 8;
            let offset = bit_idx % 8;
            if byte_idx >= self.bits.len() || (self.bits[byte_idx] & (1 << offset)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.bits.len());
        out.extend_from_slice(&(self.bit_count as u32).to_le_bytes());
        out.extend_from_slice(&(self.num_hashes as u32).to_le_bytes());
        out.extend_from_slice(&(self.bits.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.bits);
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let bit_count = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        let num_hashes = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
        let byte_len = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
        if data.len() < 12 + byte_len {
            return None;
        }
        let bits = data[12..12 + byte_len].to_vec();
        Some(BloomFilter {
            bit_count,
            num_hashes,
            bits,
        })
    }
}

// ---------------------------------------------------------------------------
// SSTable (Sorted String Table) Format & Sparse Index
// ---------------------------------------------------------------------------

pub const SST_MAGIC: [u8; 4] = [0x53, 0x53, 0x54, 0x31]; // "SST1"
pub const SST_FOOTER_SIZE: u64 = 32;
pub const INDEX_INTERVAL: usize = 16;

#[derive(Debug, Clone)]
pub struct SparseIndexEntry {
    pub key: Vec<u8>,
    pub offset: u64,
}

pub struct SSTableReader {
    pub path: PathBuf,
    pub level: usize,
    pub id: u64,
    pub entry_count: usize,
    pub index_offset: u64,
    pub index: Vec<SparseIndexEntry>,
    pub filter: BloomFilter,
    pub min_key: Vec<u8>,
    pub max_key: Vec<u8>,
}

impl SSTableReader {
    pub fn open<P: AsRef<Path>>(path: P, level: usize, id: u64) -> Result<Self, io::Error> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let file_len = file.metadata()?.len();

        if file_len < SST_FOOTER_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SSTable too short"));
        }

        // Read 32-byte Footer at EOF
        file.seek(SeekFrom::Start(file_len - SST_FOOTER_SIZE))?;
        let mut footer = [0u8; 32];
        file.read_exact(&mut footer)?;

        if footer[0..4] != SST_MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid SSTable magic"));
        }

        let index_offset = u64::from_le_bytes(footer[4..12].try_into().unwrap());
        let index_len = u32::from_le_bytes(footer[12..16].try_into().unwrap()) as usize;
        let filter_offset = u64::from_le_bytes(footer[16..24].try_into().unwrap());
        let filter_len = u32::from_le_bytes(footer[24..28].try_into().unwrap()) as usize;
        let entry_count = u32::from_le_bytes(footer[28..32].try_into().unwrap()) as usize;

        // Read Index Block
        file.seek(SeekFrom::Start(index_offset))?;
        let mut index_bytes = vec![0u8; index_len];
        file.read_exact(&mut index_bytes)?;

        let mut pos = 0;
        let mut min_key = Vec::new();
        let mut max_key = Vec::new();

        // Read min_key
        if pos + 4 <= index_len {
            let min_len = u32::from_le_bytes(index_bytes[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + min_len <= index_len {
                min_key = index_bytes[pos..pos + min_len].to_vec();
                pos += min_len;
            }
        }

        // Read max_key
        if pos + 4 <= index_len {
            let max_len = u32::from_le_bytes(index_bytes[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + max_len <= index_len {
                max_key = index_bytes[pos..pos + max_len].to_vec();
                pos += max_len;
            }
        }

        // Read sparse index entries
        let mut index = Vec::new();
        while pos + 12 <= index_len {
            let klen = u32::from_le_bytes(index_bytes[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + klen + 8 > index_len {
                break;
            }
            let key = index_bytes[pos..pos + klen].to_vec();
            pos += klen;
            let offset = u64::from_le_bytes(index_bytes[pos..pos + 8].try_into().unwrap());
            pos += 8;

            index.push(SparseIndexEntry { key, offset });
        }

        // Read Bloom Filter Block
        file.seek(SeekFrom::Start(filter_offset))?;
        let mut filter_bytes = vec![0u8; filter_len];
        file.read_exact(&mut filter_bytes)?;

        let filter = BloomFilter::decode(&filter_bytes).unwrap_or_else(|| BloomFilter::new(16, 10));

        Ok(SSTableReader {
            path,
            level,
            id,
            entry_count,
            index_offset,
            index,
            filter,
            min_key,
            max_key,
        })
    }

    /// Point lookup: checks Bloom Filter first, key bounds, binary searches Sparse Index, then reads block.
    pub fn get(&self, key: &[u8]) -> Result<Option<Option<Vec<u8>>>, io::Error> {
        if !self.filter.contains(key) {
            return Ok(None);
        }

        if !self.min_key.is_empty() && key < self.min_key.as_slice() {
            return Ok(None);
        }
        if !self.max_key.is_empty() && key > self.max_key.as_slice() {
            return Ok(None);
        }

        let mut start_offset = 0u64;
        let mut end_offset = self.index_offset;

        if !self.index.is_empty() {
            let mut left = 0;
            let mut right = self.index.len();

            while left < right {
                let mid = left + (right - left) / 2;
                if self.index[mid].key.as_slice() <= key {
                    start_offset = self.index[mid].offset;
                    left = mid + 1;
                } else {
                    right = mid;
                }
            }

            if left < self.index.len() {
                end_offset = self.index[left].offset;
            } else {
                end_offset = self.index_offset;
            }
        }

        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(start_offset))?;

        let block_len = (end_offset - start_offset) as usize;
        let mut block = vec![0u8; block_len];
        file.read_exact(&mut block)?;

        let mut pos = 0;
        while pos + 8 <= block_len {
            let klen = u32::from_le_bytes(block[pos..pos + 4].try_into().unwrap()) as usize;
            let vlen = u32::from_le_bytes(block[pos + 4..pos + 8].try_into().unwrap());
            pos += 8;

            if pos + klen > block_len {
                break;
            }
            let entry_key = &block[pos..pos + klen];
            pos += klen;

            let is_tombstone = vlen == 0xFFFFFFFF;
            let val_bytes = if is_tombstone {
                None
            } else {
                let vl = vlen as usize;
                if pos + vl > block_len {
                    break;
                }
                let val = block[pos..pos + vl].to_vec();
                pos += vl;
                Some(val)
            };

            if entry_key == key {
                return Ok(Some(val_bytes));
            } else if entry_key > key {
                break;
            }
        }

        Ok(None)
    }

    /// Read all entries sequentially
    pub fn scan_all(&self) -> Result<Vec<(Vec<u8>, Option<Vec<u8>>)>, io::Error> {
        let mut entries = Vec::with_capacity(self.entry_count);
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(0))?;

        let data_len = self.index_offset as usize;
        let mut data = vec![0u8; data_len];
        file.read_exact(&mut data)?;

        let mut pos = 0;
        while pos + 8 <= data_len {
            let klen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            let vlen = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
            pos += 8;

            if pos + klen > data_len {
                break;
            }
            let key = data[pos..pos + klen].to_vec();
            pos += klen;

            let is_tombstone = vlen == 0xFFFFFFFF;
            let val = if is_tombstone {
                None
            } else {
                let vl = vlen as usize;
                if pos + vl > data_len {
                    break;
                }
                let v = data[pos..pos + vl].to_vec();
                pos += vl;
                Some(v)
            };

            entries.push((key, val));
        }

        Ok(entries)
    }
}

pub struct SSTableWriter;

impl SSTableWriter {
    pub fn write_table<P: AsRef<Path>, I>(
        path: P,
        entries: I,
        level: usize,
        id: u64,
    ) -> Result<SSTableReader, io::Error>
    where
        I: IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>,
    {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        let mut items: Vec<(Vec<u8>, Option<Vec<u8>>)> = entries.into_iter().collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));

        let mut bloom = BloomFilter::new(items.len(), 10);
        let mut index = Vec::new();
        let mut cur_offset = 0u64;

        let min_key = items.first().map(|p| p.0.clone()).unwrap_or_default();
        let max_key = items.last().map(|p| p.0.clone()).unwrap_or_default();

        for (i, (key, val)) in items.iter().enumerate() {
            bloom.add(key);

            if i % INDEX_INTERVAL == 0 {
                index.push(SparseIndexEntry {
                    key: key.clone(),
                    offset: cur_offset,
                });
            }

            let klen = key.len() as u32;
            let (vlen, vbytes) = match val {
                Some(v) => (v.len() as u32, v.as_slice()),
                None => (0xFFFFFFFFu32, &[][..]),
            };

            file.write_all(&klen.to_le_bytes())?;
            file.write_all(&vlen.to_le_bytes())?;
            file.write_all(key)?;
            file.write_all(vbytes)?;

            cur_offset += 8 + (key.len() as u64) + (vbytes.len() as u64);
        }

        let index_offset = cur_offset;

        // Write Index Block:
        // [min_key_len: u32][min_key][max_key_len: u32][max_key][entries...]
        let mut index_bytes = Vec::new();
        index_bytes.extend_from_slice(&(min_key.len() as u32).to_le_bytes());
        index_bytes.extend_from_slice(&min_key);
        index_bytes.extend_from_slice(&(max_key.len() as u32).to_le_bytes());
        index_bytes.extend_from_slice(&max_key);

        for entry in &index {
            let klen = entry.key.len() as u32;
            index_bytes.extend_from_slice(&klen.to_le_bytes());
            index_bytes.extend_from_slice(&entry.key);
            index_bytes.extend_from_slice(&entry.offset.to_le_bytes());
        }
        file.write_all(&index_bytes)?;
        let index_len = index_bytes.len() as u32;

        // Write Bloom Filter Block
        let filter_offset = index_offset + (index_len as u64);
        let filter_bytes = bloom.encode();
        file.write_all(&filter_bytes)?;
        let filter_len = filter_bytes.len() as u32;

        // Write Footer: Magic (4B) + IndexOffset (8B) + IndexLen (4B) + FilterOffset (8B) + FilterLen (4B) + Count (4B) = 32B
        let mut footer = Vec::with_capacity(32);
        footer.extend_from_slice(&SST_MAGIC);
        footer.extend_from_slice(&index_offset.to_le_bytes());
        footer.extend_from_slice(&index_len.to_le_bytes());
        footer.extend_from_slice(&filter_offset.to_le_bytes());
        footer.extend_from_slice(&filter_len.to_le_bytes());
        footer.extend_from_slice(&(items.len() as u32).to_le_bytes());
        file.write_all(&footer)?;

        file.sync_all()?;

        Ok(SSTableReader {
            path,
            level,
            id,
            entry_count: items.len(),
            index_offset,
            index,
            filter: bloom,
            min_key,
            max_key,
        })
    }
}

// ---------------------------------------------------------------------------
// LSM Tree Core Engine
// ---------------------------------------------------------------------------

pub const DEFAULT_MEMTABLE_LIMIT: usize = 256 * 1024; // 256 KB

#[derive(Debug, Clone)]
pub struct LsmOptions {
    pub memtable_limit: usize,
}

impl Default for LsmOptions {
    fn default() -> Self {
        LsmOptions {
            memtable_limit: DEFAULT_MEMTABLE_LIMIT,
        }
    }
}

pub struct LsmEngine {
    pub dir: PathBuf,
    pub options: LsmOptions,
    pub active_memtable: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    pub active_bytes: usize,
    pub immutable_memtables: Vec<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    pub sstables: Vec<Arc<SSTableReader>>,
    pub next_sst_id: u64,
}

impl LsmEngine {
    pub fn open<P: AsRef<Path>>(dir: P, options: LsmOptions) -> Result<Self, io::Error> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let mut sstables = Vec::new();
        let mut max_id = 0u64;

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("sst_") && name.ends_with(".db") {
                            let parts: Vec<&str> = name[4..name.len() - 3].split('_').collect();
                            if parts.len() == 2 {
                                let level = parts[0].parse::<usize>().unwrap_or(0);
                                let id = u64::from_str_radix(parts[1], 16).unwrap_or(0);
                                if id > max_id {
                                    max_id = id;
                                }
                                if let Ok(reader) = SSTableReader::open(&path, level, id) {
                                    sstables.push(Arc::new(reader));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort SSTables: Level 0 first (descending by ID, newest first), then higher levels
        sstables.sort_by(|a, b| {
            if a.level != b.level {
                a.level.cmp(&b.level)
            } else {
                b.id.cmp(&a.id)
            }
        });

        Ok(LsmEngine {
            dir,
            options,
            active_memtable: BTreeMap::new(),
            active_bytes: 0,
            immutable_memtables: Vec::new(),
            sstables,
            next_sst_id: max_id + 1,
        })
    }

    pub fn put(&mut self, key: &[u8], val: &[u8]) -> Result<(), io::Error> {
        let entry_size = key.len() + val.len() + 16;
        self.active_memtable.insert(key.to_vec(), Some(val.to_vec()));
        self.active_bytes += entry_size;

        if self.active_bytes >= self.options.memtable_limit {
            self.flush_memtable()?;
        }
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<(), io::Error> {
        let entry_size = key.len() + 16;
        self.active_memtable.insert(key.to_vec(), None);
        self.active_bytes += entry_size;

        if self.active_bytes >= self.options.memtable_limit {
            self.flush_memtable()?;
        }
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, io::Error> {
        // 1. Search Active MemTable
        if let Some(entry) = self.active_memtable.get(key) {
            return Ok(entry.clone());
        }

        // 2. Search Immutable MemTables (newest to oldest)
        for imm in self.immutable_memtables.iter().rev() {
            if let Some(entry) = imm.get(key) {
                return Ok(entry.clone());
            }
        }

        // 3. Search SSTables (in order)
        for sst in &self.sstables {
            if let Some(res) = sst.get(key)? {
                return Ok(res); // May be Some(val) or None (tombstone)
            }
        }

        Ok(None)
    }

    pub fn has(&self, key: &[u8]) -> Result<bool, io::Error> {
        Ok(self.get(key)?.is_some())
    }

    pub fn flush_memtable(&mut self) -> Result<bool, io::Error> {
        if self.active_memtable.is_empty() {
            return Ok(false);
        }

        let id = self.next_sst_id;
        self.next_sst_id += 1;
        let sst_path = self.dir.join(format!("sst_0_{:08x}.db", id));

        let entries = std::mem::take(&mut self.active_memtable);
        self.active_bytes = 0;

        let reader = SSTableWriter::write_table(&sst_path, entries, 0, id)?;
        self.sstables.insert(0, Arc::new(reader));

        Ok(true)
    }

    /// Scan range: yields non-deleted entries in sorted key order
    pub fn scan(
        &self,
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
        limit: Option<usize>,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, io::Error> {
        let mut merged: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();

        // Older SSTables first, then newer SSTables overwrite
        for sst in self.sstables.iter().rev() {
            for (k, v) in sst.scan_all()? {
                merged.insert(k, v);
            }
        }

        for imm in &self.immutable_memtables {
            for (k, v) in imm {
                merged.insert(k.clone(), v.clone());
            }
        }

        for (k, v) in &self.active_memtable {
            merged.insert(k.clone(), v.clone());
        }

        let max_results = limit.unwrap_or(usize::MAX);
        let mut results = Vec::new();

        for (k, v) in merged {
            if let Some(start) = start_key {
                if k.as_slice() < start {
                    continue;
                }
            }
            if let Some(end) = end_key {
                if k.as_slice() > end {
                    break;
                }
            }

            if let Some(val) = v {
                results.push((k, val));
                if results.len() >= max_results {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// Compaction: Merges all SSTables into a single consolidated SSTable,
    /// deduplicating versions and purging deleted keys.
    pub fn compact(&mut self) -> Result<usize, io::Error> {
        self.flush_memtable()?;

        if self.sstables.len() <= 1 {
            return Ok(0);
        }

        let count = self.sstables.len();
        let mut merged: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();

        // Reverse order so newer overwrites older
        for sst in self.sstables.iter().rev() {
            for (k, v) in sst.scan_all()? {
                merged.insert(k, v);
            }
        }

        // Purge tombstones during compaction
        let compacted_entries: Vec<(Vec<u8>, Option<Vec<u8>>)> = merged
            .into_iter()
            .filter(|(_, v)| v.is_some())
            .collect();

        let id = self.next_sst_id;
        self.next_sst_id += 1;
        let new_sst_path = self.dir.join(format!("sst_1_{:08x}.db", id));

        let new_reader = SSTableWriter::write_table(&new_sst_path, compacted_entries, 1, id)?;

        // Remove old SSTables
        for sst in &self.sstables {
            let _ = fs::remove_file(&sst.path);
        }

        self.sstables = vec![Arc::new(new_reader)];
        Ok(count)
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let mem_keys = self.active_memtable.len();
        let sst_count = self.sstables.len();
        let sst_entries: usize = self.sstables.iter().map(|s| s.entry_count).sum();
        (mem_keys, sst_count, sst_entries)
    }
}

// ---------------------------------------------------------------------------
// Global Registry for Shae Scripts
// ---------------------------------------------------------------------------

static NEXT_LSM_ID: AtomicUsize = AtomicUsize::new(1);
static LSM_INSTANCES: OnceLock<RwLock<HashMap<usize, Arc<Mutex<LsmEngine>>>>> = OnceLock::new();

fn get_lsm_registry() -> &'static RwLock<HashMap<usize, Arc<Mutex<LsmEngine>>>> {
    LSM_INSTANCES.get_or_init(|| RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Standard Library Module (`std:db:lsm` or `std:db`)
// ---------------------------------------------------------------------------

pub fn build_lsm_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("open", lsm_open),
        ("put", lsm_put),
        ("get", lsm_get),
        ("delete", lsm_delete),
        ("has", lsm_has),
        ("scan", lsm_scan),
        ("flush", lsm_flush),
        ("compact", lsm_compact),
        ("stats", lsm_stats),
        ("close", lsm_close),
        // Aliases
        ("lsm_open", lsm_open),
        ("lsm_put", lsm_put),
        ("lsm_get", lsm_get),
        ("lsm_delete", lsm_delete),
        ("lsm_has", lsm_has),
        ("lsm_scan", lsm_scan),
        ("lsm_flush", lsm_flush),
        ("lsm_compact", lsm_compact),
        ("lsm_stats", lsm_stats),
        ("lsm_close", lsm_close),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("db.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn lsm_open(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("db.open requires directory path".to_string()).at(span));
    }

    let dir_str = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("db.open: expected directory string".to_string()).at(span)),
    };

    let mut options = LsmOptions::default();
    if args.len() > 1 {
        if let Value::Map(m) = &args[1] {
            let map = m.read().unwrap();
            if let Some(Value::Int(n)) = map.get("memtable_limit") {
                if *n > 0 {
                    options.memtable_limit = *n as usize;
                }
            }
        }
    }

    let engine = LsmEngine::open(&dir_str, options).map_err(|e| {
        RuntimeError::new(format!("db.open failed: {}", e)).at(span)
    })?;

    let id = NEXT_LSM_ID.fetch_add(1, Ordering::SeqCst);
    get_lsm_registry()
        .write()
        .unwrap()
        .insert(id, Arc::new(Mutex::new(engine)));

    Ok(Value::Int(id as i64))
}

fn lsm_put(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.put", 3, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.put: expected db id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("db.put: expected string key".to_string()).at(span)),
    };

    let val_bytes = match &args[2] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("db.put: expected string value".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.put: invalid db id {}", id)).at(span)
    })?;

    let mut engine = lsm_arc.lock().unwrap();
    engine.put(&key_bytes, &val_bytes).map_err(|e| {
        RuntimeError::new(format!("db.put failed: {}", e)).at(span)
    })?;

    Ok(Value::Bool(true))
}

fn lsm_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.get", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.get: expected db id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("db.get: expected string key".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.get: invalid db id {}", id)).at(span)
    })?;

    let engine = lsm_arc.lock().unwrap();
    match engine.get(&key_bytes) {
        Ok(Some(bytes)) => Ok(Value::String(String::from_utf8_lossy(&bytes).to_string())),
        Ok(None) => Ok(Value::Null),
        Err(e) => Err(RuntimeError::new(format!("db.get failed: {}", e)).at(span)),
    }
}

fn lsm_delete(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.delete", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.delete: expected db id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("db.delete: expected string key".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.delete: invalid db id {}", id)).at(span)
    })?;

    let mut engine = lsm_arc.lock().unwrap();
    engine.delete(&key_bytes).map_err(|e| {
        RuntimeError::new(format!("db.delete failed: {}", e)).at(span)
    })?;

    Ok(Value::Bool(true))
}

fn lsm_has(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.has", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.has: expected db id (int)".to_string()).at(span)),
    };

    let key_bytes = match &args[1] {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(RuntimeError::new("db.has: expected string key".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.has: invalid db id {}", id)).at(span)
    })?;

    let engine = lsm_arc.lock().unwrap();
    let exists = engine.has(&key_bytes).map_err(|e| {
        RuntimeError::new(format!("db.has failed: {}", e)).at(span)
    })?;

    Ok(Value::Bool(exists))
}

fn lsm_scan(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::new("db.scan requires db_id, start_key, and end_key".to_string()).at(span));
    }

    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.scan: expected db id (int)".to_string()).at(span)),
    };

    let start_key = match &args[1] {
        Value::String(s) => Some(s.as_bytes()),
        Value::Null => None,
        _ => return Err(RuntimeError::new("db.scan: expected string or null start_key".to_string()).at(span)),
    };

    let end_key = match &args[2] {
        Value::String(s) => Some(s.as_bytes()),
        Value::Null => None,
        _ => return Err(RuntimeError::new("db.scan: expected string or null end_key".to_string()).at(span)),
    };

    let limit = if args.len() > 3 {
        match &args[3] {
            Value::Int(i) if *i > 0 => Some(*i as usize),
            _ => None,
        }
    } else {
        None
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.scan: invalid db id {}", id)).at(span)
    })?;

    let engine = lsm_arc.lock().unwrap();
    let items = engine.scan(start_key, end_key, limit).map_err(|e| {
        RuntimeError::new(format!("db.scan failed: {}", e)).at(span)
    })?;

    let mut out_arr = Vec::with_capacity(items.len());
    for (k, v) in items {
        let pair = vec![
            Value::String(String::from_utf8_lossy(&k).to_string()),
            Value::String(String::from_utf8_lossy(&v).to_string()),
        ];
        out_arr.push(Value::Array(Arc::new(RwLock::new(pair))));
    }

    Ok(Value::Array(Arc::new(RwLock::new(out_arr))))
}

fn lsm_flush(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.flush", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.flush: expected db id (int)".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.flush: invalid db id {}", id)).at(span)
    })?;

    let mut engine = lsm_arc.lock().unwrap();
    let flushed = engine.flush_memtable().map_err(|e| {
        RuntimeError::new(format!("db.flush failed: {}", e)).at(span)
    })?;

    Ok(Value::Bool(flushed))
}

fn lsm_compact(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.compact", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.compact: expected db id (int)".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.compact: invalid db id {}", id)).at(span)
    })?;

    let mut engine = lsm_arc.lock().unwrap();
    let compacted_count = engine.compact().map_err(|e| {
        RuntimeError::new(format!("db.compact failed: {}", e)).at(span)
    })?;

    Ok(Value::Int(compacted_count as i64))
}

fn lsm_stats(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.stats", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.stats: expected db id (int)".to_string()).at(span)),
    };

    let registry = get_lsm_registry().read().unwrap();
    let lsm_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("db.stats: invalid db id {}", id)).at(span)
    })?;

    let engine = lsm_arc.lock().unwrap();
    let (mem_keys, sst_count, sst_entries) = engine.stats();

    let mut map = IndexMap::new();
    map.insert("mem_keys".to_string(), Value::Int(mem_keys as i64));
    map.insert("sst_count".to_string(), Value::Int(sst_count as i64));
    map.insert("sst_entries".to_string(), Value::Int(sst_entries as i64));

    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn lsm_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("db.close", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("db.close: expected db id (int)".to_string()).at(span)),
    };

    let mut registry = get_lsm_registry().write().unwrap();
    if let Some(lsm_arc) = registry.remove(&id) {
        let mut engine = lsm_arc.lock().unwrap();
        let _ = engine.flush_memtable();
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_bloom_filter_accuracy() {
        let mut bloom = BloomFilter::new(100, 10);
        bloom.add(b"key:alpha");
        bloom.add(b"key:beta");
        bloom.add(b"key:gamma");

        assert!(bloom.contains(b"key:alpha"));
        assert!(bloom.contains(b"key:beta"));
        assert!(bloom.contains(b"key:gamma"));
        assert!(!bloom.contains(b"key:delta"));

        let enc = bloom.encode();
        let dec = BloomFilter::decode(&enc).expect("decode succeeds");
        assert!(dec.contains(b"key:alpha"));
        assert!(!dec.contains(b"key:delta"));
    }

    #[test]
    fn test_lsm_memtable_flush_and_sstable_recovery() {
        let dir = TempDir::new("shae_lsm_test");
        let mut opts = LsmOptions::default();
        opts.memtable_limit = 200; // Force flushes

        {
            let mut lsm = LsmEngine::open(dir.path(), opts.clone()).unwrap();
            for i in 1..=20 {
                let k = format!("item:{:04}", i);
                let v = format!("value:{:06}", i * 10);
                lsm.put(k.as_bytes(), v.as_bytes()).unwrap();
            }

            // Flush remaining memtable
            lsm.flush_memtable().unwrap();
            assert!(lsm.sstables.len() >= 1, "Must have created SSTables");

            // Verify get on SSTables
            let v5 = lsm.get(b"item:0005").unwrap().expect("item 5 must exist");
            assert_eq!(v5, b"value:000050");

            // Delete item 5
            lsm.delete(b"item:0005").unwrap();
            assert_eq!(lsm.get(b"item:0005").unwrap(), None);

            // Flush before dropping so delete is written
            lsm.flush_memtable().unwrap();
        }

        // Reopen from disk
        {
            let lsm = LsmEngine::open(dir.path(), opts).unwrap();
            assert!(lsm.sstables.len() >= 1);
            let v10 = lsm.get(b"item:0010").unwrap().expect("item 10 must exist");
            assert_eq!(v10, b"value:000100");

            // Verify deleted item 5 remains absent
            assert_eq!(lsm.get(b"item:0005").unwrap(), None);
        }
    }

    #[test]
    fn test_lsm_range_scan_and_compaction() {
        let dir = TempDir::new("shae_lsm_compact_test");
        let mut opts = LsmOptions::default();
        opts.memtable_limit = 100;

        let mut lsm = LsmEngine::open(dir.path(), opts).unwrap();
        lsm.put(b"a", b"1").unwrap();
        lsm.put(b"b", b"2").unwrap();
        lsm.flush_memtable().unwrap();

        lsm.put(b"c", b"3").unwrap();
        lsm.put(b"b", b"200").unwrap(); // overwrite b
        lsm.flush_memtable().unwrap();

        lsm.put(b"d", b"4").unwrap();
        lsm.delete(b"a").unwrap(); // delete a
        lsm.flush_memtable().unwrap();

        assert!(lsm.sstables.len() >= 3);

        // Range scan before compaction
        let items = lsm.scan(None, None, None).unwrap();
        assert_eq!(items.len(), 3); // b, c, d (a was deleted)
        assert_eq!(items[0], (b"b".to_vec(), b"200".to_vec()));
        assert_eq!(items[1], (b"c".to_vec(), b"3".to_vec()));
        assert_eq!(items[2], (b"d".to_vec(), b"4".to_vec()));

        // Run Compaction
        let compacted = lsm.compact().unwrap();
        assert!(compacted >= 3);
        assert_eq!(lsm.sstables.len(), 1);

        // Scan after compaction
        let post_items = lsm.scan(None, None, None).unwrap();
        assert_eq!(post_items.len(), 3);
        assert_eq!(post_items[0], (b"b".to_vec(), b"200".to_vec()));
    }
}
