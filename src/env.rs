use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Debug)]
pub struct Environment {
    values: HashMap<String, Value>,
    parent: Option<Arc<RwLock<Environment>>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            parent: None,
        }
    }

    pub fn new_with_parent(parent: Arc<RwLock<Environment>>) -> Self {
        Self {
            values: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.values.insert(name, value);
    }

    pub fn set(&mut self, name: &str, value: Value) -> bool {
        if self.values.contains_key(name) {
            self.values.insert(name.to_string(), value);
            true
        } else if let Some(parent) = &self.parent {
            parent.write().unwrap().set(name, value)
        } else {
            false
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(val) = self.values.get(name) {
            Some(val.clone())
        } else if let Some(parent) = &self.parent {
            parent.read().unwrap().get(name)
        } else {
            None
        }
    }

    pub fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.values.keys().cloned().collect();
        if let Some(parent) = &self.parent {
            names.extend(parent.read().unwrap().all_names());
        }
        names
    }

    pub fn export_map(&self) -> indexmap::IndexMap<String, Value> {
        let mut map = indexmap::IndexMap::new();
        for (k, v) in &self.values {
            map.insert(k.clone(), v.clone());
        }
        map
    }
}
