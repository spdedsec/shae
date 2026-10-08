use crate::ast::Stmt;
use crate::chunk::Chunk;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::gc::GcRef;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use std::sync::RwLock;
use std::sync::Mutex;
use std::fmt;
use std::sync::Arc;

pub type BuiltinFn =
    fn(&mut Evaluator, Vec<Value>, crate::ast::Span) -> Result<Value, RuntimeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpvalueDesc {
    pub index: u8,
    pub is_local: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFunction {
    pub arity: usize,
    pub chunk: Chunk,
    pub name: Option<String>,
    pub upvalues: Vec<UpvalueDesc>,
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
            Value::StructInstance { name: _, .. } => "struct_instance",
            Value::EnumDef { .. } => "enum_def",
            Value::EnumConstructor { .. } => "enum_constructor",
            Value::EnumInstance { .. } => "enum_instance",
            Value::Task(_) => "task",
        }
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
            Value::GcArray(_) | Value::GcMap(_) | Value::GcClosure(_) | Value::GcString(_) | Value::GcInstance(_) => true,
Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. } => true,
            Value::StructDef { .. } | Value::StructInstance { .. } => true,
            Value::EnumDef { .. } | Value::EnumConstructor { .. } | Value::EnumInstance { .. } | Value::Task(_) => true,
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
                    .read().unwrap()
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
                    if !first { out.push_str(", "); }
                    out.push_str(k);
                    out.push_str(": ");
                    out.push_str(&v.to_repr());
                    first = false;
                }
                out.push_str(" }");
                out
            }
            Value::EnumDef { name, .. } => format!("<enum {}>", name),
            Value::EnumConstructor { enum_name, variant_name, .. } => format!("<constructor {}.{}>", enum_name, variant_name),
            Value::EnumInstance { enum_name, variant_name, values } => {
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
            Value::Closure(c) => {
                JsonValue::String(format!("<fn {}>", c.function.name.as_deref().unwrap_or("anon")))
            }
            Value::Function { name, .. } => {
                JsonValue::String(format!("<fn {}>", name.as_deref().unwrap_or("anon")))
            }
Value::Builtin { name, .. } => JsonValue::String(format!("<builtin {}>", name)),
            Value::BoundMethod { method, .. } => JsonValue::String(format!("<bound method {}>", method)),
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
            Value::EnumConstructor { enum_name, variant_name, .. } => JsonValue::String(format!("<constructor {}.{}>", enum_name, variant_name)),
            Value::EnumInstance { enum_name, variant_name, values } => {
                let mut map = serde_json::Map::new();
                map.insert("__enum__".to_string(), JsonValue::String(enum_name.clone()));
                map.insert("__variant__".to_string(), JsonValue::String(variant_name.clone()));
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
            (Value::Int(a), Value::Float(b)) | (Value::Int(a), Value::Number(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) | (Value::Number(a), Value::Int(b)) => *a == (*b as f64),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.read().unwrap() == *b.read().unwrap(),
(Value::Map(a), Value::Map(b)) => *a.read().unwrap() == *b.read().unwrap(),
            (Value::StructInstance { name: n1, fields: f1 }, Value::StructInstance { name: n2, fields: f2 }) => {
                n1 == n2 && *f1.read().unwrap() == *f2.read().unwrap()
            }
            (Value::EnumInstance { enum_name: e1, variant_name: v1, values: vals1 }, Value::EnumInstance { enum_name: e2, variant_name: v2, values: vals2 }) => {
                e1 == e2 && v1 == v2 && vals1 == vals2
            }
            (Value::Task(a), Value::Task(b)) => Arc::ptr_eq(a, b),
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
