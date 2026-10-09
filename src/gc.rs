use crate::value::{Closure, Upvalue, UpvalueLocation, Value};
use indexmap::IndexMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GcRef(pub usize);

impl fmt::Display for GcRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<gc:0x{:x}>", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GcData {
    String(String),
    Array(Vec<Value>),
    Map(IndexMap<String, Value>),
    Closure(Closure),
    Upvalue(Upvalue),
    Instance {
        name: String,
        fields: IndexMap<String, Value>,
    },
}

#[derive(Debug, Clone)]
pub struct GcObject {
    pub is_marked: bool,
    pub data: GcData,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcStats {
    pub objects_before: usize,
    pub objects_after: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
    pub freed_objects: usize,
    pub freed_bytes: usize,
}

pub struct GcHeap {
    pub objects: Vec<Option<GcObject>>,
    pub free_list: Vec<usize>,
    pub bytes_allocated: usize,
    pub next_gc_threshold: usize,
    pub total_allocations: usize,
    pub total_collections: usize,
    pub stress_gc: bool,
}

pub const DEFAULT_INITIAL_THRESHOLD: usize = 1024 * 1024; // 1 MB
pub const MIN_HEAP_THRESHOLD: usize = 64 * 1024; // 64 KB

impl GcHeap {
    pub fn new() -> Self {
        Self::with_threshold(DEFAULT_INITIAL_THRESHOLD)
    }

    pub fn with_threshold(initial_bytes: usize) -> Self {
        GcHeap {
            objects: Vec::with_capacity(256),
            free_list: Vec::new(),
            bytes_allocated: 0,
            next_gc_threshold: initial_bytes.max(MIN_HEAP_THRESHOLD),
            total_allocations: 0,
            total_collections: 0,
            stress_gc: false,
        }
    }

    pub fn estimate_size(data: &GcData) -> usize {
        let header = std::mem::size_of::<GcObject>();
        let payload = match data {
            GcData::String(s) => std::mem::size_of::<String>() + s.capacity(),
            GcData::Array(arr) => {
                std::mem::size_of::<Vec<Value>>() + arr.capacity() * std::mem::size_of::<Value>()
            }
            GcData::Map(map) => {
                std::mem::size_of::<IndexMap<String, Value>>()
                    + map.capacity()
                        * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
            }
            GcData::Closure(_) => 128,
            GcData::Upvalue(_) => 48,
            GcData::Instance { fields, .. } => {
                std::mem::size_of::<IndexMap<String, Value>>()
                    + fields.capacity()
                        * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
            }
        };
        header + payload
    }

    pub fn alloc(&mut self, data: GcData) -> GcRef {
        let size = Self::estimate_size(&data);
        self.bytes_allocated += size;
        self.total_allocations += 1;

        let obj = GcObject {
            is_marked: false,
            data,
        };

        if let Some(reused_idx) = self.free_list.pop() {
            self.objects[reused_idx] = Some(obj);
            GcRef(reused_idx)
        } else {
            let idx = self.objects.len();
            self.objects.push(Some(obj));
            GcRef(idx)
        }
    }

    pub fn alloc_array(&mut self, elements: Vec<Value>) -> GcRef {
        self.alloc(GcData::Array(elements))
    }

    pub fn alloc_map(&mut self, map: IndexMap<String, Value>) -> GcRef {
        self.alloc(GcData::Map(map))
    }

    pub fn alloc_string(&mut self, s: String) -> GcRef {
        self.alloc(GcData::String(s))
    }

    pub fn alloc_closure(&mut self, closure: Closure) -> GcRef {
        self.alloc(GcData::Closure(closure))
    }

    pub fn alloc_instance(&mut self, name: String, fields: IndexMap<String, Value>) -> GcRef {
        self.alloc(GcData::Instance { name, fields })
    }

    pub fn get(&self, r: GcRef) -> Option<&GcData> {
        self.objects
            .get(r.0)
            .and_then(|opt| opt.as_ref())
            .map(|o| &o.data)
    }

    pub fn get_mut(&mut self, r: GcRef) -> Option<&mut GcData> {
        self.objects
            .get_mut(r.0)
            .and_then(|opt| opt.as_mut())
            .map(|o| &mut o.data)
    }

    pub fn as_array(&self, r: GcRef) -> Option<&Vec<Value>> {
        match self.get(r) {
            Some(GcData::Array(arr)) => Some(arr),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self, r: GcRef) -> Option<&mut Vec<Value>> {
        match self.get_mut(r) {
            Some(GcData::Array(arr)) => Some(arr),
            _ => None,
        }
    }

    pub fn as_map(&self, r: GcRef) -> Option<&IndexMap<String, Value>> {
        match self.get(r) {
            Some(GcData::Map(map)) => Some(map),
            _ => None,
        }
    }

    pub fn as_map_mut(&mut self, r: GcRef) -> Option<&mut IndexMap<String, Value>> {
        match self.get_mut(r) {
            Some(GcData::Map(map)) => Some(map),
            _ => None,
        }
    }

    pub fn as_string(&self, r: GcRef) -> Option<&str> {
        match self.get(r) {
            Some(GcData::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn live_objects_count(&self) -> usize {
        self.objects.iter().filter(|o| o.is_some()).count()
    }

    pub fn should_collect(&self) -> bool {
        self.stress_gc || self.bytes_allocated >= self.next_gc_threshold
    }

    pub fn mark_ref(&mut self, r: GcRef, gray_stack: &mut Vec<GcRef>) {
        if r.0 < self.objects.len() {
            if let Some(ref mut obj) = self.objects[r.0] {
                if !obj.is_marked {
                    obj.is_marked = true;
                    gray_stack.push(r);
                }
            }
        }
    }

    pub fn mark_value(&mut self, val: &Value, gray_stack: &mut Vec<GcRef>) {
        match val {
            Value::GcArray(r)
            | Value::GcMap(r)
            | Value::GcClosure(r)
            | Value::GcString(r)
            | Value::GcInstance(r) => {
                self.mark_ref(*r, gray_stack);
            }
            Value::Array(arr) => {
                // For bridging with legacy Arc arrays during transition
                if let Ok(guard) = arr.read() {
                    for v in guard.iter() {
                        self.mark_value(v, gray_stack);
                    }
                }
            }
            Value::Map(m) => {
                if let Ok(guard) = m.read() {
                    for (_k, v) in guard.iter() {
                        self.mark_value(v, gray_stack);
                    }
                }
            }
            _ => {}
        }
    }

    fn trace_references(&mut self, gray_stack: &mut Vec<GcRef>) {
        while let Some(r) = gray_stack.pop() {
            let mut child_values: Vec<Value> = Vec::new();

            if let Some(Some(obj)) = self.objects.get(r.0) {
                match &obj.data {
                    GcData::Array(arr) => {
                        child_values.extend(arr.clone());
                    }
                    GcData::Map(map) => {
                        child_values.extend(map.values().cloned());
                    }
                    GcData::Instance { fields, .. } => {
                        child_values.extend(fields.values().cloned());
                    }
                    GcData::Upvalue(uv) => {
                        if let UpvalueLocation::Closed(ref val) = uv.location {
                            child_values.push(val.clone());
                        }
                    }
                    GcData::Closure(closure) => {
                        for uv in &closure.upvalues {
                            let uv_guard = uv.read().unwrap();
                            if let UpvalueLocation::Closed(ref val) = uv_guard.location {
                                child_values.push(val.clone());
                            }
                        }
                    }
                    GcData::String(_) => {}
                }
            }

            for val in child_values {
                self.mark_value(&val, gray_stack);
            }
        }
    }

    pub fn sweep(&mut self) -> (usize, usize) {
        let mut freed_count = 0;
        let mut freed_bytes = 0;

        for i in 0..self.objects.len() {
            if let Some(ref mut obj) = self.objects[i] {
                if obj.is_marked {
                    obj.is_marked = false;
                } else {
                    let bytes = Self::estimate_size(&obj.data);
                    freed_bytes += bytes;
                    freed_count += 1;
                    self.objects[i] = None;
                    self.free_list.push(i);
                }
            }
        }

        self.bytes_allocated = self.bytes_allocated.saturating_sub(freed_bytes);
        (freed_count, freed_bytes)
    }

    pub fn collect_garbage<F>(&mut self, mark_roots: F) -> GcStats
    where
        F: FnOnce(&mut Self, &mut Vec<GcRef>),
    {
        let objects_before = self.live_objects_count();
        let bytes_before = self.bytes_allocated;

        let mut gray_stack = Vec::new();
        mark_roots(self, &mut gray_stack);

        self.trace_references(&mut gray_stack);

        let (freed_objects, freed_bytes) = self.sweep();

        self.next_gc_threshold = (self.bytes_allocated * 2).max(MIN_HEAP_THRESHOLD);
        self.total_collections += 1;

        let objects_after = self.live_objects_count();
        let bytes_after = self.bytes_allocated;

        GcStats {
            objects_before,
            objects_after,
            bytes_before,
            bytes_after,
            freed_objects,
            freed_bytes,
        }
    }
}
