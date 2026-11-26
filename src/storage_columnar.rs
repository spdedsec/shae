// Shae Storage Engine: In-Memory Columnar Vector Tables & Fast Analytical Scans
// Part of Option 5: Embedded LSM-Tree & High-Speed Storage Engine

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

// ---------------------------------------------------------------------------
// Column Data Types & Filter/Agg Operators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Float,
    Str,
    Bool,
}

impl ColumnType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "int" | "integer" | "i64" => Some(ColumnType::Int),
            "float" | "double" | "f64" => Some(ColumnType::Float),
            "str" | "string" | "text" => Some(ColumnType::Str),
            "bool" | "boolean" => Some(ColumnType::Bool),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ColumnType::Int => "int",
            ColumnType::Float => "float",
            ColumnType::Str => "string",
            ColumnType::Bool => "bool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
}

impl FilterOp {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "==" | "=" | "eq" => Some(FilterOp::Eq),
            "!=" | "<>" | "neq" => Some(FilterOp::Neq),
            ">" | "gt" => Some(FilterOp::Gt),
            ">=" | "gte" => Some(FilterOp::Gte),
            "<" | "lt" => Some(FilterOp::Lt),
            "<=" | "lte" => Some(FilterOp::Lte),
            "contains" | "like" => Some(FilterOp::Contains),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggOp {
    Sum,
    Avg,
    Min,
    Max,
    Count,
}

impl AggOp {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sum" => Some(AggOp::Sum),
            "avg" | "mean" => Some(AggOp::Avg),
            "min" => Some(AggOp::Min),
            "max" => Some(AggOp::Max),
            "count" => Some(AggOp::Count),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Column Vector Data Structure
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ColumnVector {
    Int { values: Vec<i64>, nulls: Vec<bool> },
    Float { values: Vec<f64>, nulls: Vec<bool> },
    Str { values: Vec<String>, nulls: Vec<bool> },
    Bool { values: Vec<bool>, nulls: Vec<bool> },
}

impl ColumnVector {
    pub fn new(col_type: ColumnType, capacity: usize) -> Self {
        match col_type {
            ColumnType::Int => ColumnVector::Int {
                values: Vec::with_capacity(capacity),
                nulls: Vec::with_capacity(capacity),
            },
            ColumnType::Float => ColumnVector::Float {
                values: Vec::with_capacity(capacity),
                nulls: Vec::with_capacity(capacity),
            },
            ColumnType::Str => ColumnVector::Str {
                values: Vec::with_capacity(capacity),
                nulls: Vec::with_capacity(capacity),
            },
            ColumnType::Bool => ColumnVector::Bool {
                values: Vec::with_capacity(capacity),
                nulls: Vec::with_capacity(capacity),
            },
        }
    }

    pub fn col_type(&self) -> ColumnType {
        match self {
            ColumnVector::Int { .. } => ColumnType::Int,
            ColumnVector::Float { .. } => ColumnType::Float,
            ColumnVector::Str { .. } => ColumnType::Str,
            ColumnVector::Bool { .. } => ColumnType::Bool,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            ColumnVector::Int { values, .. } => values.len(),
            ColumnVector::Float { values, .. } => values.len(),
            ColumnVector::Str { values, .. } => values.len(),
            ColumnVector::Bool { values, .. } => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push_val(&mut self, val: &Value) {
        match self {
            ColumnVector::Int { values, nulls } => match val {
                Value::Int(i) => {
                    values.push(*i);
                    nulls.push(false);
                }
                Value::Float(f) => {
                    values.push(*f as i64);
                    nulls.push(false);
                }
                Value::Null => {
                    values.push(0);
                    nulls.push(true);
                }
                _ => {
                    values.push(0);
                    nulls.push(true);
                }
            },
            ColumnVector::Float { values, nulls } => match val {
                Value::Float(f) => {
                    values.push(*f);
                    nulls.push(false);
                }
                Value::Int(i) => {
                    values.push(*i as f64);
                    nulls.push(false);
                }
                Value::Null => {
                    values.push(0.0);
                    nulls.push(true);
                }
                _ => {
                    values.push(0.0);
                    nulls.push(true);
                }
            },
            ColumnVector::Str { values, nulls } => match val {
                Value::String(s) => {
                    values.push(s.clone());
                    nulls.push(false);
                }
                Value::Null => {
                    values.push(String::new());
                    nulls.push(true);
                }
                other => {
                    values.push(format!("{}", other));
                    nulls.push(false);
                }
            },
            ColumnVector::Bool { values, nulls } => match val {
                Value::Bool(b) => {
                    values.push(*b);
                    nulls.push(false);
                }
                Value::Null => {
                    values.push(false);
                    nulls.push(true);
                }
                _ => {
                    values.push(false);
                    nulls.push(true);
                }
            },
        }
    }

    pub fn get_val(&self, row: usize) -> Value {
        match self {
            ColumnVector::Int { values, nulls } => {
                if nulls.get(row).copied().unwrap_or(true) {
                    Value::Null
                } else {
                    Value::Int(values[row])
                }
            }
            ColumnVector::Float { values, nulls } => {
                if nulls.get(row).copied().unwrap_or(true) {
                    Value::Null
                } else {
                    Value::Float(values[row])
                }
            }
            ColumnVector::Str { values, nulls } => {
                if nulls.get(row).copied().unwrap_or(true) {
                    Value::Null
                } else {
                    Value::String(values[row].clone())
                }
            }
            ColumnVector::Bool { values, nulls } => {
                if nulls.get(row).copied().unwrap_or(true) {
                    Value::Null
                } else {
                    Value::Bool(values[row])
                }
            }
        }
    }

    /// Evaluates vectorized filter and returns a boolean selection bitmask
    pub fn filter_mask(&self, op: FilterOp, target: &Value) -> Vec<bool> {
        let n = self.len();
        let mut mask = vec![false; n];

        match self {
            ColumnVector::Int { values, nulls } => {
                let target_i = match target {
                    Value::Int(i) => Some(*i),
                    Value::Float(f) => Some(*f as i64),
                    _ => None,
                };

                if let Some(t) = target_i {
                    for i in 0..n {
                        if !nulls[i] {
                            let v = values[i];
                            mask[i] = match op {
                                FilterOp::Eq => v == t,
                                FilterOp::Neq => v != t,
                                FilterOp::Gt => v > t,
                                FilterOp::Gte => v >= t,
                                FilterOp::Lt => v < t,
                                FilterOp::Lte => v <= t,
                                FilterOp::Contains => false,
                            };
                        }
                    }
                }
            }
            ColumnVector::Float { values, nulls } => {
                let target_f = match target {
                    Value::Float(f) => Some(*f),
                    Value::Int(i) => Some(*i as f64),
                    _ => None,
                };

                if let Some(t) = target_f {
                    for i in 0..n {
                        if !nulls[i] {
                            let v = values[i];
                            mask[i] = match op {
                                FilterOp::Eq => (v - t).abs() < f64::EPSILON,
                                FilterOp::Neq => (v - t).abs() >= f64::EPSILON,
                                FilterOp::Gt => v > t,
                                FilterOp::Gte => v >= t,
                                FilterOp::Lt => v < t,
                                FilterOp::Lte => v <= t,
                                FilterOp::Contains => false,
                            };
                        }
                    }
                }
            }
            ColumnVector::Str { values, nulls } => {
                let target_s = match target {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                };

                if let Some(t) = target_s {
                    for i in 0..n {
                        if !nulls[i] {
                            let v = values[i].as_str();
                            mask[i] = match op {
                                FilterOp::Eq => v == t,
                                FilterOp::Neq => v != t,
                                FilterOp::Gt => v > t,
                                FilterOp::Gte => v >= t,
                                FilterOp::Lt => v < t,
                                FilterOp::Lte => v <= t,
                                FilterOp::Contains => v.contains(t),
                            };
                        }
                    }
                }
            }
            ColumnVector::Bool { values, nulls } => {
                if let Value::Bool(t) = target {
                    for i in 0..n {
                        if !nulls[i] {
                            let v = values[i];
                            mask[i] = match op {
                                FilterOp::Eq => v == *t,
                                FilterOp::Neq => v != *t,
                                _ => false,
                            };
                        }
                    }
                }
            }
        }

        mask
    }

    /// Evaluates aggregate over column with an optional selection mask
    pub fn aggregate(&self, op: AggOp, mask: Option<&[bool]>) -> Value {
        let n = self.len();

        match op {
            AggOp::Count => {
                let mut count = 0i64;
                for i in 0..n {
                    let active = mask.map_or(true, |m| m.get(i).copied().unwrap_or(false));
                    if active {
                        let is_null = match self {
                            ColumnVector::Int { nulls, .. } => nulls[i],
                            ColumnVector::Float { nulls, .. } => nulls[i],
                            ColumnVector::Str { nulls, .. } => nulls[i],
                            ColumnVector::Bool { nulls, .. } => nulls[i],
                        };
                        if !is_null {
                            count += 1;
                        }
                    }
                }
                Value::Int(count)
            }
            AggOp::Sum => match self {
                ColumnVector::Int { values, nulls } => {
                    let mut sum = 0i64;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            sum += values[i];
                        }
                    }
                    Value::Int(sum)
                }
                ColumnVector::Float { values, nulls } => {
                    let mut sum = 0.0f64;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            sum += values[i];
                        }
                    }
                    Value::Float(sum)
                }
                _ => Value::Null,
            },
            AggOp::Avg => match self {
                ColumnVector::Int { values, nulls } => {
                    let mut sum = 0.0f64;
                    let mut count = 0usize;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            sum += values[i] as f64;
                            count += 1;
                        }
                    }
                    if count > 0 {
                        Value::Float(sum / count as f64)
                    } else {
                        Value::Null
                    }
                }
                ColumnVector::Float { values, nulls } => {
                    let mut sum = 0.0f64;
                    let mut count = 0usize;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            sum += values[i];
                            count += 1;
                        }
                    }
                    if count > 0 {
                        Value::Float(sum / count as f64)
                    } else {
                        Value::Null
                    }
                }
                _ => Value::Null,
            },
            AggOp::Min => match self {
                ColumnVector::Int { values, nulls } => {
                    let mut min_val: Option<i64> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i];
                            min_val = Some(min_val.map_or(v, |m| m.min(v)));
                        }
                    }
                    min_val.map_or(Value::Null, Value::Int)
                }
                ColumnVector::Float { values, nulls } => {
                    let mut min_val: Option<f64> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i];
                            min_val = Some(min_val.map_or(v, |m| m.min(v)));
                        }
                    }
                    min_val.map_or(Value::Null, Value::Float)
                }
                ColumnVector::Str { values, nulls } => {
                    let mut min_val: Option<&str> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i].as_str();
                            min_val = Some(min_val.map_or(v, |m| m.min(v)));
                        }
                    }
                    min_val.map_or(Value::Null, |s| Value::String(s.to_string()))
                }
                _ => Value::Null,
            },
            AggOp::Max => match self {
                ColumnVector::Int { values, nulls } => {
                    let mut max_val: Option<i64> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i];
                            max_val = Some(max_val.map_or(v, |m| m.max(v)));
                        }
                    }
                    max_val.map_or(Value::Null, Value::Int)
                }
                ColumnVector::Float { values, nulls } => {
                    let mut max_val: Option<f64> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i];
                            max_val = Some(max_val.map_or(v, |m| m.max(v)));
                        }
                    }
                    max_val.map_or(Value::Null, Value::Float)
                }
                ColumnVector::Str { values, nulls } => {
                    let mut max_val: Option<&str> = None;
                    for i in 0..n {
                        if !nulls[i] && mask.map_or(true, |m| m.get(i).copied().unwrap_or(false)) {
                            let v = values[i].as_str();
                            max_val = Some(max_val.map_or(v, |m| m.max(v)));
                        }
                    }
                    max_val.map_or(Value::Null, |s| Value::String(s.to_string()))
                }
                _ => Value::Null,
            },
        }
    }

    /// Slice elements matching boolean mask
    pub fn filter_by_mask(&self, mask: &[bool]) -> Self {
        match self {
            ColumnVector::Int { values, nulls } => {
                let mut new_vals = Vec::new();
                let mut new_nulls = Vec::new();
                for i in 0..values.len() {
                    if mask.get(i).copied().unwrap_or(false) {
                        new_vals.push(values[i]);
                        new_nulls.push(nulls[i]);
                    }
                }
                ColumnVector::Int {
                    values: new_vals,
                    nulls: new_nulls,
                }
            }
            ColumnVector::Float { values, nulls } => {
                let mut new_vals = Vec::new();
                let mut new_nulls = Vec::new();
                for i in 0..values.len() {
                    if mask.get(i).copied().unwrap_or(false) {
                        new_vals.push(values[i]);
                        new_nulls.push(nulls[i]);
                    }
                }
                ColumnVector::Float {
                    values: new_vals,
                    nulls: new_nulls,
                }
            }
            ColumnVector::Str { values, nulls } => {
                let mut new_vals = Vec::new();
                let mut new_nulls = Vec::new();
                for i in 0..values.len() {
                    if mask.get(i).copied().unwrap_or(false) {
                        new_vals.push(values[i].clone());
                        new_nulls.push(nulls[i]);
                    }
                }
                ColumnVector::Str {
                    values: new_vals,
                    nulls: new_nulls,
                }
            }
            ColumnVector::Bool { values, nulls } => {
                let mut new_vals = Vec::new();
                let mut new_nulls = Vec::new();
                for i in 0..values.len() {
                    if mask.get(i).copied().unwrap_or(false) {
                        new_vals.push(values[i]);
                        new_nulls.push(nulls[i]);
                    }
                }
                ColumnVector::Bool {
                    values: new_vals,
                    nulls: new_nulls,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Columnar Table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ColumnarTable {
    pub schema: IndexMap<String, ColumnType>,
    pub columns: IndexMap<String, ColumnVector>,
    pub row_count: usize,
}

impl ColumnarTable {
    pub fn new(schema: IndexMap<String, ColumnType>) -> Self {
        let mut columns = IndexMap::new();
        for (name, col_type) in &schema {
            columns.insert(name.clone(), ColumnVector::new(*col_type, 64));
        }

        ColumnarTable {
            schema,
            columns,
            row_count: 0,
        }
    }

    pub fn insert_row(&mut self, row: &IndexMap<String, Value>) {
        for (col_name, vector) in &mut self.columns {
            let val = row.get(col_name).unwrap_or(&Value::Null);
            vector.push_val(val);
        }
        self.row_count += 1;
    }

    pub fn insert_batch(&mut self, batch: &IndexMap<String, Vec<Value>>) -> usize {
        let mut max_inserted = 0;
        for (col_name, vals) in batch {
            if let Some(col) = self.columns.get_mut(col_name) {
                for v in vals {
                    col.push_val(v);
                }
                max_inserted = max_inserted.max(vals.len());
            }
        }
        self.row_count += max_inserted;
        self.row_count
    }

    pub fn filter(&self, col_name: &str, op: FilterOp, target: &Value) -> Result<Self, String> {
        let col = self
            .columns
            .get(col_name)
            .ok_or_else(|| format!("Unknown column '{}'", col_name))?;

        let mask = col.filter_mask(op, target);
        let mut new_cols = IndexMap::new();
        let mut new_count = 0;

        for (name, vector) in &self.columns {
            let filtered_col = vector.filter_by_mask(&mask);
            new_count = filtered_col.len();
            new_cols.insert(name.clone(), filtered_col);
        }

        Ok(ColumnarTable {
            schema: self.schema.clone(),
            columns: new_cols,
            row_count: new_count,
        })
    }

    pub fn aggregate(&self, agg_specs: &IndexMap<String, (String, AggOp)>) -> Result<IndexMap<String, Value>, String> {
        let mut result = IndexMap::new();

        for (out_name, (col_name, op)) in agg_specs {
            let col = self
                .columns
                .get(col_name)
                .ok_or_else(|| format!("Unknown column '{}' in aggregation", col_name))?;

            let val = col.aggregate(*op, None);
            result.insert(out_name.clone(), val);
        }

        Ok(result)
    }

    pub fn group_by(
        &self,
        group_col_name: &str,
        agg_specs: &IndexMap<String, (String, AggOp)>,
    ) -> Result<Vec<IndexMap<String, Value>>, String> {
        let group_col = self
            .columns
            .get(group_col_name)
            .ok_or_else(|| format!("Unknown group column '{}'", group_col_name))?;

        // 1. Collect distinct groups and their row index lists
        let mut groups: IndexMap<String, Vec<usize>> = IndexMap::new();

        for i in 0..self.row_count {
            let key_val = group_col.get_val(i);
            let key_str = format!("{}", key_val);
            groups.entry(key_str).or_default().push(i);
        }

        // 2. Compute aggregate values per group
        let mut out_rows = Vec::new();

        for (group_key_str, row_indices) in groups {
            let mut row_map = IndexMap::new();

            // Set the group key value from first matching row
            if let Some(&first_idx) = row_indices.first() {
                row_map.insert(group_col_name.to_string(), group_col.get_val(first_idx));
            } else {
                row_map.insert(group_col_name.to_string(), Value::String(group_key_str));
            }

            // Construct boolean mask for this group
            let mut mask = vec![false; self.row_count];
            for &idx in &row_indices {
                mask[idx] = true;
            }

            // Evaluate aggregates over this group's mask
            for (out_name, (col_name, op)) in agg_specs {
                let col = self
                    .columns
                    .get(col_name)
                    .ok_or_else(|| format!("Unknown column '{}' in group aggregation", col_name))?;

                let agg_val = col.aggregate(*op, Some(&mask));
                row_map.insert(out_name.clone(), agg_val);
            }

            out_rows.push(row_map);
        }

        Ok(out_rows)
    }

    pub fn to_shae_rows(&self, limit: Option<usize>) -> Value {
        let max_rows = limit.unwrap_or(self.row_count).min(self.row_count);
        let mut rows = Vec::with_capacity(max_rows);

        for i in 0..max_rows {
            let mut row = IndexMap::new();
            for (col_name, col) in &self.columns {
                row.insert(col_name.clone(), col.get_val(i));
            }
            rows.push(Value::Map(Arc::new(RwLock::new(row))));
        }

        Value::Array(Arc::new(RwLock::new(rows)))
    }
}

// ---------------------------------------------------------------------------
// Global Registry for Shae Scripts
// ---------------------------------------------------------------------------

static NEXT_TBL_ID: AtomicUsize = AtomicUsize::new(1);
static TBL_INSTANCES: OnceLock<RwLock<HashMap<usize, Arc<Mutex<ColumnarTable>>>>> = OnceLock::new();

fn get_tbl_registry() -> &'static RwLock<HashMap<usize, Arc<Mutex<ColumnarTable>>>> {
    TBL_INSTANCES.get_or_init(|| RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Standard Library Module (`std:db:columnar` or `std:columnar`)
// ---------------------------------------------------------------------------

pub fn build_columnar_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("create", columnar_create),
        ("insert", columnar_insert),
        ("insert_batch", columnar_insert_batch),
        ("filter", columnar_filter),
        ("aggregate", columnar_aggregate),
        ("group_by", columnar_group_by),
        ("to_rows", columnar_to_rows),
        ("count", columnar_count),
        ("close", columnar_close),
        // Aliases
        ("columnar_create", columnar_create),
        ("columnar_insert", columnar_insert),
        ("columnar_filter", columnar_filter),
        ("columnar_aggregate", columnar_aggregate),
        ("columnar_group_by", columnar_group_by),
        ("columnar_to_rows", columnar_to_rows),
        ("columnar_count", columnar_count),
        ("columnar_close", columnar_close),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("columnar.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn columnar_create(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.create", 1, &args, span)?;

    let schema_map = match &args[0] {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return Err(RuntimeError::new("columnar.create: expected schema map { col_name: 'type' }".to_string()).at(span)),
    };

    let mut schema = IndexMap::new();
    for (k, v) in schema_map {
        let type_str = match v {
            Value::String(s) => s,
            _ => return Err(RuntimeError::new(format!("columnar.create: column '{}' type must be a string", k)).at(span)),
        };
        let col_type = ColumnType::from_str(&type_str).ok_or_else(|| {
            RuntimeError::new(format!("columnar.create: unknown type '{}' for column '{}'", type_str, k)).at(span)
        })?;
        schema.insert(k, col_type);
    }

    let tbl = ColumnarTable::new(schema);
    let id = NEXT_TBL_ID.fetch_add(1, Ordering::SeqCst);
    get_tbl_registry()
        .write()
        .unwrap()
        .insert(id, Arc::new(Mutex::new(tbl)));

    Ok(Value::Int(id as i64))
}

fn columnar_insert(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.insert", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.insert: expected table id (int)".to_string()).at(span)),
    };

    let row = match &args[1] {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return Err(RuntimeError::new("columnar.insert: expected row map".to_string()).at(span)),
    };

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.insert: invalid table id {}", id)).at(span)
    })?;

    let mut tbl = tbl_arc.lock().unwrap();
    tbl.insert_row(&row);

    Ok(Value::Int(tbl.row_count as i64))
}

fn columnar_insert_batch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.insert_batch", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.insert_batch: expected table id (int)".to_string()).at(span)),
    };

    let batch_map = match &args[1] {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return Err(RuntimeError::new("columnar.insert_batch: expected map of column arrays".to_string()).at(span)),
    };

    let mut batch = IndexMap::new();
    for (k, v) in batch_map {
        let arr = match v {
            Value::Array(a) => a.read().unwrap().clone(),
            _ => return Err(RuntimeError::new(format!("columnar.insert_batch: column '{}' must be an array", k)).at(span)),
        };
        batch.insert(k, arr);
    }

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.insert_batch: invalid table id {}", id)).at(span)
    })?;

    let mut tbl = tbl_arc.lock().unwrap();
    let count = tbl.insert_batch(&batch);

    Ok(Value::Int(count as i64))
}

fn columnar_filter(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.filter", 4, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.filter: expected table id (int)".to_string()).at(span)),
    };

    let col_name = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("columnar.filter: expected column name string".to_string()).at(span)),
    };

    let op_str = match &args[2] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("columnar.filter: expected operator string ('==', '>', etc.)".to_string()).at(span)),
    };

    let op = FilterOp::from_str(op_str).ok_or_else(|| {
        RuntimeError::new(format!("columnar.filter: unknown operator '{}'", op_str)).at(span)
    })?;

    let target_val = &args[3];

    let tbl_arc = {
        let registry = get_tbl_registry().read().unwrap();
        registry.get(&id).cloned().ok_or_else(|| {
            RuntimeError::new(format!("columnar.filter: invalid table id {}", id)).at(span)
        })?
    };

    let tbl = tbl_arc.lock().unwrap();
    let new_tbl = tbl.filter(col_name, op, target_val).map_err(|e| {
        RuntimeError::new(format!("columnar.filter failed: {}", e)).at(span)
    })?;

    let new_id = NEXT_TBL_ID.fetch_add(1, Ordering::SeqCst);
    get_tbl_registry()
        .write()
        .unwrap()
        .insert(new_id, Arc::new(Mutex::new(new_tbl)));

    Ok(Value::Int(new_id as i64))
}

fn columnar_aggregate(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.aggregate", 2, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.aggregate: expected table id (int)".to_string()).at(span)),
    };

    let spec_map = match &args[1] {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return Err(RuntimeError::new("columnar.aggregate: expected agg spec map { out_col: [in_col, op] }".to_string()).at(span)),
    };

    let mut agg_specs = IndexMap::new();
    for (out_name, spec_val) in spec_map {
        let (in_col, op_str) = match spec_val {
            Value::Array(a) => {
                let arr = a.read().unwrap();
                if arr.len() != 2 {
                    return Err(RuntimeError::new("columnar.aggregate: spec array must have 2 elements [col, op]".to_string()).at(span));
                }
                let col = match &arr[0] {
                    Value::String(s) => s.clone(),
                    _ => return Err(RuntimeError::new("columnar.aggregate: column name must be string".to_string()).at(span)),
                };
                let op = match &arr[1] {
                    Value::String(s) => s.clone(),
                    _ => return Err(RuntimeError::new("columnar.aggregate: op must be string ('sum', 'avg', etc.)".to_string()).at(span)),
                };
                (col, op)
            }
            _ => return Err(RuntimeError::new("columnar.aggregate: spec values must be arrays [col, op]".to_string()).at(span)),
        };

        let agg_op = AggOp::from_str(&op_str).ok_or_else(|| {
            RuntimeError::new(format!("columnar.aggregate: unknown agg op '{}'", op_str)).at(span)
        })?;

        agg_specs.insert(out_name, (in_col, agg_op));
    }

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.aggregate: invalid table id {}", id)).at(span)
    })?;

    let tbl = tbl_arc.lock().unwrap();
    let result = tbl.aggregate(&agg_specs).map_err(|e| {
        RuntimeError::new(format!("columnar.aggregate failed: {}", e)).at(span)
    })?;

    Ok(Value::Map(Arc::new(RwLock::new(result))))
}

fn columnar_group_by(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.group_by", 3, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.group_by: expected table id (int)".to_string()).at(span)),
    };

    let group_col = match &args[1] {
        Value::String(s) => s.as_str(),
        _ => return Err(RuntimeError::new("columnar.group_by: expected group column name string".to_string()).at(span)),
    };

    let spec_map = match &args[2] {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return Err(RuntimeError::new("columnar.group_by: expected agg spec map { out_col: [in_col, op] }".to_string()).at(span)),
    };

    let mut agg_specs = IndexMap::new();
    for (out_name, spec_val) in spec_map {
        let (in_col, op_str) = match spec_val {
            Value::Array(a) => {
                let arr = a.read().unwrap();
                if arr.len() != 2 {
                    return Err(RuntimeError::new("columnar.group_by: spec array must have 2 elements [col, op]".to_string()).at(span));
                }
                let col = match &arr[0] {
                    Value::String(s) => s.clone(),
                    _ => return Err(RuntimeError::new("columnar.group_by: column name must be string".to_string()).at(span)),
                };
                let op = match &arr[1] {
                    Value::String(s) => s.clone(),
                    _ => return Err(RuntimeError::new("columnar.group_by: op must be string".to_string()).at(span)),
                };
                (col, op)
            }
            _ => return Err(RuntimeError::new("columnar.group_by: spec values must be arrays [col, op]".to_string()).at(span)),
        };

        let agg_op = AggOp::from_str(&op_str).ok_or_else(|| {
            RuntimeError::new(format!("columnar.group_by: unknown agg op '{}'", op_str)).at(span)
        })?;

        agg_specs.insert(out_name, (in_col, agg_op));
    }

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.group_by: invalid table id {}", id)).at(span)
    })?;

    let tbl = tbl_arc.lock().unwrap();
    let rows = tbl.group_by(group_col, &agg_specs).map_err(|e| {
        RuntimeError::new(format!("columnar.group_by failed: {}", e)).at(span)
    })?;

    let values: Vec<Value> = rows
        .into_iter()
        .map(|r| Value::Map(Arc::new(RwLock::new(r))))
        .collect();

    Ok(Value::Array(Arc::new(RwLock::new(values))))
}

fn columnar_to_rows(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("columnar.to_rows requires table id".to_string()).at(span));
    }

    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.to_rows: expected table id (int)".to_string()).at(span)),
    };

    let limit = if args.len() > 1 {
        match &args[1] {
            Value::Int(i) if *i > 0 => Some(*i as usize),
            _ => None,
        }
    } else {
        None
    };

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.to_rows: invalid table id {}", id)).at(span)
    })?;

    let tbl = tbl_arc.lock().unwrap();
    Ok(tbl.to_shae_rows(limit))
}

fn columnar_count(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.count", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.count: expected table id (int)".to_string()).at(span)),
    };

    let registry = get_tbl_registry().read().unwrap();
    let tbl_arc = registry.get(&id).ok_or_else(|| {
        RuntimeError::new(format!("columnar.count: invalid table id {}", id)).at(span)
    })?;

    let tbl = tbl_arc.lock().unwrap();
    Ok(Value::Int(tbl.row_count as i64))
}

fn columnar_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("columnar.close", 1, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("columnar.close: expected table id (int)".to_string()).at(span)),
    };

    let mut registry = get_tbl_registry().write().unwrap();
    let removed = registry.remove(&id).is_some();
    Ok(Value::Bool(removed))
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_columnar_insert_and_filter() {
        let mut schema = IndexMap::new();
        schema.insert("id".to_string(), ColumnType::Int);
        schema.insert("region".to_string(), ColumnType::Str);
        schema.insert("amount".to_string(), ColumnType::Float);
        schema.insert("active".to_string(), ColumnType::Bool);

        let mut tbl = ColumnarTable::new(schema);

        let mut r1 = IndexMap::new();
        r1.insert("id".to_string(), Value::Int(1));
        r1.insert("region".to_string(), Value::String("US".to_string()));
        r1.insert("amount".to_string(), Value::Float(100.0));
        r1.insert("active".to_string(), Value::Bool(true));
        tbl.insert_row(&r1);

        let mut r2 = IndexMap::new();
        r2.insert("id".to_string(), Value::Int(2));
        r2.insert("region".to_string(), Value::String("EU".to_string()));
        r2.insert("amount".to_string(), Value::Float(250.0));
        r2.insert("active".to_string(), Value::Bool(true));
        tbl.insert_row(&r2);

        let mut r3 = IndexMap::new();
        r3.insert("id".to_string(), Value::Int(3));
        r3.insert("region".to_string(), Value::String("US".to_string()));
        r3.insert("amount".to_string(), Value::Float(50.0));
        r3.insert("active".to_string(), Value::Bool(false));
        tbl.insert_row(&r3);

        assert_eq!(tbl.row_count, 3);

        // Filter: active == true
        let active_tbl = tbl.filter("active", FilterOp::Eq, &Value::Bool(true)).unwrap();
        assert_eq!(active_tbl.row_count, 2);

        // Filter: amount > 80.0
        let high_val = tbl.filter("amount", FilterOp::Gt, &Value::Float(80.0)).unwrap();
        assert_eq!(high_val.row_count, 2);
    }

    #[test]
    fn test_columnar_aggregations() {
        let mut schema = IndexMap::new();
        schema.insert("score".to_string(), ColumnType::Float);
        schema.insert("views".to_string(), ColumnType::Int);

        let mut tbl = ColumnarTable::new(schema);
        for i in 1..=10 {
            let mut r = IndexMap::new();
            r.insert("score".to_string(), Value::Float(i as f64 * 10.0));
            r.insert("views".to_string(), Value::Int(i * 100));
            tbl.insert_row(&r);
        }

        let mut agg_specs = IndexMap::new();
        agg_specs.insert("total_views".to_string(), ("views".to_string(), AggOp::Sum));
        agg_specs.insert("avg_score".to_string(), ("score".to_string(), AggOp::Avg));
        agg_specs.insert("min_score".to_string(), ("score".to_string(), AggOp::Min));
        agg_specs.insert("max_views".to_string(), ("views".to_string(), AggOp::Max));
        agg_specs.insert("count".to_string(), ("score".to_string(), AggOp::Count));

        let res = tbl.aggregate(&agg_specs).unwrap();
        assert_eq!(res.get("total_views"), Some(&Value::Int(5500)));
        assert_eq!(res.get("avg_score"), Some(&Value::Float(55.0)));
        assert_eq!(res.get("min_score"), Some(&Value::Float(10.0)));
        assert_eq!(res.get("max_views"), Some(&Value::Int(1000)));
        assert_eq!(res.get("count"), Some(&Value::Int(10)));
    }

    #[test]
    fn test_columnar_group_by() {
        let mut schema = IndexMap::new();
        schema.insert("dept".to_string(), ColumnType::Str);
        schema.insert("salary".to_string(), ColumnType::Float);

        let mut tbl = ColumnarTable::new(schema);

        let mut r1 = IndexMap::new();
        r1.insert("dept".to_string(), Value::String("Eng".to_string()));
        r1.insert("salary".to_string(), Value::Float(120.0));
        tbl.insert_row(&r1);

        let mut r2 = IndexMap::new();
        r2.insert("dept".to_string(), Value::String("Sales".to_string()));
        r2.insert("salary".to_string(), Value::Float(90.0));
        tbl.insert_row(&r2);

        let mut r3 = IndexMap::new();
        r3.insert("dept".to_string(), Value::String("Eng".to_string()));
        r3.insert("salary".to_string(), Value::Float(140.0));
        tbl.insert_row(&r3);

        let mut agg_specs = IndexMap::new();
        agg_specs.insert("avg_salary".to_string(), ("salary".to_string(), AggOp::Avg));
        agg_specs.insert("total_salary".to_string(), ("salary".to_string(), AggOp::Sum));

        let groups = tbl.group_by("dept", &agg_specs).unwrap();
        assert_eq!(groups.len(), 2);

        let eng_group = groups.iter().find(|g| g.get("dept") == Some(&Value::String("Eng".to_string()))).unwrap();
        assert_eq!(eng_group.get("avg_salary"), Some(&Value::Float(130.0)));
        assert_eq!(eng_group.get("total_salary"), Some(&Value::Float(260.0)));

        let sales_group = groups.iter().find(|g| g.get("dept") == Some(&Value::String("Sales".to_string()))).unwrap();
        assert_eq!(sales_group.get("avg_salary"), Some(&Value::Float(90.0)));
        assert_eq!(sales_group.get("total_salary"), Some(&Value::Float(90.0)));
    }
}
