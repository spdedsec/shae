use crate::ast::Stmt;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

pub type BuiltinFn =
    fn(&mut Evaluator, Vec<Value>, crate::ast::Span) -> Result<Value, RuntimeError>;

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<IndexMap<String, Value>>>),
    Function {
        name: Option<String>,
        params: Rc<Vec<String>>,
        body: Rc<Vec<Stmt>>,
        closure: Rc<RefCell<Environment>>,
    },
Builtin {
        name: String,
        func: BuiltinFn,
    },
    StructDef {
        name: String,
        fields: Vec<String>,
    },
    StructInstance {
        name: String,
        fields: Rc<RefCell<IndexMap<String, Value>>>,
    },
    EnumDef {
        name: String,
        variants: Rc<IndexMap<String, Vec<String>>>,
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
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Function { .. } => "function",
Value::Builtin { .. } => "builtin_function",
            Value::StructDef { .. } => "struct_def",
            Value::StructInstance { name: _, .. } => "struct_instance",
            Value::EnumDef { .. } => "enum_def",
            Value::EnumConstructor { .. } => "enum_constructor",
            Value::EnumInstance { .. } => "enum_instance",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Map(m) => !m.borrow().is_empty(),
Value::Function { .. } | Value::Builtin { .. } => true,
            Value::StructDef { .. } | Value::StructInstance { .. } => true,
            Value::EnumDef { .. } | Value::EnumConstructor { .. } | Value::EnumInstance { .. } => true,
        }
    }

    pub fn to_display(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => s.clone(),
            Value::Array(a) => {
                let items: Vec<String> = a.borrow().iter().map(|v| v.to_repr()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let entries: Vec<String> = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_repr()))
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Value::Function { name, .. } => {
                if let Some(n) = name {
                    format!("<fn {}>", n)
                } else {
                    "<fn anonymous>".to_string()
                }
            }
Value::Builtin { name, .. } => format!("<builtin {}>", name),
            Value::StructDef { name, .. } => format!("<struct {}>", name),
            Value::StructInstance { name, fields } => {
                let map = fields.borrow();
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
            Value::Number(n) => {
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
                let list: Vec<JsonValue> = a.borrow().iter().map(|v| v.to_json()).collect();
                JsonValue::Array(list)
            }
            Value::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m.borrow().iter() {
                    map.insert(k.clone(), v.to_json());
                }
                JsonValue::Object(map)
            }
            Value::Function { name, .. } => {
                JsonValue::String(format!("<fn {}>", name.as_deref().unwrap_or("anon")))
            }
Value::Builtin { name, .. } => JsonValue::String(format!("<builtin {}>", name)),
            Value::StructDef { name, .. } => JsonValue::String(format!("<struct {}>", name)),
            Value::StructInstance { name, fields } => {
                let mut map = serde_json::Map::new();
                map.insert("__type__".to_string(), JsonValue::String(name.clone()));
                for (k, v) in fields.borrow().iter() {
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
        }
    }

    pub fn from_json(json: &JsonValue) -> Value {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Bool(*b),
            JsonValue::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
            JsonValue::String(s) => Value::String(s.clone()),
            JsonValue::Array(arr) => {
                let list: Vec<Value> = arr.iter().map(Value::from_json).collect();
                Value::Array(Rc::new(RefCell::new(list)))
            }
            JsonValue::Object(obj) => {
                let mut map = IndexMap::new();
                for (k, v) in obj {
                    map.insert(k.clone(), Value::from_json(v));
                }
                Value::Map(Rc::new(RefCell::new(map)))
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b, // exact float equality
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
(Value::Map(a), Value::Map(b)) => *a.borrow() == *b.borrow(),
            (Value::StructInstance { name: n1, fields: f1 }, Value::StructInstance { name: n2, fields: f2 }) => {
                n1 == n2 && *f1.borrow() == *f2.borrow()
            }
            (Value::EnumInstance { enum_name: e1, variant_name: v1, values: vals1 }, Value::EnumInstance { enum_name: e2, variant_name: v2, values: vals2 }) => {
                e1 == e2 && v1 == v2 && vals1 == vals2
            }
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
