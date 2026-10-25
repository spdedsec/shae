use crate::ast::Stmt;
use crate::chunk::Chunk;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::gc::GcRef;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

pub type BuiltinFn =
    fn(&mut Evaluator, Vec<Value>, crate::ast::Span) -> Result<Value, RuntimeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpvalueDesc {
    pub index: u8,
    pub is_local: bool,
}

#[derive(Debug, Clone)]
pub struct JitMetadata {
    pub invocation_count: Arc<std::sync::atomic::AtomicUsize>,
    pub jit_fn: Arc<std::sync::RwLock<Option<crate::jit::JitFnPtr>>>,
}

impl PartialEq for JitMetadata {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Default for JitMetadata {
    fn default() -> Self {
        Self {
            invocation_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            jit_fn: Arc::new(std::sync::RwLock::new(None)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFunction {
    pub arity: usize,
    pub chunk: Chunk,
    pub name: Option<String>,
    pub upvalues: Vec<UpvalueDesc>,
    pub is_async: bool,
    pub jit: JitMetadata,
}

impl CompiledFunction {
    pub fn new(
        arity: usize,
        chunk: Chunk,
        name: Option<String>,
        upvalues: Vec<UpvalueDesc>,
        is_async: bool,
    ) -> Self {
        Self {
            arity,
            chunk,
            name,
            upvalues,
            is_async,
            jit: JitMetadata::default(),
        }
    }

    pub fn record_invocation(&self) -> usize {
        self.jit
            .invocation_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    pub fn get_jit_fn(&self) -> Option<crate::jit::JitFnPtr> {
        *self.jit.jit_fn.read().unwrap()
    }

    pub fn set_jit_fn(&self, f: crate::jit::JitFnPtr) {
        *self.jit.jit_fn.write().unwrap() = Some(f);
    }

    pub fn is_float_candidate(&self) -> bool {
        self.chunk
            .constants
            .iter()
            .any(|c| matches!(c, Value::Float(_) | Value::Number(_)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpvalueLocation {
    Open(usize),
    Closed(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Upvalue {
    pub location: UpvalueLocation,
}

impl Upvalue {
    pub fn new(slot: usize) -> Self {
        Upvalue {
            location: UpvalueLocation::Open(slot),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub function: Arc<CompiledFunction>,
    pub upvalues: Vec<Arc<RwLock<Upvalue>>>,
    pub module_globals: Option<Arc<RwLock<HashMap<String, Value>>>>,
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.function, &other.function)
    }
}

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Number(f64),
    String(String),
    Array(Arc<RwLock<Vec<Value>>>),
    Map(Arc<RwLock<IndexMap<String, Value>>>),
    CompiledFunction(Arc<CompiledFunction>),
    Closure(Arc<Closure>),
    GcArray(GcRef),
    GcMap(GcRef),
    GcClosure(GcRef),
    GcString(GcRef),
    GcInstance(GcRef),
    Function {
        name: Option<String>,
        params: Arc<Vec<String>>,
        body: Arc<Vec<Stmt>>,
        closure: Arc<RwLock<Environment>>,
        is_async: bool,
    },
    Builtin {
        name: String,
        func: BuiltinFn,
    },
    BoundMethod {
        object: Box<Value>,
        method: String,
    },
    StructDef {
        name: String,
        fields: Vec<String>,
    },
    StructInstance {
        name: String,
        fields: Arc<RwLock<IndexMap<String, Value>>>,
    },
    EnumDef {
        name: String,
        variants: Arc<IndexMap<String, Vec<String>>>,
    },
    EnumConstructor {
        enum_name: String,
        variant_name: String,
        params: Vec<String>,
    },
    EnumInstance {
        enum_name: String,
        variant_name: String,
        values: Vec<Value>,
    },
    Task(Arc<Mutex<Option<std::thread::JoinHandle<Result<Value, RuntimeError>>>>>),
    Channel(Arc<crate::channel::Channel>),
    FfiLibrary(Arc<crate::ffi::DynamicLibrary>),
    FfiFunction(Arc<crate::ffi::DynamicFunction>),
    Actor(Arc<crate::actor::ActorRef>),
    Supervisor(Arc<crate::actor::SupervisorRef>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) | Value::GcArray(_) => "array",
            Value::Map(_) | Value::GcMap(_) => "map",
            Value::CompiledFunction(_) | Value::Closure(_) | Value::GcClosure(_) => "function",
            Value::GcString(_) => "string",
            Value::GcInstance(_) => "instance",
            Value::Function { .. } => "function",
            Value::Builtin { .. } => "builtin_function",
            Value::BoundMethod { .. } => "bound_method",
            Value::StructDef { .. } => "struct_def",
            Value::StructInstance { .. } => "struct_instance",
            Value::EnumDef { .. } => "enum_def",
            Value::EnumConstructor { .. } => "enum_constructor",
            Value::EnumInstance { .. } => "enum_instance",
            Value::Task(_) => "task",
            Value::Channel(_) => "channel",
            Value::FfiLibrary(_) => "ffi_library",
            Value::FfiFunction(_) => "ffi_function",
            Value::Actor(_) => "actor",
            Value::Supervisor(_) => "supervisor",
        }
    }

    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            Value::Function { .. }
                | Value::Builtin { .. }
                | Value::BoundMethod { .. }
                | Value::CompiledFunction(_)
                | Value::Closure(_)
                | Value::GcClosure(_)
        )
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(n) | Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.read().unwrap().is_empty(),
            Value::Map(m) => !m.read().unwrap().is_empty(),
            Value::CompiledFunction(_) => true,
            Value::Closure(_) => true,
            Value::GcArray(_)
            | Value::GcMap(_)
            | Value::GcClosure(_)
            | Value::GcString(_)
            | Value::GcInstance(_) => true,
            Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. } => true,
            Value::StructDef { .. } | Value::StructInstance { .. } => true,
            Value::EnumDef { .. }
            | Value::EnumConstructor { .. }
            | Value::EnumInstance { .. }
            | Value::Task(_)
            | Value::Channel(_)
            | Value::FfiLibrary(_)
            | Value::FfiFunction(_)
            | Value::Actor(_)
            | Value::Supervisor(_) => true,
        }
    }

    pub fn to_display(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(n) | Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => s.clone(),
            Value::Array(a) => {
                let items: Vec<String> = a.read().unwrap().iter().map(|v| v.to_repr()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let entries: Vec<String> = m
                    .read()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_repr()))
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Value::CompiledFunction(f) => {
                if let Some(n) = &f.name {
                    format!("<fn {}>", n)
                } else {
                    "<fn anonymous>".to_string()
                }
            }
            Value::Closure(c) => {
                if let Some(n) = &c.function.name {
                    format!("<fn {}>", n)
                } else {
                    "<fn anonymous>".to_string()
                }
            }
            Value::Function { name, .. } => {
                if let Some(n) = name {
                    format!("<fn {}>", n)
                } else {
                    "<fn anonymous>".to_string()
                }
            }
            Value::Builtin { name, .. } => format!("<builtin {}>", name),
            Value::BoundMethod { method, .. } => format!("<bound method {}>", method),
            Value::StructDef { name, .. } => format!("<struct {}>", name),
            Value::StructInstance { name, fields } => {
                let map = fields.read().unwrap();
                let mut out = String::new();
                out.push_str(name);
                out.push_str(" { ");
                let mut first = true;
                for (k, v) in map.iter() {
                    if !first {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    out.push_str(&v.to_repr());
                    first = false;
                }
                out.push_str(" }");
                out
            }
            Value::EnumDef { name, .. } => format!("<enum {}>", name),
            Value::EnumConstructor {
                enum_name,
                variant_name,
                ..
            } => format!("<constructor {}.{}>", enum_name, variant_name),
            Value::EnumInstance {
                enum_name,
                variant_name,
                values,
            } => {
                if values.is_empty() {
                    format!("{}.{}", enum_name, variant_name)
                } else {
                    let vals: Vec<String> = values.iter().map(|v| v.to_repr()).collect();
                    format!("{}.{}({})", enum_name, variant_name, vals.join(", "))
                }
            }
            Value::GcArray(r) => format!("<gc_array:{}>", r),
            Value::GcMap(r) => format!("<gc_map:{}>", r),
            Value::GcClosure(r) => format!("<gc_closure:{}>", r),
            Value::GcString(r) => format!("<gc_string:{}>", r),
            Value::GcInstance(r) => format!("<gc_instance:{}>", r),
            Value::Task(_) => "<task>".to_string(),
            Value::Channel(ch) => {
                if let Some(c) = ch.capacity() {
                    format!("<channel:{}>", c)
                } else {
                    "<channel>".to_string()
                }
            }
            Value::FfiLibrary(lib) => format!("<ffi_library \"{}\">", lib.path),
            Value::FfiFunction(func) => format!("<ffi_function {}()>", func.name),
            Value::Actor(act) => act.to_string(),
            Value::Supervisor(sup) => sup.to_string(),
        }
    }

    pub fn to_repr(&self) -> String {
        match self {
            Value::String(s) => format!("\"{}\"", s),
            _ => self.to_display(),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        match self {
            Value::Null => JsonValue::Null,
            Value::Bool(b) => JsonValue::Bool(*b),
            Value::Int(n) => JsonValue::Number((*n).into()),
            Value::Float(n) | Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() <= 9007199254740991.0 {
                    JsonValue::Number((*n as i64).into())
                } else {
                    serde_json::Number::from_f64(*n)
                        .map(JsonValue::Number)
                        .unwrap_or(JsonValue::Null)
                }
            }
            Value::String(s) => JsonValue::String(s.clone()),
            Value::Array(a) => {
                let list: Vec<JsonValue> = a.read().unwrap().iter().map(|v| v.to_json()).collect();
                JsonValue::Array(list)
            }
            Value::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m.read().unwrap().iter() {
                    map.insert(k.clone(), v.to_json());
                }
                JsonValue::Object(map)
            }
            Value::CompiledFunction(f) => {
                JsonValue::String(format!("<fn {}>", f.name.as_deref().unwrap_or("anon")))
            }
            Value::Closure(c) => JsonValue::String(format!(
                "<fn {}>",
                c.function.name.as_deref().unwrap_or("anon")
            )),
            Value::Function { name, .. } => {
                JsonValue::String(format!("<fn {}>", name.as_deref().unwrap_or("anon")))
            }
            Value::Builtin { name, .. } => JsonValue::String(format!("<builtin {}>", name)),
            Value::BoundMethod { method, .. } => {
                JsonValue::String(format!("<bound method {}>", method))
            }
            Value::StructDef { name, .. } => JsonValue::String(format!("<struct {}>", name)),
            Value::StructInstance { name, fields } => {
                let mut map = serde_json::Map::new();
                map.insert("__type__".to_string(), JsonValue::String(name.clone()));
                for (k, v) in fields.read().unwrap().iter() {
                    map.insert(k.clone(), v.to_json());
                }
                JsonValue::Object(map)
            }
            Value::EnumDef { name, .. } => JsonValue::String(format!("<enum {}>", name)),
            Value::EnumConstructor {
                enum_name,
                variant_name,
                ..
            } => JsonValue::String(format!("<constructor {}.{}>", enum_name, variant_name)),
            Value::EnumInstance {
                enum_name,
                variant_name,
                values,
            } => {
                let mut map = serde_json::Map::new();
                map.insert("__enum__".to_string(), JsonValue::String(enum_name.clone()));
                map.insert(
                    "__variant__".to_string(),
                    JsonValue::String(variant_name.clone()),
                );
                let list: Vec<JsonValue> = values.iter().map(|v| v.to_json()).collect();
                map.insert("values".to_string(), JsonValue::Array(list));
                JsonValue::Object(map)
            }
            Value::GcArray(r) => JsonValue::String(format!("<gc_array:{}>", r)),
            Value::GcMap(r) => JsonValue::String(format!("<gc_map:{}>", r)),
            Value::GcClosure(r) => JsonValue::String(format!("<gc_closure:{}>", r)),
            Value::GcString(r) => JsonValue::String(format!("<gc_string:{}>", r)),
            Value::GcInstance(r) => JsonValue::String(format!("<gc_instance:{}>", r)),
            Value::Task(_) => JsonValue::String("<task>".to_string()),
            Value::Channel(_) => JsonValue::String("<channel>".to_string()),
            Value::FfiLibrary(lib) => JsonValue::String(format!("<ffi_library \"{}\">", lib.path)),
            Value::FfiFunction(func) => {
                JsonValue::String(format!("<ffi_function {}()>", func.name))
            }
            Value::Actor(act) => JsonValue::String(act.to_string()),
            Value::Supervisor(sup) => JsonValue::String(sup.to_string()),
        }
    }

    pub fn from_json(json: &JsonValue) -> Value {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Bool(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else {
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            JsonValue::String(s) => Value::String(s.clone()),
            JsonValue::Array(arr) => {
                let list: Vec<Value> = arr.iter().map(Value::from_json).collect();
                Value::Array(Arc::new(RwLock::new(list)))
            }
            JsonValue::Object(obj) => {
                let mut map = IndexMap::new();
                for (k, v) in obj {
                    map.insert(k.clone(), Value::from_json(v));
                }
                Value::Map(Arc::new(RwLock::new(map)))
            }
        }
    }

    /// Recursively deep-clones a `Value` to guarantee 100% memory isolation
    /// across actor boundaries. Compound collections (`Array`, `Map`, `StructInstance`)
    /// are allocated into fresh `Arc<RwLock<...>>` containers. Cyclic structures
    /// are detected via pointer memoization to prevent infinite loops.
    pub fn deep_clone(&self) -> Value {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(*b),
            Value::Int(i) => Value::Int(*i),
            Value::Float(f) => Value::Float(*f),
            Value::Number(n) => Value::Number(*n),
            Value::String(s) => Value::String(s.clone()),
            Value::Channel(c) => Value::Channel(Arc::clone(c)),
            Value::Task(t) => Value::Task(Arc::clone(t)),
            Value::Actor(a) => Value::Actor(Arc::clone(a)),
            Value::Supervisor(s) => Value::Supervisor(Arc::clone(s)),
            Value::FfiLibrary(l) => Value::FfiLibrary(Arc::clone(l)),
            Value::FfiFunction(f) => Value::FfiFunction(Arc::clone(f)),
            Value::CompiledFunction(f) => Value::CompiledFunction(Arc::clone(f)),
            Value::Builtin { name, func } => Value::Builtin {
                name: name.clone(),
                func: *func,
            },
            Value::StructDef { name, fields } => Value::StructDef {
                name: name.clone(),
                fields: fields.clone(),
            },
            Value::EnumDef { name, variants } => Value::EnumDef {
                name: name.clone(),
                variants: Arc::clone(variants),
            },
            Value::EnumConstructor {
                enum_name,
                variant_name,
                params,
            } => Value::EnumConstructor {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                params: params.clone(),
            },
            Value::GcArray(r) => Value::GcArray(*r),
            Value::GcMap(r) => Value::GcMap(*r),
            Value::GcClosure(r) => Value::GcClosure(*r),
            Value::GcString(r) => Value::GcString(*r),
            Value::GcInstance(r) => Value::GcInstance(*r),
            _ => {
                let mut visited = HashMap::new();
                self.deep_clone_internal(&mut visited, 0)
            }
        }
    }

    fn deep_clone_internal(&self, visited: &mut HashMap<usize, Value>, depth: usize) -> Value {
        const MAX_DEEP_CLONE_DEPTH: usize = 256;
        if depth > MAX_DEEP_CLONE_DEPTH {
            return self.clone();
        }

        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(*b),
            Value::Int(i) => Value::Int(*i),
            Value::Float(f) => Value::Float(*f),
            Value::Number(n) => Value::Number(*n),
            Value::String(s) => Value::String(s.clone()),

            Value::Array(arr) => {
                let ptr = Arc::as_ptr(arr) as usize;
                if let Some(existing) = visited.get(&ptr) {
                    return existing.clone();
                }

                let new_inner = Arc::new(RwLock::new(Vec::new()));
                let cloned_val = Value::Array(Arc::clone(&new_inner));
                visited.insert(ptr, cloned_val.clone());

                let guard = match arr.read() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let mut items = Vec::with_capacity(guard.len());
                for item in guard.iter() {
                    items.push(item.deep_clone_internal(visited, depth + 1));
                }
                *new_inner.write().unwrap() = items;
                cloned_val
            }

            Value::Map(map) => {
                let ptr = Arc::as_ptr(map) as usize;
                if let Some(existing) = visited.get(&ptr) {
                    return existing.clone();
                }

                let new_inner = Arc::new(RwLock::new(IndexMap::new()));
                let cloned_val = Value::Map(Arc::clone(&new_inner));
                visited.insert(ptr, cloned_val.clone());

                let guard = match map.read() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let mut items = IndexMap::with_capacity(guard.len());
                for (k, v) in guard.iter() {
                    items.insert(k.clone(), v.deep_clone_internal(visited, depth + 1));
                }
                *new_inner.write().unwrap() = items;
                cloned_val
            }

            Value::StructInstance { name, fields } => {
                let ptr = Arc::as_ptr(fields) as usize;
                if let Some(existing) = visited.get(&ptr) {
                    return existing.clone();
                }

                let new_fields = Arc::new(RwLock::new(IndexMap::new()));
                let cloned_val = Value::StructInstance {
                    name: name.clone(),
                    fields: Arc::clone(&new_fields),
                };
                visited.insert(ptr, cloned_val.clone());

                let guard = match fields.read() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let mut items = IndexMap::with_capacity(guard.len());
                for (k, v) in guard.iter() {
                    items.insert(k.clone(), v.deep_clone_internal(visited, depth + 1));
                }
                *new_fields.write().unwrap() = items;
                cloned_val
            }

            Value::EnumInstance {
                enum_name,
                variant_name,
                values,
            } => {
                let cloned_values = values
                    .iter()
                    .map(|v| v.deep_clone_internal(visited, depth + 1))
                    .collect();
                Value::EnumInstance {
                    enum_name: enum_name.clone(),
                    variant_name: variant_name.clone(),
                    values: cloned_values,
                }
            }

            Value::BoundMethod { object, method } => Value::BoundMethod {
                object: Box::new(object.deep_clone_internal(visited, depth + 1)),
                method: method.clone(),
            },

            Value::Closure(c) => {
                let cloned_upvalues = c
                    .upvalues
                    .iter()
                    .map(|u| {
                        let guard = match u.read() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        let new_loc = match &guard.location {
                            UpvalueLocation::Open(slot) => UpvalueLocation::Open(*slot),
                            UpvalueLocation::Closed(val) => {
                                UpvalueLocation::Closed(val.deep_clone_internal(visited, depth + 1))
                            }
                        };
                        Arc::new(RwLock::new(Upvalue { location: new_loc }))
                    })
                    .collect();

                Value::Closure(Arc::new(Closure {
                    function: Arc::clone(&c.function),
                    upvalues: cloned_upvalues,
                    module_globals: c.module_globals.as_ref().map(Arc::clone),
                }))
            }

            Value::Function { .. } => self.clone(),
            Value::Channel(c) => Value::Channel(Arc::clone(c)),
            Value::Task(t) => Value::Task(Arc::clone(t)),
            Value::Actor(a) => Value::Actor(Arc::clone(a)),
            Value::Supervisor(s) => Value::Supervisor(Arc::clone(s)),
            Value::FfiLibrary(l) => Value::FfiLibrary(Arc::clone(l)),
            Value::FfiFunction(f) => Value::FfiFunction(Arc::clone(f)),
            Value::CompiledFunction(f) => Value::CompiledFunction(Arc::clone(f)),
            Value::Builtin { name, func } => Value::Builtin {
                name: name.clone(),
                func: *func,
            },
            Value::StructDef { name, fields } => Value::StructDef {
                name: name.clone(),
                fields: fields.clone(),
            },
            Value::EnumDef { name, variants } => Value::EnumDef {
                name: name.clone(),
                variants: Arc::clone(variants),
            },
            Value::EnumConstructor {
                enum_name,
                variant_name,
                params,
            } => Value::EnumConstructor {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                params: params.clone(),
            },
            Value::GcArray(r) => Value::GcArray(*r),
            Value::GcMap(r) => Value::GcMap(*r),
            Value::GcClosure(r) => Value::GcClosure(*r),
            Value::GcString(r) => Value::GcString(*r),
            Value::GcInstance(r) => Value::GcInstance(*r),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Float(a), Value::Number(b)) | (Value::Number(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Int(a), Value::Number(b)) => {
                (*a as f64) == *b
            }
            (Value::Float(a), Value::Int(b)) | (Value::Number(a), Value::Int(b)) => {
                *a == (*b as f64)
            }
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.read().unwrap() == *b.read().unwrap(),
            (Value::Map(a), Value::Map(b)) => *a.read().unwrap() == *b.read().unwrap(),
            (
                Value::StructInstance {
                    name: n1,
                    fields: f1,
                },
                Value::StructInstance {
                    name: n2,
                    fields: f2,
                },
            ) => n1 == n2 && *f1.read().unwrap() == *f2.read().unwrap(),
            (
                Value::EnumInstance {
                    enum_name: e1,
                    variant_name: v1,
                    values: vals1,
                },
                Value::EnumInstance {
                    enum_name: e2,
                    variant_name: v2,
                    values: vals2,
                },
            ) => e1 == e2 && v1 == v2 && vals1 == vals2,
            (
                Value::StructDef {
                    name: n1,
                    fields: f1,
                },
                Value::StructDef {
                    name: n2,
                    fields: f2,
                },
            ) => n1 == n2 && f1 == f2,
            (
                Value::EnumDef {
                    name: n1,
                    variants: v1,
                },
                Value::EnumDef {
                    name: n2,
                    variants: v2,
                },
            ) => n1 == n2 && v1 == v2,
            (Value::Task(a), Value::Task(b)) => Arc::ptr_eq(a, b),
            (Value::Channel(a), Value::Channel(b)) => Arc::ptr_eq(a, b),
            (Value::FfiLibrary(a), Value::FfiLibrary(b)) => Arc::ptr_eq(a, b),
            (Value::FfiFunction(a), Value::FfiFunction(b)) => Arc::ptr_eq(a, b),
            (Value::Actor(a), Value::Actor(b)) => a == b,
            (Value::Supervisor(a), Value::Supervisor(b)) => a == b,
            (Value::CompiledFunction(a), Value::CompiledFunction(b)) => Arc::ptr_eq(a, b) || a == b,
            (Value::Closure(a), Value::Closure(b)) => Arc::ptr_eq(a, b) || a == b,
            (Value::GcArray(a), Value::GcArray(b)) => a == b,
            (Value::GcMap(a), Value::GcMap(b)) => a == b,
            (Value::GcClosure(a), Value::GcClosure(b)) => a == b,
            (Value::GcString(a), Value::GcString(b)) => a == b,
            (Value::GcInstance(a), Value::GcInstance(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Value::Null => {
                0u8.hash(state);
            }
            Value::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Value::Int(i) => {
                let f = *i as f64;
                if (f as i64) == *i && f.fract() == 0.0 {
                    2u8.hash(state);
                    f.to_bits().hash(state);
                } else {
                    3u8.hash(state);
                    i.hash(state);
                }
            }
            Value::Float(f) | Value::Number(f) => {
                let normalized = if *f == 0.0 { 0.0 } else { *f };
                if normalized.fract() == 0.0
                    && normalized >= (i64::MIN as f64)
                    && normalized <= (i64::MAX as f64)
                    && (normalized as i64 as f64) == normalized
                {
                    2u8.hash(state);
                    normalized.to_bits().hash(state);
                } else {
                    4u8.hash(state);
                    normalized.to_bits().hash(state);
                }
            }
            Value::String(s) => {
                5u8.hash(state);
                s.hash(state);
            }
            Value::Array(a) => {
                6u8.hash(state);
                if let Ok(guard) = a.read() {
                    guard.len().hash(state);
                    for item in guard.iter() {
                        item.hash(state);
                    }
                }
            }
            Value::Map(m) => {
                7u8.hash(state);
                if let Ok(guard) = m.read() {
                    guard.len().hash(state);
                    for (k, v) in guard.iter() {
                        k.hash(state);
                        v.hash(state);
                    }
                }
            }
            Value::CompiledFunction(f) => {
                8u8.hash(state);
                f.name.hash(state);
                f.arity.hash(state);
            }
            Value::Closure(c) => {
                9u8.hash(state);
                c.function.name.hash(state);
                c.function.arity.hash(state);
            }
            Value::GcArray(r) => {
                10u8.hash(state);
                r.hash(state);
            }
            Value::GcMap(r) => {
                11u8.hash(state);
                r.hash(state);
            }
            Value::GcClosure(r) => {
                12u8.hash(state);
                r.hash(state);
            }
            Value::GcString(r) => {
                13u8.hash(state);
                r.hash(state);
            }
            Value::GcInstance(r) => {
                14u8.hash(state);
                r.hash(state);
            }
            Value::Function { name, .. } => {
                15u8.hash(state);
                name.hash(state);
            }
            Value::Builtin { name, .. } => {
                16u8.hash(state);
                name.hash(state);
            }
            Value::BoundMethod { object, method } => {
                17u8.hash(state);
                object.hash(state);
                method.hash(state);
            }
            Value::StructDef { name, fields } => {
                18u8.hash(state);
                name.hash(state);
                fields.hash(state);
            }
            Value::StructInstance { name, fields } => {
                19u8.hash(state);
                name.hash(state);
                if let Ok(guard) = fields.read() {
                    for (k, v) in guard.iter() {
                        k.hash(state);
                        v.hash(state);
                    }
                }
            }
            Value::EnumDef { name, variants } => {
                20u8.hash(state);
                name.hash(state);
                for (k, v) in variants.iter() {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::EnumConstructor {
                enum_name,
                variant_name,
                params,
            } => {
                21u8.hash(state);
                enum_name.hash(state);
                variant_name.hash(state);
                params.hash(state);
            }
            Value::EnumInstance {
                enum_name,
                variant_name,
                values,
            } => {
                22u8.hash(state);
                enum_name.hash(state);
                variant_name.hash(state);
                values.hash(state);
            }
            Value::Task(t) => {
                23u8.hash(state);
                Arc::as_ptr(t).hash(state);
            }
            Value::Channel(c) => {
                24u8.hash(state);
                Arc::as_ptr(c).hash(state);
            }
            Value::FfiLibrary(l) => {
                25u8.hash(state);
                l.path.hash(state);
            }
            Value::FfiFunction(f) => {
                26u8.hash(state);
                f.name.hash(state);
            }
            Value::Actor(a) => {
                27u8.hash(state);
                a.hash(state);
            }
            Value::Supervisor(s) => {
                28u8.hash(state);
                s.hash(state);
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display())
    }
}

pub enum IndexError {
    NotWhole,
    OutOfRange(usize),
}

pub fn resolve_int_index(n: i64, len: usize) -> Result<usize, IndexError> {
    let mut idx = n as isize;
    if idx < 0 {
        idx += len as isize;
    }
    if idx < 0 || idx >= len as isize {
        return Err(IndexError::OutOfRange(len));
    }
    Ok(idx as usize)
}

pub fn resolve_index(n: f64, len: usize) -> Result<usize, IndexError> {
    if n.fract() != 0.0 {
        return Err(IndexError::NotWhole);
    }
    let mut idx = n as isize;
    if idx < 0 {
        idx += len as isize;
    }
    if idx < 0 || idx >= len as isize {
        return Err(IndexError::OutOfRange(len));
    }
    Ok(idx as usize)
}
