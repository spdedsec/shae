use crate::ast::Stmt;
use crate::env::Environment;
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type BuiltinFn = fn(Vec<Value>, Rc<RefCell<Environment>>) -> Result<Value, String>;

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<HashMap<String, Value>>>),
    Function {
        name: Option<String>,
        params: Vec<String>,
        body: Vec<Stmt>,
        closure: Rc<RefCell<Environment>>,
    },
    Builtin {
        name: String,
        func: BuiltinFn,
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
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
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
                let mut map = HashMap::new();
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
            (Value::Number(a), Value::Number(b)) => (a - b).abs() < f64::EPSILON,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
            (Value::Map(a), Value::Map(b)) => *a.borrow() == *b.borrow(),
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
