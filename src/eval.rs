use crate::ast::{BinaryOp, Expr, InterpPart, Literal, Program, Span, Stmt, StmtKind, UnaryOp};
use crate::env::Environment;
use crate::value::{IndexError, Value, resolve_index, resolve_int_index};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::Arc;
use thiserror::Error;

fn to_i64_val(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) | Value::Number(f) if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 => Some(*f as i64),
        _ => None,
    }
}

fn to_f64_val(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) | Value::Number(f) => Some(*f),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub struct RuntimeError {
    pub message: String,
    pub hint: Option<String>,
    pub span: Option<Span>,
    pub stack: Vec<(String, crate::ast::Span)>,
}

impl RuntimeError {
    pub fn new(message: String) -> Self {
        Self {
            message,
            hint: None,
            span: None,
            stack: Vec::new(),
        }
    }

    pub fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_string());
        self
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn or_at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(h) = &self.hint {
            write!(f, "\n  💡 Hint: {}", h)?;
        }
        Ok(())
    }
}

pub enum Signal {
    None,
    Value(Value),
    Return(Value),
    Break,
    Continue,
}

pub struct Evaluator {
    pub global_env: Arc<RwLock<Environment>>,
    max_depth: usize,
    depth: usize,
    pub current_file: Option<PathBuf>,
    pub module_cache: Arc<RwLock<HashMap<String, Value>>>,
}

impl Evaluator {
    pub fn new() -> Self {
        let mut env = Environment::new();
        crate::builtins::register(&mut env);
        Self {
            global_env: Arc::new(RwLock::new(env)),
            max_depth: 2000,
            depth: 0,
            current_file: None,
            module_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set_max_depth(&mut self, limit: usize) {
        self.max_depth = limit;
    }

    pub fn with_env(env: Arc<RwLock<Environment>>) -> Self {
        Self {
            global_env: env,
            max_depth: 2000,
            depth: 0,
            current_file: None,
            module_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_env_cache_file(
        env: Arc<RwLock<Environment>>,
        module_cache: Arc<RwLock<HashMap<String, Value>>>,
        current_file: Option<PathBuf>,
    ) -> Self {
        Self {
            global_env: env,
            max_depth: 2000,
            depth: 0,
            current_file,
            module_cache,
        }
    }

    pub fn load_module(&mut self, path_str: &str, span: Span) -> Result<Value, RuntimeError> {
        // Standard library module namespaces (std:fs, std:path, std:sys, std:time)
        if path_str.starts_with("std:") || path_str.starts_with("std::") {
            if let Some(cached) = self.module_cache.read().unwrap().get(path_str) {
                return Ok(cached.clone());
            }
            let mod_val = crate::stdlib::load_std_module(path_str, span)?;
            self.module_cache
                .write()
                .unwrap()
                .insert(path_str.to_string(), mod_val.clone());
            return Ok(mod_val);
        }

        // File-based module: resolve relative to current_file
        let resolved_path = if path_str.starts_with("./") || path_str.starts_with("../") {
            if let Some(cur) = &self.current_file {
                if let Some(parent) = cur.parent() {
                    parent.join(path_str)
                } else {
                    PathBuf::from(path_str)
                }
            } else {
                PathBuf::from(path_str)
            }
        } else if let Some(cur) = &self.current_file {
            if let Some(parent) = cur.parent() {
                let candidate = parent.join(path_str);
                if candidate.exists() {
                    candidate
                } else {
                    PathBuf::from(path_str)
                }
            } else {
                PathBuf::from(path_str)
            }
        } else {
            PathBuf::from(path_str)
        };

        let canonical_key = match std::fs::canonicalize(&resolved_path) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => resolved_path.to_string_lossy().to_string(),
        };

        // Cache hit
        if let Some(cached) = self.module_cache.read().unwrap().get(&canonical_key) {
            return Ok(cached.clone());
        }

        // Guard against circular import loops by inserting an empty map into cache early
        let export_map = Arc::new(RwLock::new(indexmap::IndexMap::new()));
        let module_val = Value::Map(export_map.clone());
        self.module_cache
            .write()
            .unwrap()
            .insert(canonical_key.clone(), module_val.clone());

        let source = std::fs::read_to_string(&resolved_path).map_err(|e| {
            RuntimeError::new(format!("Failed to read module '{}': {}", path_str, e)).at(span)
        })?;

        let mut builtin_env = Environment::new();
        crate::builtins::register(&mut builtin_env);
        let builtin_env_rc = Arc::new(RwLock::new(builtin_env));
        let module_env = Arc::new(RwLock::new(Environment::new_with_parent(builtin_env_rc)));

        let child_file = match std::fs::canonicalize(&resolved_path) {
            Ok(p) => Some(p),
            Err(_) => Some(resolved_path),
        };

        let mut ev = Evaluator::with_env_cache_file(
            module_env.clone(),
            self.module_cache.clone(),
            child_file,
        );

        let tokens = crate::lexer::tokenize(&source).map_err(|e| {
            RuntimeError::new(format!("Lexer error in module '{}': {}", path_str, e)).at(span)
        })?;
        let program = crate::parser::parse(tokens).map_err(|e| {
            RuntimeError::new(format!("Parser error in module '{}': {}", path_str, e)).at(span)
        })?;
        ev.eval_program(&program).map_err(|e| {
            RuntimeError::new(format!("Runtime error in module '{}': {}", path_str, e.message)).at(span)
        })?;

        let evaluated_map = module_env.read().unwrap().export_map();
        *export_map.write().unwrap() = evaluated_map;

        Ok(module_val)
    }

    pub fn eval_program(&mut self, program: &Program) -> Result<Value, RuntimeError> {
        let mut last_val = Value::Null;
        let env = self.global_env.clone();
        for stmt in &program.statements {
            let sig = self.eval_stmt(stmt, &env)?;
            match sig {
                Signal::Value(v) => last_val = v,
                Signal::Return(v) => return Ok(v),
                Signal::Break => {
                    return Err(
                        RuntimeError::new("Cannot break outside of a loop".into()).at(stmt.span)
                    );
                }
                Signal::Continue => {
                    return Err(
                        RuntimeError::new("Cannot continue outside of a loop".into()).at(stmt.span),
                    );
                }
                Signal::None => last_val = Value::Null,
            }
        }
        Ok(last_val)
    }

    fn eval_stmt(
        &mut self,
        stmt: &Stmt,
        env: &Arc<RwLock<Environment>>,
    ) -> Result<Signal, RuntimeError> {
        match &stmt.kind {
StmtKind::StructDef { name, fields } => {
                env.write().unwrap().define(name.clone(), Value::StructDef {
                    name: name.clone(),
                    fields: fields.clone(),
                });
                Ok(Signal::None)
            }
            StmtKind::EnumDef { name, variants } => {
                let mut var_map = indexmap::IndexMap::new();
                for v in variants {
                    var_map.insert(v.name.clone(), v.fields.clone());
                }
                env.write().unwrap().define(name.clone(), Value::EnumDef {
                    name: name.clone(),
                    variants: std::sync::Arc::new(var_map),
                });
                Ok(Signal::None)
            }
            StmtKind::Expr(expr) => {
                let val = self.eval_expr(expr, env).map_err(|e| e.or_at(stmt.span))?;
                Ok(Signal::Value(val))
            }
            StmtKind::Let { pattern, init } => {
                let val = self.eval_expr(init, env).map_err(|e| e.or_at(stmt.span))?;
                self.bind_pattern_value(pattern, val, env, stmt.span)?;
                Ok(Signal::None)
            }
            StmtKind::Assign { target, value } => {
                let val = self.eval_expr(value, env).map_err(|e| e.or_at(stmt.span))?;
                self.eval_assign(target, val, env)
                    .map_err(|e| e.or_at(stmt.span))?;
                Ok(Signal::None)
            }
            StmtKind::FnDef { name, params, body } => {
                let func = Value::Function {
                    name: Some(name.clone()),
                    params: params.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                };
                env.write().unwrap().define(name.clone(), func);
                Ok(Signal::None)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self
                    .eval_expr(condition, env)
                    .map_err(|e| e.or_at(stmt.span))?;
                if cond_val.is_truthy() {
                    self.eval_block(then_branch, env)
                } else if let Some(else_b) = else_branch {
                    self.eval_block(else_b, env)
                } else {
                    Ok(Signal::None)
                }
            }
            
            StmtKind::TryCatch { try_body, catch_ident, catch_body } => {
                let try_env = std::sync::Arc::new(std::sync::RwLock::new(crate::env::Environment::new_with_parent(env.clone())));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = std::sync::Arc::new(std::sync::RwLock::new(crate::env::Environment::new_with_parent(env.clone())));
                        catch_env.write().unwrap().define(catch_ident.clone(), Value::String(e.message));
                        self.eval_block(catch_body, &catch_env)
                    }
                }
            }
            StmtKind::While { condition, body } => {
                loop {
                    let cond_val = self
                        .eval_expr(condition, env)
                        .map_err(|e| e.or_at(stmt.span))?;
                    if !cond_val.is_truthy() {
                        break;
                    }
                    match self.eval_block(body, env)? {
                        Signal::Value(_) | Signal::None => {}
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                    }
                }
                Ok(Signal::None)
            }
            StmtKind::For {
                item,
                iterable,
                body,
            } => {
                let iter_val = self
                    .eval_expr(iterable, env)
                    .map_err(|e| e.or_at(stmt.span))?;
                let elements = match iter_val {
                    Value::Array(a) => a.read().unwrap().clone(),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "Cannot iterate over {}",
                            other.type_name()
                        ))
                        .at(stmt.span));
                    }
                };
                for elem in elements {
                    let loop_env = Arc::new(RwLock::new(Environment::new_with_parent(env.clone())));
                    loop_env.write().unwrap().define(item.clone(), elem);
                    match self.eval_block(body, &loop_env)? {
                        Signal::Value(_) | Signal::None => {}
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                    }
                }
                Ok(Signal::None)
            }
            StmtKind::Return(Some(expr)) => {
                let val = self.eval_expr(expr, env).map_err(|e| e.or_at(stmt.span))?;
                Ok(Signal::Return(val))
            }
            StmtKind::Return(None) => Ok(Signal::Return(Value::Null)),
            StmtKind::Break => Ok(Signal::Break),
            StmtKind::Continue => Ok(Signal::Continue),
            StmtKind::Use { imports, path } => {
                let mod_val = self.load_module(path, stmt.span)?;
                match mod_val {
                    Value::Map(m) => {
                        let map = m.read().unwrap();
                        for item in imports {
                            if item.name == "*" {
                                for (k, v) in map.iter() {
                                    env.write().unwrap().define(k.clone(), v.clone());
                                }
                            } else {
                                let val = map.get(&item.name).cloned().ok_or_else(|| {
                                    RuntimeError::new(format!(
                                        "Module '{}' does not export '{}'",
                                        path, item.name
                                    ))
                                    .at(stmt.span)
                                })?;
                                let bind_name = item.alias.as_ref().unwrap_or(&item.name);
                                env.write().unwrap().define(bind_name.clone(), val);
                            }
                        }
                    }
                    _ => {
                        return Err(RuntimeError::new(format!(
                            "Expected module exports from '{}', found non-map value",
                            path
                        ))
                        .at(stmt.span));
                    }
                }
                Ok(Signal::None)
            }
        }
    }

    fn eval_block(
        &mut self,
        block: &[Stmt],
        env: &Arc<RwLock<Environment>>,
    ) -> Result<Signal, RuntimeError> {
        let mut last_val = Value::Null;
        let mut produced_value = false;
        for stmt in block {
            let sig = self.eval_stmt(stmt, env)?;
            match sig {
                Signal::Value(v) => {
                    last_val = v;
                    produced_value = true;
                }
                Signal::Return(_) | Signal::Break | Signal::Continue => return Ok(sig),
                Signal::None => {
                    last_val = Value::Null;
                    produced_value = false;
                }
            }
        }
        if produced_value {
            Ok(Signal::Value(last_val))
        } else {
            Ok(Signal::None)
        }
    }

    fn eval_assign(
        &mut self,
        target: &Expr,
        value: Value,
        env: &Arc<RwLock<Environment>>,
    ) -> Result<(), RuntimeError> {
        match target {
            Expr::Variable { name, span } => {
                if !env.write().unwrap().set(name, value.clone()) {
                    let candidates = env.read().unwrap().all_names();
                    let hint =
                        if name == "NULL" || name == "undefined" || name == "nil" || name == "None"
                        {
                            "Use 'null' for null values in Shae.".to_string()
                        } else if let Some(closest) = crate::suggest::closest(name, &candidates) {
                            format!("Did you mean '{}'?", closest)
                        } else {
                            "Declare it first with 'let {} = ...'".to_string()
                        };
                    return Err(RuntimeError::new(format!("Undefined variable '{}'", name))
                        .with_hint(&hint)
                        .at(*span));
                }
                Ok(())
            }
            Expr::Get {
                target: base_expr,
                property,
                span,
                ..
            } => {
                let base_val = self.eval_expr(base_expr, env)?;
                if let Value::Map(m) = base_val {
                    m.write().unwrap().insert(property.clone(), value);
                    Ok(())
                } else if let Value::StructInstance { name, fields } = base_val {
                    if fields.read().unwrap().contains_key(property) {
                        fields.write().unwrap().insert(property.clone(), value);
                        Ok(())
                    } else {
                        Err(RuntimeError::new(format!("Struct '{}' has no field '{}'", name, property)).at(*span))
                    }
                } else {
                    Err(RuntimeError::new(format!(
                        "Cannot set property '{}' on {}",
                        property,
                        base_val.type_name()
                    ))
                    .at(*span))
                }
            }
            Expr::Index {
                target: base_expr,
                index: idx_expr,
                span,
            } => {
                let base_val = self.eval_expr(base_expr, env)?;
                let idx_val = self.eval_expr(idx_expr, env)?;
                match base_val {
                    Value::Array(a) => {
                        let mut arr = a.write().unwrap();
                        let len = arr.len();
                        let res = match idx_val {
                            Value::Int(i) => resolve_int_index(i, len),
                            Value::Float(n) | Value::Number(n) => resolve_index(n, len),
                            _ => return Err(RuntimeError::new("Array index must be a number".into()).at(*span)),
                        };
                        match res {
                            Ok(i) => {
                                arr[i] = value;
                                Ok(())
                            }
                            Err(IndexError::NotWhole) => Err(RuntimeError::new(
                                "Array index must be a whole number".into(),
                            )
                            .at(*span)),
                            Err(IndexError::OutOfRange(_)) => Err(RuntimeError::new(format!(
                                "Index {} out of bounds for array of length {}",
                                idx_val.to_display(), len
                            ))
                            .at(*span)),
                        }
                    }
                    Value::Map(m) => {
                        if let Value::String(s) = idx_val {
                            m.write().unwrap().insert(s, value);
                            Ok(())
                        } else {
                            Err(RuntimeError::new("Map key must be a string".into()).at(*span))
                        }
                    }
                    other => Err(RuntimeError::new(format!(
                        "Cannot index into {}",
                        other.type_name()
                    ))
                    .at(*span)),
                }
            }
            _ => Err(RuntimeError::new("Invalid assignment target".into())),
        }
    }

    // Fall back to eval_expr implementation after adding eval_chain below.

    fn eval_expr(
        &mut self,
        expr: &Expr,
        env: &Arc<RwLock<Environment>>,
    ) -> Result<Value, RuntimeError> {
        let _span = expr.span();
        match expr {
            Expr::Literal(Literal::Null) => Ok(Value::Null),
            Expr::Literal(Literal::Bool(b)) => Ok(Value::Bool(*b)),
            Expr::Literal(Literal::Int(n)) => Ok(Value::Int(*n)),
            Expr::Literal(Literal::Float(n)) => Ok(Value::Float(*n)),
            Expr::Literal(Literal::Number(n)) => Ok(Value::Float(*n)),
            Expr::Literal(Literal::String(s)) => Ok(Value::String(s.clone())),
            Expr::Variable { name, span } => {
                if let Some(v) = env.read().unwrap().get(name) {
                    Ok(v)
                } else {
                    let candidates = env.read().unwrap().all_names();
                    let hint =
                        if name == "NULL" || name == "undefined" || name == "nil" || name == "None"
                        {
                            "Use 'null' for null values in Shae.".to_string()
                        } else if let Some(closest) = crate::suggest::closest(name, &candidates) {
                            format!("Did you mean '{}'?", closest)
                        } else {
                            "Declare it first with 'let {} = ...'".to_string()
                        };
                    Err(RuntimeError::new(format!("Undefined variable '{}'", name))
                        .with_hint(&hint)
                        .at(*span))
                }
            }
            Expr::Array(elements) => {
                let mut vals = Vec::new();
                for e in elements {
                    vals.push(self.eval_expr(e, env)?);
                }
                Ok(Value::Array(Arc::new(RwLock::new(vals))))
            }
            Expr::Map(elements) => {
                let mut m = indexmap::IndexMap::new();
                for (k, v_expr) in elements {
                    let val = self.eval_expr(v_expr, env)?;
                    m.insert(k.clone(), val);
                }
                Ok(Value::Map(Arc::new(RwLock::new(m))))
            }
Expr::StructInit { name, fields, span } => {
                let def = env.read().unwrap().get(name);
                if let Some(Value::StructDef { fields: def_fields, .. }) = def {
                    if fields.len() != def_fields.len() {
                        return Err(RuntimeError::new(format!("Struct '{}' expects {} fields, but got {}.", name, def_fields.len(), fields.len())).at(*span));
                    }
                    
                    let mut instance_fields = indexmap::IndexMap::new();
                    let mut provided = std::collections::HashSet::new();
                    for (f_name, f_expr) in fields {
                        if !def_fields.contains(f_name) {
                            return Err(RuntimeError::new(format!("Struct '{}' has no field '{}'.", name, f_name)).at(*span));
                        }
                        provided.insert(f_name.clone());
                        let val = self.eval_expr(f_expr, env)?;
                        instance_fields.insert(f_name.clone(), val);
                    }
                    if provided.len() != def_fields.len() {
                        return Err(RuntimeError::new(format!("Struct '{}' initialization is missing fields.", name)).at(*span));
                    }
                    
                    Ok(Value::StructInstance {
                        name: name.clone(),
                        fields: std::sync::Arc::new(std::sync::RwLock::new(instance_fields)),
                    })
                } else {
                    Err(RuntimeError::new(format!("'{}' is not a struct.", name)).at(*span))
                }
            }
            Expr::Match { target, arms, span } => {
                let target_val = self.eval_expr(target, env)?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &target_val) {
                        let match_env = std::sync::Arc::new(std::sync::RwLock::new(crate::env::Environment::new_with_parent(env.clone())));
                        for (k, v) in bindings {
                            match_env.write().unwrap().define(k, v);
                        }
                        if let Some(guard) = &arm.guard {
                            let guard_val = self.eval_expr(guard, &match_env)?;
                            if !guard_val.is_truthy() {
                                continue;
                            }
                        }
                        return self.eval_expr(&arm.body, &match_env);
                    }
                }
                Err(RuntimeError::new("Non-exhaustive match. No pattern matched the value.".into()).at(*span))
            }
            Expr::Use { path, span } => {
                let path_val = self.eval_expr(path, env)?;
                let path_str = match path_val {
                    Value::String(s) => s,
                    _ => {
                        return Err(
                            RuntimeError::new("Module path must be a string".into()).at(*span)
                        )
                    }
                };
                self.load_module(&path_str, *span)
            }
            Expr::Interpolated(parts) => {
                let mut res = String::new();
                for part in parts {
                    match part {
                        InterpPart::Text(t) => res.push_str(t),
                        InterpPart::Expr(e) => {
                            let val = self.eval_expr(e, env)?;
                            res.push_str(&val.to_display());
                        }
                    }
                }
                Ok(Value::String(res))
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                if *op == BinaryOp::And {
                    let left_val = self.eval_expr(left, env)?;
                    if !left_val.is_truthy() {
                        return Ok(Value::Bool(false));
                    }
                    let right_val = self.eval_expr(right, env)?;
                    return Ok(Value::Bool(right_val.is_truthy()));
                }
                if *op == BinaryOp::Or {
                    let left_val = self.eval_expr(left, env)?;
                    if left_val.is_truthy() {
                        return Ok(Value::Bool(true));
                    }
                    let right_val = self.eval_expr(right, env)?;
                    return Ok(Value::Bool(right_val.is_truthy()));
                }
                if *op == BinaryOp::Coalesce {
                    let left_val = self.eval_chain(left, env, true)?;
                    if let Some(v) = left_val {
                        if !matches!(v, Value::Null) {
                            return Ok(v);
                        }
                    }
                    return self.eval_expr(right, env);
                }
                
                if *op == BinaryOp::Pipe {
                    let left_val = self.eval_expr(left, env)?;
                    match &**right {
                        Expr::Call { callee, args, span: call_span } => {
                            let callee_val = self.eval_expr(callee, env)?;
                            let mut evaled_args = vec![left_val];
                            for arg in args {
                                evaled_args.push(self.eval_expr(arg, env)?);
                            }
                            return self.call_value(&callee_val, evaled_args, *call_span);
                        }
                        _ => {
                            let func_val = self.eval_expr(right, env)?;
                            return self.call_value(&func_val, vec![left_val], *span);
                        }
                    }
                }

                let left_val = self.eval_expr(left, env)?;
                let right_val = self.eval_expr(right, env)?;

                match op {
                    BinaryOp::Add => {
                        match (&left_val, &right_val) {
                            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
                            (Value::Int(a), Value::Float(b)) | (Value::Int(a), Value::Number(b)) => Ok(Value::Float(*a as f64 + b)),
                            (Value::Float(a), Value::Int(b)) | (Value::Number(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
                            (Value::Float(a), Value::Float(b))
                            | (Value::Number(a), Value::Number(b))
                            | (Value::Float(a), Value::Number(b))
                            | (Value::Number(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
                            (Value::Array(a), Value::Array(b)) => {
                                let mut new_arr = a.read().unwrap().clone();
                                new_arr.extend(b.read().unwrap().clone());
                                Ok(Value::Array(Arc::new(RwLock::new(new_arr))))
                            }
                            _ => Err(RuntimeError::new(format!(
                                "Cannot add {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).with_hint("For string concatenation, use string interpolation: `\"hi {name}\"` or convert explicitly: `str(x) + str(y)`").at(*span))
                        }
                    }
                    BinaryOp::Sub => {
                        match (&left_val, &right_val) {
                            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
                            (Value::Int(a), Value::Float(b)) | (Value::Int(a), Value::Number(b)) => Ok(Value::Float(*a as f64 - b)),
                            (Value::Float(a), Value::Int(b)) | (Value::Number(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
                            (Value::Float(a), Value::Float(b))
                            | (Value::Number(a), Value::Number(b))
                            | (Value::Float(a), Value::Number(b))
                            | (Value::Number(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                            _ => Err(RuntimeError::new(format!(
                                "Arithmetic operation requires numbers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Mul => {
                        match (&left_val, &right_val) {
                            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
                            (Value::Int(a), Value::Float(b)) | (Value::Int(a), Value::Number(b)) => Ok(Value::Float(*a as f64 * b)),
                            (Value::Float(a), Value::Int(b)) | (Value::Number(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
                            (Value::Float(a), Value::Float(b))
                            | (Value::Number(a), Value::Number(b))
                            | (Value::Float(a), Value::Number(b))
                            | (Value::Number(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                            _ => Err(RuntimeError::new(format!(
                                "Arithmetic operation requires numbers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Div => {
                        if let (Value::Int(a), Value::Int(b)) = (&left_val, &right_val) {
                            if *b == 0 {
                                Err(RuntimeError::new("Dividing by zero creates black holes".into()).at(*span))
                            } else if a % b == 0 {
                                Ok(Value::Int(a / b))
                            } else {
                                Ok(Value::Float(*a as f64 / *b as f64))
                            }
                        } else if let (Some(a), Some(b)) = (to_f64_val(&left_val), to_f64_val(&right_val)) {
                            if b == 0.0 {
                                Err(RuntimeError::new("Dividing by zero creates black holes".into()).at(*span))
                            } else {
                                Ok(Value::Float(a / b))
                            }
                        } else {
                            Err(RuntimeError::new(format!(
                                "Arithmetic operation requires numbers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Mod => {
                        if let (Value::Int(a), Value::Int(b)) = (&left_val, &right_val) {
                            if *b == 0 {
                                Err(RuntimeError::new("Modulo by zero".into()).at(*span))
                            } else {
                                Ok(Value::Int(a % b))
                            }
                        } else if let (Some(a), Some(b)) = (to_f64_val(&left_val), to_f64_val(&right_val)) {
                            if b == 0.0 {
                                Err(RuntimeError::new("Modulo by zero".into()).at(*span))
                            } else {
                                Ok(Value::Float(a % b))
                            }
                        } else {
                            Err(RuntimeError::new(format!(
                                "Arithmetic operation requires numbers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::BitAnd => {
                        if let (Some(a), Some(b)) = (to_i64_val(&left_val), to_i64_val(&right_val)) {
                            Ok(Value::Int(a & b))
                        } else {
                            Err(RuntimeError::new(format!(
                                "Bitwise AND requires integers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::BitOr => {
                        if let (Some(a), Some(b)) = (to_i64_val(&left_val), to_i64_val(&right_val)) {
                            Ok(Value::Int(a | b))
                        } else {
                            Err(RuntimeError::new(format!(
                                "Bitwise OR requires integers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::BitXor => {
                        if let (Some(a), Some(b)) = (to_i64_val(&left_val), to_i64_val(&right_val)) {
                            Ok(Value::Int(a ^ b))
                        } else {
                            Err(RuntimeError::new(format!(
                                "Bitwise XOR requires integers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Shl => {
                        if let (Some(a), Some(b)) = (to_i64_val(&left_val), to_i64_val(&right_val)) {
                            if b < 0 {
                                Err(RuntimeError::new("Negative shift count".into()).at(*span))
                            } else if b >= 64 {
                                Ok(Value::Int(0))
                            } else {
                                Ok(Value::Int(a.wrapping_shl(b as u32)))
                            }
                        } else {
                            Err(RuntimeError::new(format!(
                                "Bitwise shift requires integers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Shr => {
                        if let (Some(a), Some(b)) = (to_i64_val(&left_val), to_i64_val(&right_val)) {
                            if b < 0 {
                                Err(RuntimeError::new("Negative shift count".into()).at(*span))
                            } else if b >= 64 {
                                if a < 0 {
                                    Ok(Value::Int(-1))
                                } else {
                                    Ok(Value::Int(0))
                                }
                            } else {
                                Ok(Value::Int(a.wrapping_shr(b as u32)))
                            }
                        } else {
                            Err(RuntimeError::new(format!(
                                "Bitwise shift requires integers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Eq => Ok(Value::Bool(left_val == right_val)),
                    BinaryOp::NotEq => Ok(Value::Bool(left_val != right_val)),
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        if let (Value::String(a), Value::String(b)) = (&left_val, &right_val) {
                            let res = match op {
                                BinaryOp::Lt => a < b,
                                BinaryOp::LtEq => a <= b,
                                BinaryOp::Gt => a > b,
                                BinaryOp::GtEq => a >= b,
                                _ => unreachable!(),
                            };
                            Ok(Value::Bool(res))
                        } else if let (Value::Int(a), Value::Int(b)) = (&left_val, &right_val) {
                            let res = match op {
                                BinaryOp::Lt => a < b,
                                BinaryOp::LtEq => a <= b,
                                BinaryOp::Gt => a > b,
                                BinaryOp::GtEq => a >= b,
                                _ => unreachable!(),
                            };
                            Ok(Value::Bool(res))
                        } else if let (Some(a), Some(b)) = (to_f64_val(&left_val), to_f64_val(&right_val)) {
                            let res = match op {
                                BinaryOp::Lt => a < b,
                                BinaryOp::LtEq => a <= b,
                                BinaryOp::Gt => a > b,
                                BinaryOp::GtEq => a >= b,
                                _ => unreachable!(),
                            };
                            Ok(Value::Bool(res))
                        } else {
                            Err(RuntimeError::new(format!(
                                "Cannot compare {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Expr::Unary {
                op,
                expr: inner_expr,
                span,
            } => {
                let inner = self.eval_expr(inner_expr, env)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!inner.is_truthy())),
                    UnaryOp::Neg => {
                        match inner {
                            Value::Int(n) => Ok(Value::Int(-n)),
                            Value::Float(n) | Value::Number(n) => Ok(Value::Float(-n)),
                            _ => Err(RuntimeError::new(format!("Cannot negate {}", inner.type_name())).at(*span)),
                        }
                    }
                    UnaryOp::BitNot => {
                        if let Some(n) = to_i64_val(&inner) {
                            Ok(Value::Int(!n))
                        } else {
                            Err(RuntimeError::new(format!("Cannot bitwise NOT {}", inner.type_name())).at(*span))
                        }
                    }
                }
            }
            Expr::Lambda {
                params,
                body,
                span: _,
            } => Ok(Value::Function {
                name: None,
                params: params.clone(),
                body: body.clone(),
                closure: env.clone(),
            }),
            _ => {
                // For Call, Get, Index, use eval_chain
                if let Some(val) = self.eval_chain(expr, env, false)? {
                    Ok(val)
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    fn eval_chain(
        &mut self,
        expr: &Expr,
        env: &Arc<RwLock<Environment>>,
        lenient: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        match expr {
            Expr::Get {
                target,
                property,
                safe,
                span,
            } => {
                let base = self.eval_chain(target, env, lenient)?;
                let base = match base {
                    Some(b) => b,
                    None => {
                        if *safe || lenient {
                            return Ok(None);
                        }
                        return Err(
                            RuntimeError::new("Cannot read property of null".into()).at(*span)
                        );
                    }
                };
                if matches!(base, Value::Null) {
                    if *safe || lenient {
                        return Ok(None);
                    }
                    return Err(RuntimeError::new("Cannot read property of null".into()).at(*span));
                }
                match base {

                    Value::StructInstance { name, fields } => {
                        if let Some(val) = fields.read().unwrap().get(property) {
                            Ok(Some(val.clone()))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!("Struct '{}' has no field '{}'", name, property)).at(*span))
                        }
                    }
                    Value::EnumDef { name, variants } => {
                        if let Some(params) = variants.get(property) {
                            Ok(Some(Value::EnumConstructor {
                                enum_name: name.clone(),
                                variant_name: property.clone(),
                                params: params.clone(),
                            }))
                        } else {
                            Err(RuntimeError::new(format!("Enum '{}' has no variant '{}'", name, property)).at(*span))
                        }
                    }
                    Value::Map(m) => {
                        let map = m.read().unwrap();
                        if let Some(v) = map.get(property) {
                            Ok(Some(v.clone()))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                let keys: Vec<String> = map.keys().cloned().collect();
                                let hint = if let Some(closest) =
                                    crate::suggest::closest(property, &keys)
                                {
                                    format!(
                                        "Did you mean '{}'? Or use '?.' to return null.",
                                        closest
                                    )
                                } else {
                                    "Use '?.' to return null if the property might not exist."
                                        .to_string()
                                };
                                Err(RuntimeError::new(format!(
                                    "Property '{}' not found in map",
                                    property
                                ))
                                .with_hint(&hint)
                                .at(*span))
                            }
                        }
                    }
                    Value::Array(a) => {
                        if property == "len" {
                            Ok(Some(Value::Int(a.read().unwrap().len() as i64)))
                        } else if property == "first" {
                            let val = a.read().unwrap().first().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'first' of an empty array".into()).at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "last" {
                            let val = a.read().unwrap().last().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'last' of an empty array".into()).at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "push" || property == "pop" || property == "map" || property == "filter" || property == "reduce" || property == "sum" || property == "sort"
                            || property == "find" || property == "some" || property == "every" || property == "flat" || property == "join" || property == "reverse" || property == "slice" {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Array(a.clone())),
                                method: property.clone()
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!("Array has no property '{}'", property)).at(*span))
                            }
                        }
                    }
                    Value::String(s) => {
                        if property == "len" {
                            Ok(Some(Value::Int(s.chars().count() as i64)))
                        } else if property == "trim" || property == "upper" || property == "lower" || property == "split" || property == "replace"
                            || property == "starts_with" || property == "ends_with" || property == "contains" || property == "pad_start" || property == "lines" || property == "chars" || property == "slice" {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::String(s.clone())),
                                method: property.clone()
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!("String has no property '{}'", property)).at(*span))
                            }
                        }
                    }
                    Value::Task(t) => {
                        if property == "join" || property == "wait" {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Task(t.clone())),
                                method: property.clone(),
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!("Task has no property '{}'", property)).at(*span))
                            }
                        }
                    }
                    Value::Channel(ch) => {
                        if property == "send"
                            || property == "recv"
                            || property == "try_recv"
                            || property == "tryRecv"
                            || property == "close"
                            || property == "len"
                            || property == "capacity"
                        {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Channel(ch.clone())),
                                method: property.clone(),
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!("Channel has no property '{}'", property)).at(*span))
                            }
                        }
                    }
                    other => {
                        if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "Cannot read property '{}' on {}",
                                property,
                                other.type_name()
                            ))
                            .at(*span))
                        }
                    }
                }
            }
            Expr::Index {
                target,
                index,
                span,
            } => {
                let base = self.eval_chain(target, env, lenient)?;
                let base = match base {
                    Some(b) => b,
                    None => {
                        if lenient {
                            return Ok(None);
                        }
                        return Err(RuntimeError::new("Cannot index into null".into()).at(*span));
                    }
                };
                if matches!(base, Value::Null) {
                    if lenient {
                        return Ok(None);
                    }
                    return Err(RuntimeError::new("Cannot index into null".into()).at(*span));
                }

                let idx_val = self.eval_expr(index, env)?;
                match base {
                    Value::Array(a) => {
                        let len = a.read().unwrap().len();
                        let res = match &idx_val {
                            Value::Int(i) => resolve_int_index(*i, len),
                            Value::Float(n) | Value::Number(n) => resolve_index(*n, len),
                            _ => return Err(RuntimeError::new("Array index must be a number".into()).at(*span)),
                        };
                        match res {
                            Ok(i) => Ok(Some(a.read().unwrap()[i].clone())),
                            Err(IndexError::NotWhole) => Err(RuntimeError::new(
                                "Array index must be a whole number".into(),
                            )
                            .at(*span)),
                            Err(IndexError::OutOfRange(_)) => {
                                if lenient {
                                    Ok(None)
                                } else {
                                    Err(RuntimeError::new(format!(
                                        "Index {} out of bounds for array of length {}",
                                        idx_val.to_display(), len
                                    ))
                                    .at(*span))
                                }
                            }
                        }
                    }
                    Value::Map(m) => {
                        if let Value::String(s) = idx_val {
                            if let Some(v) = m.read().unwrap().get(&s) {
                                Ok(Some(v.clone()))
                            } else {
                                if lenient {
                                    Ok(None)
                                } else {
                                    Err(RuntimeError::new(format!("Key '{}' not found in map", s))
                                        .at(*span))
                                }
                            }
                        } else {
                            Err(RuntimeError::new("Map key must be a string".into()).at(*span))
                        }
                    }
                    Value::String(s) => {
                        let len = s.len();
                        let res = match &idx_val {
                            Value::Int(i) => resolve_int_index(*i, len),
                            Value::Float(n) | Value::Number(n) => resolve_index(*n, len),
                            _ => return Err(RuntimeError::new("String index must be a number".into()).at(*span)),
                        };
                        match res {
                            Ok(i) => {
                                let ch = s.chars().nth(i).unwrap().to_string();
                                Ok(Some(Value::String(ch)))
                            }
                            Err(IndexError::NotWhole) => Err(RuntimeError::new(
                                "String index must be a whole number".into(),
                            )
                            .at(*span)),
                            Err(IndexError::OutOfRange(_)) => {
                                if lenient {
                                    Ok(None)
                                } else {
                                    Err(RuntimeError::new(format!(
                                        "Index {} out of bounds for string of length {}",
                                        idx_val.to_display(), len
                                    ))
                                    .at(*span))
                                }
                            }
                        }
                    }
                    other => Err(RuntimeError::new(format!(
                        "Cannot index into {}",
                        other.type_name()
                    ))
                    .at(*span)),
                }
            }
            Expr::Call { callee, args, span } => {
                let callee_val = self.eval_chain(callee, env, lenient)?;
                let callee_val = match callee_val {
                    Some(b) => b,
                    None => {
                        if lenient {
                            return Ok(None);
                        }
                        return Err(RuntimeError::new("Cannot call null".into()).at(*span));
                    }
                };
                if matches!(callee_val, Value::Null) {
                    if lenient {
                        return Ok(None);
                    }
                    return Err(RuntimeError::new("Cannot call null".into()).at(*span));
                }

                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval_expr(a, env)?);
                }

                let res = self.call_value(&callee_val, arg_vals, *span)?;
                Ok(Some(res))
            }
            _ => {
                // Evaluate normally
                let val = self.eval_expr(expr, env)?;
                Ok(Some(val))
            }
        }
    }

    pub fn call_value(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        if self.depth >= self.max_depth {
            return Err(
                RuntimeError::new("Stack overflow: maximum call depth exceeded".into()).at(span),
            );
        }

        self.depth += 1;
        let res = match callee {
            Value::Function {
                name,
                params,
                body,
                closure,
            } => {
                if args.len() != params.len() {
                    let fn_name = name.clone().unwrap_or_else(|| "anonymous function".into());
                    self.depth -= 1;
                    return Err(RuntimeError::new(format!(
                        "'{}' expects {} arguments, but got {}",
                        fn_name,
                        params.len(),
                        args.len()
                    ))
                    .at(span));
                }
                let call_env = Arc::new(RwLock::new(Environment::new_with_parent(closure.clone())));
                for (param_name, arg_val) in params.iter().zip(args.into_iter()) {
                    call_env.write().unwrap().define(param_name.clone(), arg_val);
                }
                match self.eval_block(body, &call_env) {
                    Ok(Signal::Return(v)) => Ok(v),
                    Ok(Signal::Value(v)) => Ok(v),
                    Ok(_) => Ok(Value::Null),
                    Err(e) => Err(e),
                }
            }Value::EnumConstructor { enum_name, variant_name, params } => {
                if args.len() != params.len() {
                    self.depth -= 1;
                    return Err(RuntimeError::new(format!(
                        "Enum variant {}.{} expects {} arguments, got {}",
                        enum_name, variant_name, params.len(), args.len()
                    )).at(span));
                }
                Ok(Value::EnumInstance {
                    enum_name: enum_name.clone(),
                    variant_name: variant_name.clone(),
                    values: args,
                })
            }

            Value::Builtin { func, .. } => func(self, args, span),

            Value::BoundMethod { object, method } => {
                match (*object.clone(), method.as_str()) {
                    // Array methods
                    (Value::Array(a), "push") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("push() expects 1 argument".into()).at(span));
                        }
                        a.write().unwrap().push(args[0].clone());
                        Ok(Value::Null)
                    }
                    (Value::Array(a), "pop") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("pop() expects 0 arguments".into()).at(span));
                        }
                        if let Some(val) = a.write().unwrap().pop() {
                            Ok(val)
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    (Value::Array(a), "map") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("map() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("map() argument must be a function".into()).at(span));
                        }
                        let mut new_arr = Vec::new();
                        for item in a.read().unwrap().iter() {
                            let mapped = self.call_value(func, vec![item.clone()], span)?;
                            new_arr.push(mapped);
                        }
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(new_arr))))
                    }
                    (Value::Array(a), "filter") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("filter() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("filter() argument must be a function".into()).at(span));
                        }
                        let mut new_arr = Vec::new();
                        for item in a.read().unwrap().iter() {
                            let keep = self.call_value(func, vec![item.clone()], span)?;
                            if keep.is_truthy() {
                                new_arr.push(item.clone());
                            }
                        }
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(new_arr))))
                    }
                    (Value::Array(a), "reduce") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("reduce() expects 2 arguments (function, initial_value)".into()).at(span));
                        }
                        let func = &args[0];
                        let mut acc = args[1].clone();
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("reduce() first argument must be a function".into()).at(span));
                        }
                        for item in a.read().unwrap().iter() {
                            acc = self.call_value(func, vec![acc, item.clone()], span)?;
                        }
                        Ok(acc)
                    }
                    (Value::Array(a), "sum") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sum() expects 0 arguments".into()).at(span));
                        }
                        let mut sum_int = 0i64;
                        let mut is_all_int = true;
                        let mut sum_float = 0.0f64;
                        for item in a.read().unwrap().iter() {
                            match item {
                                Value::Int(n) => {
                                    sum_int += n;
                                    sum_float += *n as f64;
                                }
                                Value::Float(n) | Value::Number(n) => {
                                    is_all_int = false;
                                    sum_float += n;
                                }
                                _ => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(format!("Cannot sum non-number: {}", item.type_name())).at(span));
                                }
                            }
                        }
                        if is_all_int {
                            Ok(Value::Int(sum_int))
                        } else {
                            Ok(Value::Float(sum_float))
                        }
                    }
                    (Value::Array(a), "sort") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sort() expects 0 arguments".into()).at(span));
                        }
                        let mut arr = a.write().unwrap();
                        arr.sort_by(|x, y| {
                            match (x, y) {
                                (Value::Int(ix), Value::Int(iy)) => ix.cmp(iy),
                                (Value::Int(ix), Value::Float(fy)) | (Value::Int(ix), Value::Number(fy)) => {
                                    (*ix as f64).partial_cmp(fy).unwrap_or(std::cmp::Ordering::Equal)
                                }
                                (Value::Float(fx), Value::Int(iy)) | (Value::Number(fx), Value::Int(iy)) => {
                                    fx.partial_cmp(&(*iy as f64)).unwrap_or(std::cmp::Ordering::Equal)
                                }
                                (Value::Float(nx), Value::Float(ny)) | (Value::Number(nx), Value::Number(ny))
                                | (Value::Float(nx), Value::Number(ny)) | (Value::Number(nx), Value::Float(ny)) => {
                                    nx.partial_cmp(ny).unwrap_or(std::cmp::Ordering::Equal)
                                }
                                (Value::String(sx), Value::String(sy)) => sx.cmp(sy),
                                _ => std::cmp::Ordering::Equal,
                            }
                        });
                        Ok(Value::Array(a.clone()))
                    }
                    (Value::Array(a), "find") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("find() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("find() argument must be a function".into()).at(span));
                        }
                        let mut found = Value::Null;
                        for item in a.read().unwrap().iter() {
                            let matches = self.call_value(func, vec![item.clone()], span)?;
                            if matches.is_truthy() {
                                found = item.clone();
                                break;
                            }
                        }
                        Ok(found)
                    }
                    (Value::Array(a), "some") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("some() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("some() argument must be a function".into()).at(span));
                        }
                        for item in a.read().unwrap().iter() {
                            let matches = self.call_value(func, vec![item.clone()], span)?;
                            if matches.is_truthy() {
                                return Ok(Value::Bool(true));
                            }
                        }
                        Ok(Value::Bool(false))
                    }
                    (Value::Array(a), "every") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("every() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("every() argument must be a function".into()).at(span));
                        }
                        for item in a.read().unwrap().iter() {
                            let matches = self.call_value(func, vec![item.clone()], span)?;
                            if !matches.is_truthy() {
                                return Ok(Value::Bool(false));
                            }
                        }
                        Ok(Value::Bool(true))
                    }
                    (Value::Array(a), "flat") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("flat() expects 0 arguments".into()).at(span));
                        }
                        let mut flattened = Vec::new();
                        for item in a.read().unwrap().iter() {
                            if let Value::Array(nested) = item {
                                for inner in nested.read().unwrap().iter() {
                                    flattened.push(inner.clone());
                                }
                            } else {
                                flattened.push(item.clone());
                            }
                        }
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(flattened))))
                    }
                    (Value::Array(a), "join") => {
                        let sep = if args.is_empty() {
                            ",".to_string()
                        } else if args.len() == 1 {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(format!("join() separator must be a string, got {}", other.type_name())).at(span));
                                }
                            }
                        } else {
                            self.depth -= 1;
                            return Err(RuntimeError::new("join() expects 0 or 1 argument".into()).at(span));
                        };
                        let items: Vec<String> = a.read().unwrap().iter().map(|item| item.to_display()).collect();
                        Ok(Value::String(items.join(&sep)))
                    }
                    (Value::Array(a), "reverse") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("reverse() expects 0 arguments".into()).at(span));
                        }
                        a.write().unwrap().reverse();
                        Ok(Value::Array(a.clone()))
                    }
                    (Value::Array(a), "slice") => {
                        let len = a.read().unwrap().len() as isize;
                        let start = if !args.is_empty() {
                            match to_i64_val(&args[0]) {
                                Some(n) => n as isize,
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new("slice() start index must be an integer".into()).at(span));
                                }
                            }
                        } else {
                            0
                        };
                        let end = if args.len() >= 2 {
                            match to_i64_val(&args[1]) {
                                Some(n) => n as isize,
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new("slice() end index must be an integer".into()).at(span));
                                }
                            }
                        } else {
                            len
                        };
                        if args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("slice() expects 1 or 2 arguments".into()).at(span));
                        }

                        let norm_start = if start < 0 { (start + len).max(0) } else { start.min(len) } as usize;
                        let norm_end = if end < 0 { (end + len).max(0) } else { end.min(len) } as usize;

                        let sliced = if norm_start <= norm_end {
                            a.read().unwrap()[norm_start..norm_end].to_vec()
                        } else {
                            Vec::new()
                        };
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(sliced))))
                    }
                    // String methods
                    (Value::String(s), "trim") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("trim() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.trim().to_string()))
                    }
                    (Value::String(s), "upper") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("upper() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.to_uppercase()))
                    }
                    (Value::String(s), "lower") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("lower() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.to_lowercase()))
                    }
                    (Value::String(s), "split") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("split() expects 1 argument (delimiter)".into()).at(span));
                        }
                        if let Value::String(delim) = &args[0] {
                            let parts: Vec<Value> = s.split(delim).map(|p| Value::String(p.to_string())).collect();
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(parts))))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("split() delimiter must be a string".into()).at(span))
                        }
                    }
                    (Value::String(s), "replace") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("replace() expects 2 arguments (old, new)".into()).at(span));
                        }
                        if let (Value::String(old), Value::String(new)) = (&args[0], &args[1]) {
                            Ok(Value::String(s.replace(old, new)))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("replace() arguments must be strings".into()).at(span))
                        }
                    }
                    (Value::String(s), "starts_with") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("starts_with() expects 1 argument (prefix string)".into()).at(span));
                        }
                        if let Value::String(prefix) = &args[0] {
                            Ok(Value::Bool(s.starts_with(prefix)))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("starts_with() prefix must be a string".into()).at(span))
                        }
                    }
                    (Value::String(s), "ends_with") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("ends_with() expects 1 argument (suffix string)".into()).at(span));
                        }
                        if let Value::String(suffix) = &args[0] {
                            Ok(Value::Bool(s.ends_with(suffix)))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("ends_with() suffix must be a string".into()).at(span))
                        }
                    }
                    (Value::String(s), "contains") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("contains() expects 1 argument (substring)".into()).at(span));
                        }
                        if let Value::String(sub) = &args[0] {
                            Ok(Value::Bool(s.contains(sub)))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("contains() argument must be a string".into()).at(span))
                        }
                    }
                    (Value::String(s), "pad_start") => {
                        if args.is_empty() || args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("pad_start() expects 1 or 2 arguments (target_length, [pad_string])".into()).at(span));
                        }
                        let target_len = match to_i64_val(&args[0]) {
                            Some(n) if n >= 0 => n as usize,
                            _ => {
                                self.depth -= 1;
                                return Err(RuntimeError::new("pad_start() target length must be a non-negative integer".into()).at(span));
                            }
                        };
                        let pad_str = if args.len() == 2 {
                            match &args[1] {
                                Value::String(p) => p.clone(),
                                _ => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new("pad_start() pad string must be a string".into()).at(span));
                                }
                            }
                        } else {
                            " ".to_string()
                        };
                        let current_len = s.chars().count();
                        if current_len >= target_len || pad_str.is_empty() {
                            Ok(Value::String(s.clone()))
                        } else {
                            let mut pad_needed = target_len - current_len;
                            let mut result = String::new();
                            while pad_needed > 0 {
                                for ch in pad_str.chars() {
                                    if pad_needed == 0 { break; }
                                    result.push(ch);
                                    pad_needed -= 1;
                                }
                            }
                            result.push_str(&s);
                            Ok(Value::String(result))
                        }
                    }
                    (Value::String(s), "lines") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("lines() expects 0 arguments".into()).at(span));
                        }
                        let line_vals: Vec<Value> = s.lines().map(|line| Value::String(line.to_string())).collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(line_vals))))
                    }
                    (Value::String(s), "chars") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("chars() expects 0 arguments".into()).at(span));
                        }
                        let char_vals: Vec<Value> = s.chars().map(|ch| Value::String(ch.to_string())).collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(char_vals))))
                    }
                    (Value::String(s), "slice") => {
                        let chars: Vec<char> = s.chars().collect();
                        let len = chars.len() as isize;
                        let start = if !args.is_empty() {
                            match to_i64_val(&args[0]) {
                                Some(n) => n as isize,
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new("slice() start index must be an integer".into()).at(span));
                                }
                            }
                        } else {
                            0
                        };
                        let end = if args.len() >= 2 {
                            match to_i64_val(&args[1]) {
                                Some(n) => n as isize,
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new("slice() end index must be an integer".into()).at(span));
                                }
                            }
                        } else {
                            len
                        };
                        if args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("slice() expects 1 or 2 arguments".into()).at(span));
                        }

                        let norm_start = if start < 0 { (start + len).max(0) } else { start.min(len) } as usize;
                        let norm_end = if end < 0 { (end + len).max(0) } else { end.min(len) } as usize;

                        let sliced: String = if norm_start <= norm_end {
                            chars[norm_start..norm_end].iter().collect()
                        } else {
                            String::new()
                        };
                        Ok(Value::String(sliced))
                    }
                    (Value::Task(t), "join") | (Value::Task(t), "wait") => {
                        let mut guard = t.lock().unwrap();
                        if let Some(handle) = guard.take() {
                            match handle.join() {
                                Ok(Ok(val)) => Ok(val),
                                Ok(Err(e)) => Err(e),
                                Err(_) => Err(RuntimeError::new("Spawned task panicked".into()).at(span)),
                            }
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    (Value::Channel(ch), "send") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "ch.send(val) expects 1 argument".into(),
                            )
                            .at(span));
                        }
                        ch.send(args[0].clone(), span)?;
                        Ok(Value::Null)
                    }
                    (Value::Channel(ch), "recv") => {
                        let timeout = if args.is_empty() {
                            None
                        } else if args.len() == 1 {
                            match args[0] {
                                Value::Int(ms) if ms >= 0 => Some(ms as u64),
                                Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => Some(ms as u64),
                                Value::Null => None,
                                _ => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(
                                        "recv() timeout must be non-negative integer".into(),
                                    )
                                    .at(span));
                                }
                            }
                        } else {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "ch.recv() expects 0 or 1 argument".into(),
                            )
                            .at(span));
                        };
                        ch.recv(timeout, span)
                    }
                    (Value::Channel(ch), "try_recv" | "tryRecv") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "ch.tryRecv() expects 0 arguments".into(),
                            )
                            .at(span));
                        }
                        ch.try_recv(span)
                    }
                    (Value::Channel(ch), "close") => {
                        ch.close();
                        Ok(Value::Null)
                    }
                    (Value::Channel(ch), "len") => {
                        Ok(Value::Int(ch.len() as i64))
                    }
                    (Value::Channel(ch), "capacity") => {
                        if let Some(c) = ch.capacity() {
                            Ok(Value::Int(c as i64))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    _ => {
                        self.depth -= 1;
                        Err(RuntimeError::new(format!("Method {} not found", method)).at(span))
                    }
                }
            }

            other => Err(RuntimeError::new(format!("Cannot call {}", other.type_name())).at(span)),
        };
        self.depth -= 1;
        match res {
            Ok(v) => Ok(v),
            Err(mut e) => {
                let func_name = match callee {
                    Value::Function { name, .. } => name.clone().unwrap_or_else(|| "anonymous".to_string()),
                    Value::Builtin { name, .. } => name.clone(),
                    Value::BoundMethod { method, .. } => method.clone(),
                    Value::EnumConstructor { variant_name, .. } => variant_name.clone(),
                    _ => "unknown".to_string(),
                };
                e.stack.push((func_name, span));
                Err(e)
            }
        }
    }

    fn match_pattern(&self, pattern: &crate::ast::Pattern, val: &Value) -> Option<Vec<(String, Value)>> {
        use crate::ast::{Pattern, Literal};
        match pattern {
            Pattern::Wildcard => Some(Vec::new()),
            Pattern::Variable(name) => Some(vec![(name.clone(), val.clone())]),
            Pattern::Literal(lit) => {
                let lit_val = match lit {
                    Literal::Int(n) => Value::Int(*n),
                    Literal::Float(n) => Value::Float(*n),
                    Literal::Number(n) => Value::Number(*n),
                    Literal::String(s) => Value::String(s.clone()),
                    Literal::Bool(b) => Value::Bool(*b),
                    Literal::Null => Value::Null,
                };
                if val == &lit_val {
                    Some(Vec::new())
                } else {
                    None
                }
            }
            Pattern::Range { start, end, inclusive } => {
                match (val, start, end) {
                    (Value::Int(v), Literal::Int(s), Literal::Int(e)) => {
                        let in_range = if *inclusive {
                            *v >= *s && *v <= *e
                        } else {
                            *v >= *s && *v < *e
                        };
                        if in_range { Some(Vec::new()) } else { None }
                    }
                    (Value::String(v), Literal::String(s), Literal::String(e)) => {
                        let in_range = if *inclusive {
                            v >= s && v <= e
                        } else {
                            v >= s && v < e
                        };
                        if in_range { Some(Vec::new()) } else { None }
                    }
                    _ => {
                        let v_num = to_f64_val(val);
                        let s_num = match start {
                            Literal::Int(n) => Some(*n as f64),
                            Literal::Float(n) | Literal::Number(n) => Some(*n),
                            _ => None,
                        };
                        let e_num = match end {
                            Literal::Int(n) => Some(*n as f64),
                            Literal::Float(n) | Literal::Number(n) => Some(*n),
                            _ => None,
                        };
                        if let (Some(v), Some(s), Some(e)) = (v_num, s_num, e_num) {
                            let in_range = if *inclusive {
                                v >= s && v <= e
                            } else {
                                v >= s && v < e
                            };
                            if in_range { Some(Vec::new()) } else { None }
                        } else {
                            None
                        }
                    }
                }
            }
            Pattern::Enum { enum_name, variant_name, fields } => {
                if let Value::EnumInstance { enum_name: v_enum, variant_name: v_variant, values } = val {
                    let matches_name = match enum_name {
                        Some(e) => e == v_enum && variant_name == v_variant,
                        None => variant_name == v_variant,
                    };
                    if matches_name {
                        if fields.len() != values.len() {
                            return None;
                        }
                        let mut bindings = Vec::new();
                        for (f_pat, v) in fields.iter().zip(values.iter()) {
                            if let Some(sub) = self.match_pattern(f_pat, v) {
                                bindings.extend(sub);
                            } else {
                                return None;
                            }
                        }
                        return Some(bindings);
                    }
                }
                None
            }
        }
    }

    fn bind_pattern_value(
        &mut self,
        pattern: &crate::ast::BindingPattern,
        val: Value,
        env: &Arc<RwLock<Environment>>,
        span: Span,
    ) -> Result<(), RuntimeError> {
        use crate::ast::BindingPattern;
        match pattern {
            BindingPattern::Ident(name) => {
                env.write().unwrap().define(name.clone(), val);
                Ok(())
            }
            BindingPattern::Array { elements, rest } => {
                let arr_guard = match &val {
                    Value::Array(a) => a.read().unwrap().clone(),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "Cannot destructure non-array {} as array",
                            other.type_name()
                        ))
                        .at(span));
                    }
                };
                let mut idx = 0;
                for elem_pat in elements {
                    let elem_val = if idx < arr_guard.len() {
                        arr_guard[idx].clone()
                    } else {
                        Value::Null
                    };
                    self.bind_pattern_value(elem_pat, elem_val, env, span)?;
                    idx += 1;
                }
                if let Some(rest_name) = rest {
                    let rest_vals = if idx < arr_guard.len() {
                        arr_guard[idx..].to_vec()
                    } else {
                        Vec::new()
                    };
                    env.write().unwrap().define(
                        rest_name.clone(),
                        Value::Array(Arc::new(RwLock::new(rest_vals))),
                    );
                }
                Ok(())
            }
            BindingPattern::Object { fields, rest } => {
                let mut extracted_keys = std::collections::HashSet::new();
                match &val {
                    Value::Map(m) => {
                        let map_guard = m.read().unwrap();
                        for (field_name, opt_sub) in fields {
                            extracted_keys.insert(field_name.clone());
                            let field_val = map_guard.get(field_name).cloned().unwrap_or(Value::Null);
                            if let Some(sub_pat) = opt_sub {
                                self.bind_pattern_value(sub_pat, field_val, env, span)?;
                            } else {
                                env.write().unwrap().define(field_name.clone(), field_val);
                            }
                        }
                        if let Some(rest_name) = rest {
                            let mut rest_map = indexmap::IndexMap::new();
                            for (k, v) in map_guard.iter() {
                                if !extracted_keys.contains(k) {
                                    rest_map.insert(k.clone(), v.clone());
                                }
                            }
                            env.write().unwrap().define(
                                rest_name.clone(),
                                Value::Map(Arc::new(RwLock::new(rest_map))),
                            );
                        }
                        Ok(())
                    }
                    Value::StructInstance { fields: struct_fields, .. } => {
                        let fields_guard = struct_fields.read().unwrap();
                        for (field_name, opt_sub) in fields {
                            extracted_keys.insert(field_name.clone());
                            let field_val = fields_guard.get(field_name).cloned().unwrap_or(Value::Null);
                            if let Some(sub_pat) = opt_sub {
                                self.bind_pattern_value(sub_pat, field_val, env, span)?;
                            } else {
                                env.write().unwrap().define(field_name.clone(), field_val);
                            }
                        }
                        if let Some(rest_name) = rest {
                            let mut rest_map = indexmap::IndexMap::new();
                            for (k, v) in fields_guard.iter() {
                                if !extracted_keys.contains(k) {
                                    rest_map.insert(k.clone(), v.clone());
                                }
                            }
                            env.write().unwrap().define(
                                rest_name.clone(),
                                Value::Map(Arc::new(RwLock::new(rest_map))),
                            );
                        }
                        Ok(())
                    }
                    other => Err(RuntimeError::new(format!(
                        "Cannot destructure non-object {} as object",
                        other.type_name()
                    ))
                    .at(span)),
                }
            }
        }
    }
}
