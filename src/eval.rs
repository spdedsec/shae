use crate::ast::{BinaryOp, Expr, InterpPart, Literal, Program, Span, Stmt, StmtKind, UnaryOp};
use crate::env::Environment;
use crate::value::{IndexError, Value, resolve_index, resolve_int_index};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use thiserror::Error;

fn to_i64_val(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) | Value::Number(f)
            if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 =>
        {
            Some(*f as i64)
        }
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

#[derive(Debug, Clone, Error)]
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

pub fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                components.pop();
            }
            c => components.push(c),
        }
    }
    components.into_iter().collect()
}

pub struct Evaluator {
    pub global_env: Arc<RwLock<Environment>>,
    max_depth: usize,
    depth: usize,
    pub current_file: Option<PathBuf>,
    pub module_cache: Arc<RwLock<HashMap<String, Value>>>,
    pub embedded_archive: Option<Arc<HashMap<String, String>>>,
    pub embedded_bytecode: Option<Arc<HashMap<String, Vec<u8>>>>,
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
            embedded_archive: None,
            embedded_bytecode: None,
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
            embedded_archive: None,
            embedded_bytecode: None,
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
            embedded_archive: None,
            embedded_bytecode: None,
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

        // File-based module: resolve relative to current_file or package dependencies
        let resolved_path = if path_str.starts_with("./") || path_str.starts_with("../") {
            if let Some(cur) = &self.current_file {
                if let Some(parent) = cur.parent() {
                    let candidate = parent.join(path_str);
                    if candidate.exists() {
                        candidate
                    } else if candidate.with_extension("shaec").exists() {
                        candidate.with_extension("shaec")
                    } else if candidate.with_extension("shae").exists() {
                        candidate.with_extension("shae")
                    } else {
                        let rel_trimmed = path_str.strip_prefix("./").unwrap_or(path_str);
                        let lib_compiler =
                            std::path::PathBuf::from("lib/compiler").join(rel_trimmed);
                        if lib_compiler.exists() {
                            lib_compiler
                        } else if lib_compiler.with_extension("shaec").exists() {
                            lib_compiler.with_extension("shaec")
                        } else if lib_compiler.with_extension("shae").exists() {
                            lib_compiler.with_extension("shae")
                        } else {
                            candidate
                        }
                    }
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
                } else if candidate.with_extension("shaec").exists() {
                    candidate.with_extension("shaec")
                } else if candidate.with_extension("shae").exists() {
                    candidate.with_extension("shae")
                } else if let Some(pkg_file) = crate::pkg::resolve_package_file(parent, path_str) {
                    pkg_file
                } else {
                    PathBuf::from(path_str)
                }
            } else if let Some(pkg_file) =
                crate::pkg::resolve_package_file(&std::path::PathBuf::from("."), path_str)
            {
                pkg_file
            } else {
                PathBuf::from(path_str)
            }
        } else if let Some(pkg_file) =
            crate::pkg::resolve_package_file(&std::path::PathBuf::from("."), path_str)
        {
            pkg_file
        } else {
            PathBuf::from(path_str)
        };

        let norm_resolved = normalize_path(&resolved_path);
        let canonical_key = match std::fs::canonicalize(&resolved_path) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => norm_resolved.to_string_lossy().to_string(),
        };

        // Cache hit
        if let Some(cached) = self.module_cache.read().unwrap().get(&canonical_key) {
            return Ok(cached.clone());
        }

        // Check embedded bytecode for standalone bundles
        let maybe_archive_bytecode = if let Some(bc_map) = &self.embedded_bytecode {
            let norm_str = norm_resolved.to_string_lossy().to_string();
            let raw_str = resolved_path.to_string_lossy().to_string();
            let trimmed = path_str.strip_prefix("./").unwrap_or(path_str);
            bc_map
                .get(&norm_str)
                .or_else(|| bc_map.get(&raw_str))
                .or_else(|| bc_map.get(&canonical_key))
                .or_else(|| bc_map.get(path_str))
                .or_else(|| bc_map.get(trimmed))
                .or_else(|| {
                    bc_map.iter().find_map(
                        |(k, v)| {
                            if k.ends_with(trimmed) { Some(v) } else { None }
                        },
                    )
                })
                .cloned()
        } else {
            None
        };

        // Check embedded archive for standalone bundles
        let maybe_archive_source = if let Some(archive) = &self.embedded_archive {
            let norm_str = norm_resolved.to_string_lossy().to_string();
            let raw_str = resolved_path.to_string_lossy().to_string();
            let trimmed = path_str.strip_prefix("./").unwrap_or(path_str);
            archive
                .get(&norm_str)
                .or_else(|| archive.get(&raw_str))
                .or_else(|| archive.get(&canonical_key))
                .or_else(|| archive.get(path_str))
                .or_else(|| archive.get(trimmed))
                .or_else(|| {
                    archive.iter().find_map(
                        |(k, v)| {
                            if k.ends_with(trimmed) { Some(v) } else { None }
                        },
                    )
                })
                .cloned()
        } else {
            None
        };

        // Guard against circular import loops by inserting an empty map into cache early
        let export_map = Arc::new(RwLock::new(indexmap::IndexMap::new()));
        let module_val = Value::Map(export_map.clone());
        self.module_cache
            .write()
            .unwrap()
            .insert(canonical_key.clone(), module_val.clone());

        let child_file = match std::fs::canonicalize(&resolved_path) {
            Ok(p) => Some(p),
            Err(_) => Some(norm_resolved),
        };

        let file_bytes = if let Some(bc) = maybe_archive_bytecode {
            bc
        } else if let Some(s) = maybe_archive_source {
            s.into_bytes()
        } else {
            match std::fs::read(&resolved_path) {
                Ok(b) => b,
                Err(e) => {
                    self.module_cache.write().unwrap().remove(&canonical_key);
                    return Err(RuntimeError::new(format!(
                        "Failed to read module '{}': {}",
                        path_str, e
                    ))
                    .at(span));
                }
            }
        };

        if file_bytes.starts_with(crate::chunk::BYTECODE_MAGIC)
            || resolved_path
                .extension()
                .map_or(false, |ext| ext == "shaec")
        {
            let chunk = match crate::chunk::Chunk::from_bytes(&file_bytes) {
                Ok(c) => c,
                Err(e) => {
                    self.module_cache.write().unwrap().remove(&canonical_key);
                    return Err(RuntimeError::new(format!(
                        "Failed to load bytecode module '{}': {}",
                        path_str, e
                    ))
                    .at(span));
                }
            };
            let mut vm = crate::vm::VM::new();
            vm.current_file = child_file;
            vm.embedded_archive = self.embedded_archive.clone();
            vm.embedded_bytecode = self.embedded_bytecode.clone();
            match vm.interpret(chunk) {
                crate::vm::InterpretResult::Ok(_) => {
                    let mut guard = export_map.write().unwrap();
                    let dummy_env = {
                        let mut env = Environment::new();
                        crate::builtins::register(&mut env);
                        env.export_map()
                    };
                    for (k, v) in vm.globals {
                        if k != "gc" && !dummy_env.contains_key(&k) {
                            guard.insert(k, v);
                        }
                    }
                    if let Some(mg) = &vm.module_globals {
                        for (k, v) in mg.read().unwrap().iter() {
                            if k != "gc" && !dummy_env.contains_key(k) {
                                guard.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    return Ok(module_val);
                }
                crate::vm::InterpretResult::RuntimeError(msg) => {
                    self.module_cache.write().unwrap().remove(&canonical_key);
                    return Err(RuntimeError::new(format!(
                        "Runtime error in bytecode module '{}': {}",
                        path_str, msg
                    ))
                    .at(span));
                }
                crate::vm::InterpretResult::CompileError => {
                    self.module_cache.write().unwrap().remove(&canonical_key);
                    return Err(RuntimeError::new(format!(
                        "Compile error in bytecode module '{}'",
                        path_str
                    ))
                    .at(span));
                }
            }
        }

        let source = match String::from_utf8(file_bytes) {
            Ok(s) => s,
            Err(e) => {
                self.module_cache.write().unwrap().remove(&canonical_key);
                return Err(RuntimeError::new(format!(
                    "Module '{}' is not valid UTF-8: {}",
                    path_str, e
                ))
                .at(span));
            }
        };

        let mut builtin_env = Environment::new();
        crate::builtins::register(&mut builtin_env);
        let builtin_env_rc = Arc::new(RwLock::new(builtin_env));
        let module_env = Arc::new(RwLock::new(Environment::new_with_parent(builtin_env_rc)));

        let mut ev = Evaluator::with_env_cache_file(
            module_env.clone(),
            self.module_cache.clone(),
            child_file,
        );
        ev.embedded_archive = self.embedded_archive.clone();
        ev.embedded_bytecode = self.embedded_bytecode.clone();

        let tokens = match crate::lexer::tokenize(&source) {
            Ok(t) => t,
            Err(e) => {
                self.module_cache.write().unwrap().remove(&canonical_key);
                return Err(RuntimeError::new(format!(
                    "Lexer error in module '{}': {}",
                    path_str, e
                ))
                .at(span));
            }
        };
        let program = match crate::parser::parse(tokens) {
            Ok(p) => p,
            Err(e) => {
                self.module_cache.write().unwrap().remove(&canonical_key);
                return Err(RuntimeError::new(format!(
                    "Parser error in module '{}': {}",
                    path_str, e
                ))
                .at(span));
            }
        };
        if let Err(e) = ev.eval_program(&program) {
            self.module_cache.write().unwrap().remove(&canonical_key);
            return Err(RuntimeError::new(format!(
                "Runtime error in module '{}': {}",
                path_str, e.message
            ))
            .at(span));
        }

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
                let field_names = fields.iter().map(|f| f.name.clone()).collect();
                env.write().unwrap().define(
                    name.clone(),
                    Value::StructDef {
                        name: name.clone(),
                        fields: field_names,
                    },
                );
                Ok(Signal::None)
            }
            StmtKind::EnumDef { name, variants } => {
                let mut var_map = indexmap::IndexMap::new();
                for v in variants {
                    var_map.insert(v.name.clone(), v.fields.clone());
                }
                env.write().unwrap().define(
                    name.clone(),
                    Value::EnumDef {
                        name: name.clone(),
                        variants: std::sync::Arc::new(var_map),
                    },
                );
                Ok(Signal::None)
            }
            StmtKind::Expr(expr) => {
                let val = self.eval_expr(expr, env).map_err(|e| e.or_at(stmt.span))?;
                Ok(Signal::Value(val))
            }
            StmtKind::Let {
                pattern,
                type_ann: _,
                init,
            } => {
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
            StmtKind::FnDef {
                name,
                params,
                return_type: _,
                body,
                is_async,
            } => {
                let param_names = Arc::new(params.iter().map(|p| p.name.clone()).collect());
                let func = Value::Function {
                    name: Some(name.clone()),
                    params: param_names,
                    body: body.clone(),
                    closure: env.clone(),
                    is_async: *is_async,
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

            StmtKind::TryCatch {
                try_body,
                catch_ident,
                catch_body,
            } => {
                let try_env = std::sync::Arc::new(std::sync::RwLock::new(
                    crate::env::Environment::new_with_parent(env.clone()),
                ));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = std::sync::Arc::new(std::sync::RwLock::new(
                            crate::env::Environment::new_with_parent(env.clone()),
                        ));
                        catch_env
                            .write()
                            .unwrap()
                            .define(catch_ident.clone(), Value::String(e.message));
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
                        Err(RuntimeError::new(format!(
                            "Struct '{}' has no field '{}'",
                            name, property
                        ))
                        .at(*span))
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
                            _ => {
                                return Err(RuntimeError::new(
                                    "Array index must be a number".into(),
                                )
                                .at(*span));
                            }
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
                                idx_val.to_display(),
                                len
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
                    Value::ByteBuffer(b) => {
                        let byte_val = match value {
                            Value::Int(n) => {
                                if (0..=255).contains(&n) {
                                    n as u8
                                } else {
                                    return Err(RuntimeError::new(format!(
                                        "Buffer element must be in range 0..255, got {}",
                                        n
                                    )).at(*span));
                                }
                            }
                            Value::Float(f) | Value::Number(f) => {
                                if f.fract() == 0.0 && (0.0..=255.0).contains(&f) {
                                    f as u8
                                } else {
                                    return Err(RuntimeError::new(format!(
                                        "Buffer element must be an integer in range 0..255, got {}",
                                        f
                                    )).at(*span));
                                }
                            }
                            _ => {
                                return Err(RuntimeError::new(
                                    "Buffer element must be a byte number (0..255)".into(),
                                ).at(*span));
                            }
                        };
                        let len = b.read().unwrap().len();
                        let res = match idx_val {
                            Value::Int(i) => resolve_int_index(i, len),
                            Value::Float(n) | Value::Number(n) => resolve_index(n, len),
                            _ => {
                                return Err(RuntimeError::new("Buffer index must be a number".into()).at(*span));
                            }
                        };
                        match res {
                            Ok(i) => {
                                match b.write().unwrap().set_byte(i, byte_val) {
                                    Ok(()) => Ok(()),
                                    Err(e) => Err(RuntimeError::new(e).at(*span)),
                                }
                            }
                            Err(IndexError::NotWhole) => Err(RuntimeError::new(
                                "Buffer index must be a whole number".into(),
                            ).at(*span)),
                            Err(IndexError::OutOfRange(_)) => Err(RuntimeError::new(format!(
                                "Index {} out of bounds for buffer of length {}",
                                idx_val.to_display(),
                                len
                            )).at(*span)),
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
                if let Some(Value::StructDef {
                    fields: def_fields, ..
                }) = def
                {
                    if fields.len() != def_fields.len() {
                        return Err(RuntimeError::new(format!(
                            "Struct '{}' expects {} fields, but got {}.",
                            name,
                            def_fields.len(),
                            fields.len()
                        ))
                        .at(*span));
                    }

                    let mut instance_fields = indexmap::IndexMap::new();
                    let mut provided = std::collections::HashSet::new();
                    for (f_name, f_expr) in fields {
                        if !def_fields.contains(f_name) {
                            return Err(RuntimeError::new(format!(
                                "Struct '{}' has no field '{}'.",
                                name, f_name
                            ))
                            .at(*span));
                        }
                        provided.insert(f_name.clone());
                        let val = self.eval_expr(f_expr, env)?;
                        instance_fields.insert(f_name.clone(), val);
                    }
                    if provided.len() != def_fields.len() {
                        return Err(RuntimeError::new(format!(
                            "Struct '{}' initialization is missing fields.",
                            name
                        ))
                        .at(*span));
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
                        let match_env = std::sync::Arc::new(std::sync::RwLock::new(
                            crate::env::Environment::new_with_parent(env.clone()),
                        ));
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
                Err(
                    RuntimeError::new("Non-exhaustive match. No pattern matched the value.".into())
                        .at(*span),
                )
            }
            Expr::Use { path, span } => {
                let path_val = self.eval_expr(path, env)?;
                let path_str = match path_val {
                    Value::String(s) => s,
                    _ => {
                        return Err(
                            RuntimeError::new("Module path must be a string".into()).at(*span)
                        );
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
                        Expr::Call {
                            callee,
                            args,
                            span: call_span,
                        } => {
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
                    UnaryOp::Neg => match inner {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(n) | Value::Number(n) => Ok(Value::Float(-n)),
                        _ => Err(
                            RuntimeError::new(format!("Cannot negate {}", inner.type_name()))
                                .at(*span),
                        ),
                    },
                    UnaryOp::BitNot => {
                        if let Some(n) = to_i64_val(&inner) {
                            Ok(Value::Int(!n))
                        } else {
                            Err(RuntimeError::new(format!(
                                "Cannot bitwise NOT {}",
                                inner.type_name()
                            ))
                            .at(*span))
                        }
                    }
                }
            }
            Expr::Lambda {
                params,
                return_type: _,
                body,
                span: _,
                is_async,
            } => {
                let param_names = Arc::new(params.iter().map(|p| p.name.clone()).collect());
                Ok(Value::Function {
                    name: None,
                    params: param_names,
                    body: body.clone(),
                    closure: env.clone(),
                    is_async: *is_async,
                })
            }
            Expr::Await { expr, span } => {
                let val = self.eval_expr(expr, env).map_err(|e| e.or_at(*span))?;
                match val {
                    Value::Task(task_ref) => {
                        let mut guard = task_ref.lock().unwrap();
                        guard.join().map_err(|e| e.or_at(*span))
                    }
                    other => Ok(other),
                }
            }
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
                            Err(RuntimeError::new(format!(
                                "Struct '{}' has no field '{}'",
                                name, property
                            ))
                            .at(*span))
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
                            Err(RuntimeError::new(format!(
                                "Enum '{}' has no variant '{}'",
                                name, property
                            ))
                            .at(*span))
                        }
                    }
                    Value::Map(m) => {
                        let map = m.read().unwrap();
                        if let Some(v) = map.get(property) {
                            Ok(Some(v.clone()))
                        } else if property == "len" {
                            Ok(Some(Value::Int(map.len() as i64)))
                        } else if matches!(
                            property.as_str(),
                            "keys" | "values" | "has" | "contains" | "get" | "delete"
                        ) || (map.contains_key("_sqlite_handle")
                            && matches!(
                                property.as_str(),
                                "execute" | "query" | "close" | "changes" | "last_insert_id"
                            ))
                            || (map.contains_key("_is_set")
                                && matches!(
                                    property.as_str(),
                                    "add"
                                        | "has"
                                        | "contains"
                                        | "delete"
                                        | "size"
                                        | "to_array"
                                        | "clear"
                                        | "union"
                                        | "intersect"
                                        | "difference"
                                ))
                        {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Map(m.clone())),
                                method: property.clone(),
                            }))
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
                                Err(RuntimeError::new(
                                    "Cannot get 'first' of an empty array".into(),
                                )
                                .at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "last" {
                            let val = a.read().unwrap().last().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(
                                    RuntimeError::new("Cannot get 'last' of an empty array".into())
                                        .at(*span),
                                )
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "push"
                            || property == "pop"
                            || property == "map"
                            || property == "filter"
                            || property == "reduce"
                            || property == "sum"
                            || property == "sort"
                            || property == "find"
                            || property == "some"
                            || property == "every"
                            || property == "flat"
                            || property == "join"
                            || property == "reverse"
                            || property == "slice"
                        {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Array(a.clone())),
                                method: property.clone(),
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "Array has no property '{}'",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
                    Value::String(s) => {
                        if property == "len" {
                            Ok(Some(Value::Int(s.chars().count() as i64)))
                        } else if matches!(
                            property.as_str(),
                            "trim"
                                | "upper"
                                | "lower"
                                | "split"
                                | "replace"
                                | "starts_with"
                                | "ends_with"
                                | "contains"
                                | "pad_start"
                                | "lines"
                                | "chars"
                                | "slice"
                                | "graphemes"
                                | "grapheme_len"
                                | "graphemeLen"
                                | "byte_len"
                                | "byteLen"
                                | "bytes"
                                | "runes"
                                | "codepoints"
                                | "is_ascii"
                                | "isAscii"
                                | "slice_graphemes"
                                | "sliceGraphemes"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::String(s.clone())),
                                method: property.clone(),
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "String has no property '{}'",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
                    Value::Task(t) => {
                        if matches!(
                            property.as_str(),
                            "join"
                                | "wait"
                                | "is_done"
                                | "is_finished"
                                | "isDone"
                                | "isFinished"
                                | "is_ready"
                                | "isReady"
                                | "status"
                                | "result"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Task(t.clone())),
                                method: property.clone(),
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "Task has no property '{}'",
                                    property
                                ))
                                .at(*span))
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
                                Err(RuntimeError::new(format!(
                                    "Channel has no property '{}'",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
                    Value::FfiLibrary(lib) => {
                        if matches!(property.as_str(), "fn" | "symbol" | "get" | "close") {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::FfiLibrary(lib.clone())),
                                method: property.clone(),
                            }))
                        } else if property == "path" {
                            Ok(Some(Value::String(lib.path.clone())))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "FFI library has no property '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::Actor(act) => {
                        if matches!(
                            property.as_str(),
                            "send"
                                | "ask"
                                | "stop"
                                | "is_alive"
                                | "isAlive"
                                | "len"
                                | "mailbox_len"
                                | "mailboxLen"
                                | "id"
                                | "status"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Actor(act.clone())),
                                method: property.clone(),
                            }))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "Actor has no property or method '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::RemoteActor(rem) => {
                        if matches!(
                            property.as_str(),
                            "send" | "ask" | "is_alive" | "isAlive" | "node" | "name"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::RemoteActor(rem.clone())),
                                method: property.clone(),
                            }))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "RemoteActor has no property or method '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::ClusterNode(node) => {
                        if matches!(
                            property.as_str(),
                            "connect"
                                | "join"
                                | "peers"
                                | "nodes"
                                | "actor"
                                | "whereis"
                                | "spawn"
                                | "register"
                                | "unregister"
                                | "registered"
                                | "send"
                                | "ask"
                                | "broadcast"
                                | "on_broadcast"
                                | "stop"
                                | "is_alive"
                                | "isAlive"
                                | "is_tls"
                                | "isTls"
                                | "name"
                                | "address"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::ClusterNode(node.clone())),
                                method: property.clone(),
                            }))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "ClusterNode has no property or method '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::Supervisor(sup) => {
                        if matches!(
                            property.as_str(),
                            "get_child"
                                | "getChild"
                                | "start_child"
                                | "startChild"
                                | "stop_child"
                                | "stopChild"
                                | "restart_child"
                                | "restartChild"
                                | "children"
                                | "stop"
                                | "is_alive"
                                | "isAlive"
                                | "id"
                                | "status"
                                | "len"
                        ) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Supervisor(sup.clone())),
                                method: property.clone(),
                            }))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "Supervisor has no property or method '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::ByteBuffer(b) => {
                        if property == "len" {
                            Ok(Some(Value::Int(b.read().unwrap().len() as i64)))
                        } else if property == "offset" {
                            Ok(Some(Value::Int(b.read().unwrap().offset() as i64)))
                        } else if crate::buffer::is_buffer_method(property) {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::ByteBuffer(b.clone())),
                                method: property.clone(),
                            }))
                        } else if *safe || lenient {
                            Ok(None)
                        } else {
                            Err(RuntimeError::new(format!(
                                "ByteBuffer has no property or method '{}'",
                                property
                            ))
                            .at(*span))
                        }
                    }
                    Value::Builtin { name, .. } if name == "ByteBuffer" => {
                        match property.as_str() {
                            "alloc" => Ok(Some(Value::Builtin {
                                name: "alloc".into(),
                                func: crate::stdlib::buffer_alloc,
                            })),
                            "from" => Ok(Some(Value::Builtin {
                                name: "from".into(),
                                func: crate::stdlib::buffer_from,
                            })),
                            "from_hex" | "fromHex" => Ok(Some(Value::Builtin {
                                name: property.clone(),
                                func: crate::stdlib::buffer_from_hex,
                            })),
                            "from_utf8" | "fromUtf8" => Ok(Some(Value::Builtin {
                                name: property.clone(),
                                func: crate::stdlib::buffer_from_utf8,
                            })),
                            "concat" => Ok(Some(Value::Builtin {
                                name: "concat".into(),
                                func: crate::stdlib::buffer_concat,
                            })),
                            _ => {
                                if *safe || lenient {
                                    Ok(None)
                                } else {
                                    Err(RuntimeError::new(format!(
                                        "ByteBuffer has no static property '{}'",
                                        property
                                    ))
                                    .at(*span))
                                }
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
                            _ => {
                                return Err(RuntimeError::new(
                                    "Array index must be a number".into(),
                                )
                                .at(*span));
                            }
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
                                        idx_val.to_display(),
                                        len
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
                            _ => {
                                return Err(RuntimeError::new(
                                    "String index must be a number".into(),
                                )
                                .at(*span));
                            }
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
                                        idx_val.to_display(),
                                        len
                                    ))
                                    .at(*span))
                                }
                            }
                        }
                    }
                    Value::ByteBuffer(b) => {
                        let buf = b.read().unwrap();
                        let len = buf.len();
                        let res = match &idx_val {
                            Value::Int(i) => resolve_int_index(*i, len),
                            Value::Float(n) | Value::Number(n) => resolve_index(*n, len),
                            _ => {
                                return Err(RuntimeError::new(
                                    "Buffer index must be a number".into(),
                                )
                                .at(*span));
                            }
                        };
                        match res {
                            Ok(i) => match buf.get_byte(i) {
                                Ok(byte) => Ok(Some(Value::Int(byte as i64))),
                                Err(e) => Err(RuntimeError::new(e).at(*span)),
                            },
                            Err(IndexError::NotWhole) => Err(RuntimeError::new(
                                "Buffer index must be a whole number".into(),
                            )
                            .at(*span)),
                            Err(IndexError::OutOfRange(_)) => {
                                if lenient {
                                    Ok(None)
                                } else {
                                    Err(RuntimeError::new(format!(
                                        "Index {} out of bounds for buffer of length {}",
                                        idx_val.to_display(),
                                        len
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
                is_async,
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
                if *is_async {
                    self.depth -= 1;
                    let call_env =
                        Arc::new(RwLock::new(Environment::new_with_parent(closure.clone())));
                    for (param_name, arg_val) in params.iter().zip(args) {
                        call_env
                            .write()
                            .unwrap()
                            .define(param_name.clone(), arg_val);
                    }
                    let body = body.clone();
                    let global_env = self.global_env.clone();
                    let current_file = self.current_file.clone();
                    let handle = crate::async_rt::spawn_task(move || {
                        let mut eval = Evaluator::with_env(global_env);
                        eval.current_file = current_file;
                        match eval.eval_block(&body, &call_env) {
                            Ok(Signal::Return(v)) => Ok(v),
                            Ok(Signal::Value(v)) => Ok(v),
                            Ok(_) => Ok(Value::Null),
                            Err(e) => Err(e),
                        }
                    });
                    return Ok(Value::Task(Arc::new(std::sync::Mutex::new(
                        crate::value::TaskInner::new(handle),
                    ))));
                }
                let call_env = Arc::new(RwLock::new(Environment::new_with_parent(closure.clone())));
                for (param_name, arg_val) in params.iter().zip(args) {
                    call_env
                        .write()
                        .unwrap()
                        .define(param_name.clone(), arg_val);
                }
                match self.eval_block(body, &call_env) {
                    Ok(Signal::Return(v)) => Ok(v),
                    Ok(Signal::Value(v)) => Ok(v),
                    Ok(_) => Ok(Value::Null),
                    Err(e) => Err(e),
                }
            }
            Value::Closure(closure) => {
                if args.len() != closure.function.arity {
                    self.depth -= 1;
                    return Err(RuntimeError::new(format!(
                        "Expected {} arguments but got {}.",
                        closure.function.arity,
                        args.len()
                    ))
                    .at(span));
                }
                if closure.function.is_async {
                    self.depth -= 1;
                    let closure = closure.clone();
                    let current_file = self.current_file.clone();
                    let global_map = self.global_env.read().unwrap().export_map();
                    let handle = crate::async_rt::spawn_task(move || {
                        let mut vm = crate::vm::VM::new();
                        vm.current_file = current_file;
                        for (k, v) in global_map {
                            vm.globals.insert(k, v);
                        }
                        match vm.execute_closure(closure, args) {
                            crate::vm::InterpretResult::Ok(val) => Ok(val),
                            crate::vm::InterpretResult::RuntimeError(msg) => {
                                Err(RuntimeError::new(msg).at(span))
                            }
                            crate::vm::InterpretResult::CompileError => {
                                Err(RuntimeError::new("Compile error in VM".into()).at(span))
                            }
                        }
                    });
                    return Ok(Value::Task(Arc::new(std::sync::Mutex::new(
                        crate::value::TaskInner::new(handle),
                    ))));
                }
                let mut vm = crate::vm::VM::new();
                vm.current_file = self.current_file.clone();
                for (k, v) in self.global_env.read().unwrap().export_map() {
                    vm.globals.insert(k, v);
                }
                match vm.execute_closure(closure.clone(), args) {
                    crate::vm::InterpretResult::Ok(val) => Ok(val),
                    crate::vm::InterpretResult::RuntimeError(msg) => {
                        Err(RuntimeError::new(msg).at(span))
                    }
                    crate::vm::InterpretResult::CompileError => {
                        Err(RuntimeError::new("Compile error in VM".into()).at(span))
                    }
                }
            }
            Value::CompiledFunction(func) => {
                let closure = Arc::new(crate::value::Closure {
                    function: func.clone(),
                    upvalues: Vec::new(),
                    module_globals: None,
                });
                if func.is_async {
                    self.depth -= 1;
                    let current_file = self.current_file.clone();
                    let global_map = self.global_env.read().unwrap().export_map();
                    let handle = crate::async_rt::spawn_task(move || {
                        let mut vm = crate::vm::VM::new();
                        vm.current_file = current_file;
                        for (k, v) in global_map {
                            vm.globals.insert(k, v);
                        }
                        match vm.execute_closure(closure, args) {
                            crate::vm::InterpretResult::Ok(val) => Ok(val),
                            crate::vm::InterpretResult::RuntimeError(msg) => {
                                Err(RuntimeError::new(msg).at(span))
                            }
                            crate::vm::InterpretResult::CompileError => {
                                Err(RuntimeError::new("Compile error in VM".into()).at(span))
                            }
                        }
                    });
                    return Ok(Value::Task(Arc::new(std::sync::Mutex::new(
                        crate::value::TaskInner::new(handle),
                    ))));
                }
                let mut vm = crate::vm::VM::new();
                vm.current_file = self.current_file.clone();
                for (k, v) in self.global_env.read().unwrap().export_map() {
                    vm.globals.insert(k, v);
                }
                match vm.execute_closure(closure, args) {
                    crate::vm::InterpretResult::Ok(val) => Ok(val),
                    crate::vm::InterpretResult::RuntimeError(msg) => {
                        Err(RuntimeError::new(msg).at(span))
                    }
                    crate::vm::InterpretResult::CompileError => {
                        Err(RuntimeError::new("Compile error in VM".into()).at(span))
                    }
                }
            }
            Value::EnumConstructor {
                enum_name,
                variant_name,
                params,
            } => {
                if args.len() != params.len() {
                    self.depth -= 1;
                    return Err(RuntimeError::new(format!(
                        "Enum variant {}.{} expects {} arguments, got {}",
                        enum_name,
                        variant_name,
                        params.len(),
                        args.len()
                    ))
                    .at(span));
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
                            return Err(
                                RuntimeError::new("push() expects 1 argument".into()).at(span)
                            );
                        }
                        a.write().unwrap().push(args[0].clone());
                        Ok(Value::Null)
                    }
                    (Value::Array(a), "pop") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("pop() expects 0 arguments".into()).at(span)
                            );
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
                            return Err(RuntimeError::new(
                                "map() expects 1 argument (a function)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "map() argument must be a function".into(),
                            )
                            .at(span));
                        }
                        let items: Vec<Value> = a.read().unwrap().clone();
                        let mut new_arr = Vec::with_capacity(items.len());
                        for item in items {
                            let mapped = self.call_value(func, vec![item], span)?;
                            new_arr.push(mapped);
                        }
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            new_arr,
                        ))))
                    }
                    (Value::Array(a), "filter") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "filter() expects 1 argument (a function)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "filter() argument must be a function".into(),
                            )
                            .at(span));
                        }
                        let items: Vec<Value> = a.read().unwrap().clone();
                        let mut new_arr = Vec::new();
                        for item in items {
                            let keep = self.call_value(func, vec![item.clone()], span)?;
                            if keep.is_truthy() {
                                new_arr.push(item);
                            }
                        }
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            new_arr,
                        ))))
                    }
                    (Value::Array(a), "reduce") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "reduce() expects 2 arguments (function, initial_value)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        let mut acc = args[1].clone();
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "reduce() first argument must be a function".into(),
                            )
                            .at(span));
                        }
                        let items: Vec<Value> = a.read().unwrap().clone();
                        for item in items {
                            acc = self.call_value(func, vec![acc, item], span)?;
                        }
                        Ok(acc)
                    }
                    (Value::Array(a), "sum") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("sum() expects 0 arguments".into()).at(span)
                            );
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
                                    return Err(RuntimeError::new(format!(
                                        "Cannot sum non-number: {}",
                                        item.type_name()
                                    ))
                                    .at(span));
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
                            return Err(
                                RuntimeError::new("sort() expects 0 arguments".into()).at(span)
                            );
                        }
                        let mut arr = a.write().unwrap();
                        arr.sort_by(|x, y| match (x, y) {
                            (Value::Int(ix), Value::Int(iy)) => ix.cmp(iy),
                            (Value::Int(ix), Value::Float(fy))
                            | (Value::Int(ix), Value::Number(fy)) => (*ix as f64)
                                .partial_cmp(fy)
                                .unwrap_or(std::cmp::Ordering::Equal),
                            (Value::Float(fx), Value::Int(iy))
                            | (Value::Number(fx), Value::Int(iy)) => fx
                                .partial_cmp(&(*iy as f64))
                                .unwrap_or(std::cmp::Ordering::Equal),
                            (Value::Float(nx), Value::Float(ny))
                            | (Value::Number(nx), Value::Number(ny))
                            | (Value::Float(nx), Value::Number(ny))
                            | (Value::Number(nx), Value::Float(ny)) => {
                                nx.partial_cmp(ny).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            (Value::String(sx), Value::String(sy)) => sx.cmp(sy),
                            _ => std::cmp::Ordering::Equal,
                        });
                        Ok(Value::Array(a.clone()))
                    }
                    (Value::Array(a), "find") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "find() expects 1 argument (a function)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "find() argument must be a function".into(),
                            )
                            .at(span));
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
                            return Err(RuntimeError::new(
                                "some() expects 1 argument (a function)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "some() argument must be a function".into(),
                            )
                            .at(span));
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
                            return Err(RuntimeError::new(
                                "every() expects 1 argument (a function)".into(),
                            )
                            .at(span));
                        }
                        let func = &args[0];
                        if !matches!(
                            func,
                            Value::Function { .. }
                                | Value::Builtin { .. }
                                | Value::BoundMethod { .. }
                                | Value::Closure(_)
                                | Value::CompiledFunction(_)
                        ) {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "every() argument must be a function".into(),
                            )
                            .at(span));
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
                            return Err(
                                RuntimeError::new("flat() expects 0 arguments".into()).at(span)
                            );
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
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            flattened,
                        ))))
                    }
                    (Value::Array(a), "join") => {
                        let sep = if args.is_empty() {
                            ",".to_string()
                        } else if args.len() == 1 {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(format!(
                                        "join() separator must be a string, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            }
                        } else {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("join() expects 0 or 1 argument".into()).at(span)
                            );
                        };
                        let items: Vec<String> = a
                            .read()
                            .unwrap()
                            .iter()
                            .map(|item| item.to_display())
                            .collect();
                        Ok(Value::String(items.join(&sep)))
                    }
                    (Value::Array(a), "reverse") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("reverse() expects 0 arguments".into()).at(span)
                            );
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
                                    return Err(RuntimeError::new(
                                        "slice() start index must be an integer".into(),
                                    )
                                    .at(span));
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
                                    return Err(RuntimeError::new(
                                        "slice() end index must be an integer".into(),
                                    )
                                    .at(span));
                                }
                            }
                        } else {
                            len
                        };
                        if args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "slice() expects 1 or 2 arguments".into(),
                            )
                            .at(span));
                        }

                        let norm_start = if start < 0 {
                            (start + len).max(0)
                        } else {
                            start.min(len)
                        } as usize;
                        let norm_end = if end < 0 {
                            (end + len).max(0)
                        } else {
                            end.min(len)
                        } as usize;

                        let sliced = if norm_start <= norm_end {
                            a.read().unwrap()[norm_start..norm_end].to_vec()
                        } else {
                            Vec::new()
                        };
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            sliced,
                        ))))
                    }
                    // String methods
                    (Value::String(s), "trim") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("trim() expects 0 arguments".into()).at(span)
                            );
                        }
                        Ok(Value::String(s.trim().to_string()))
                    }
                    (Value::String(s), "upper") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("upper() expects 0 arguments".into()).at(span)
                            );
                        }
                        Ok(Value::String(s.to_uppercase()))
                    }
                    (Value::String(s), "lower") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("lower() expects 0 arguments".into()).at(span)
                            );
                        }
                        Ok(Value::String(s.to_lowercase()))
                    }
                    (Value::String(s), "split") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "split() expects 1 argument (delimiter)".into(),
                            )
                            .at(span));
                        }
                        if let Value::String(delim) = &args[0] {
                            let parts: Vec<Value> = s
                                .split(delim)
                                .map(|p| Value::String(p.to_string()))
                                .collect();
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                parts,
                            ))))
                        } else {
                            self.depth -= 1;
                            Err(
                                RuntimeError::new("split() delimiter must be a string".into())
                                    .at(span),
                            )
                        }
                    }
                    (Value::String(s), "replace") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "replace() expects 2 arguments (old, new)".into(),
                            )
                            .at(span));
                        }
                        if let (Value::String(old), Value::String(new)) = (&args[0], &args[1]) {
                            Ok(Value::String(s.replace(old, new)))
                        } else {
                            self.depth -= 1;
                            Err(
                                RuntimeError::new("replace() arguments must be strings".into())
                                    .at(span),
                            )
                        }
                    }
                    (Value::String(s), "starts_with") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "starts_with() expects 1 argument (prefix string)".into(),
                            )
                            .at(span));
                        }
                        if let Value::String(prefix) = &args[0] {
                            Ok(Value::Bool(s.starts_with(prefix)))
                        } else {
                            self.depth -= 1;
                            Err(
                                RuntimeError::new("starts_with() prefix must be a string".into())
                                    .at(span),
                            )
                        }
                    }
                    (Value::String(s), "ends_with") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "ends_with() expects 1 argument (suffix string)".into(),
                            )
                            .at(span));
                        }
                        if let Value::String(suffix) = &args[0] {
                            Ok(Value::Bool(s.ends_with(suffix)))
                        } else {
                            self.depth -= 1;
                            Err(
                                RuntimeError::new("ends_with() suffix must be a string".into())
                                    .at(span),
                            )
                        }
                    }
                    (Value::String(s), "contains") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "contains() expects 1 argument (substring)".into(),
                            )
                            .at(span));
                        }
                        if let Value::String(sub) = &args[0] {
                            Ok(Value::Bool(s.contains(sub)))
                        } else {
                            self.depth -= 1;
                            Err(
                                RuntimeError::new("contains() argument must be a string".into())
                                    .at(span),
                            )
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
                                return Err(RuntimeError::new(
                                    "pad_start() target length must be a non-negative integer"
                                        .into(),
                                )
                                .at(span));
                            }
                        };
                        let pad_str = if args.len() == 2 {
                            match &args[1] {
                                Value::String(p) => p.clone(),
                                _ => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(
                                        "pad_start() pad string must be a string".into(),
                                    )
                                    .at(span));
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
                                    if pad_needed == 0 {
                                        break;
                                    }
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
                            return Err(
                                RuntimeError::new("lines() expects 0 arguments".into()).at(span)
                            );
                        }
                        let line_vals: Vec<Value> = s
                            .lines()
                            .map(|line| Value::String(line.to_string()))
                            .collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            line_vals,
                        ))))
                    }
                    (Value::String(s), "chars") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("chars() expects 0 arguments".into()).at(span)
                            );
                        }
                        let char_vals: Vec<Value> =
                            s.chars().map(|ch| Value::String(ch.to_string())).collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            char_vals,
                        ))))
                    }
                    (Value::String(s), "slice") => {
                        let chars: Vec<char> = s.chars().collect();
                        let len = chars.len() as isize;
                        let start = if !args.is_empty() {
                            match to_i64_val(&args[0]) {
                                Some(n) => n as isize,
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(
                                        "slice() start index must be an integer".into(),
                                    )
                                    .at(span));
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
                                    return Err(RuntimeError::new(
                                        "slice() end index must be an integer".into(),
                                    )
                                    .at(span));
                                }
                            }
                        } else {
                            len
                        };
                        if args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "slice() expects 1 or 2 arguments".into(),
                            )
                            .at(span));
                        }

                        let norm_start = if start < 0 {
                            (start + len).max(0)
                        } else {
                            start.min(len)
                        } as usize;
                        let norm_end = if end < 0 {
                            (end + len).max(0)
                        } else {
                            end.min(len)
                        } as usize;

                        let sliced: String = if norm_start <= norm_end {
                            chars[norm_start..norm_end].iter().collect()
                        } else {
                            String::new()
                        };
                        Ok(Value::String(sliced))
                    }
                    (Value::String(s), "graphemes") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "graphemes() expects 0 arguments".into(),
                            )
                            .at(span));
                        }
                        let items: Vec<Value> = crate::unicode::graphemes(&s)
                            .into_iter()
                            .map(|g| Value::String(g.to_string()))
                            .collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            items,
                        ))))
                    }
                    (Value::String(s), "grapheme_len") | (Value::String(s), "graphemeLen") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "grapheme_len() expects 0 arguments".into(),
                            )
                            .at(span));
                        }
                        Ok(Value::Int(crate::unicode::grapheme_len(&s) as i64))
                    }
                    (Value::String(s), "byte_len") | (Value::String(s), "byteLen") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("byte_len() expects 0 arguments".into()).at(span)
                            );
                        }
                        Ok(Value::Int(s.len() as i64))
                    }
                    (Value::String(s), "bytes") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("bytes() expects 0 arguments".into()).at(span)
                            );
                        }
                        let items: Vec<Value> = s.bytes().map(|b| Value::Int(b as i64)).collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            items,
                        ))))
                    }
                    (Value::String(s), "runes") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("runes() expects 0 arguments".into()).at(span)
                            );
                        }
                        let items: Vec<Value> = crate::unicode::runes(&s)
                            .into_iter()
                            .map(Value::String)
                            .collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            items,
                        ))))
                    }
                    (Value::String(s), "codepoints") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "codepoints() expects 0 arguments".into(),
                            )
                            .at(span));
                        }
                        let items: Vec<Value> = crate::unicode::codepoints(&s)
                            .into_iter()
                            .map(Value::Int)
                            .collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            items,
                        ))))
                    }
                    (Value::String(s), "is_ascii") | (Value::String(s), "isAscii") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("is_ascii() expects 0 arguments".into()).at(span)
                            );
                        }
                        Ok(Value::Bool(crate::unicode::is_ascii(&s)))
                    }
                    (Value::String(s), "slice_graphemes")
                    | (Value::String(s), "sliceGraphemes") => {
                        if args.is_empty() || args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "slice_graphemes() expects 1 or 2 arguments (start, [end])".into(),
                            )
                            .at(span));
                        }
                        let start = match to_i64_val(&args[0]) {
                            Some(n) => n,
                            None => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(
                                    "slice_graphemes() start index must be an integer".into(),
                                )
                                .at(span));
                            }
                        };
                        let end = if args.len() >= 2 {
                            match to_i64_val(&args[1]) {
                                Some(n) => Some(n),
                                None => {
                                    self.depth -= 1;
                                    return Err(RuntimeError::new(
                                        "slice_graphemes() end index must be an integer".into(),
                                    )
                                    .at(span));
                                }
                            }
                        } else {
                            None
                        };
                        Ok(Value::String(crate::unicode::slice_graphemes(
                            &s, start, end,
                        )))
                    }
                    (Value::Task(t), "join") | (Value::Task(t), "wait") => {
                        let mut guard = t.lock().unwrap();
                        guard.join().map_err(|e| e.or_at(span))
                    }
                    (
                        Value::Task(t),
                        "is_done" | "is_finished" | "isDone" | "isFinished" | "is_ready"
                        | "isReady",
                    ) => {
                        let guard = t.lock().unwrap();
                        Ok(Value::Bool(guard.is_finished()))
                    }
                    (Value::Task(t), "status") => {
                        let guard = t.lock().unwrap();
                        let s = if let Some(ref res) = guard.cached_result {
                            match res {
                                Ok(_) => "completed",
                                Err(_) => "failed",
                            }
                        } else if guard.is_finished() {
                            "completed"
                        } else {
                            "pending"
                        };
                        Ok(Value::String(s.to_string()))
                    }
                    (Value::Task(t), "result") => {
                        let guard = t.lock().unwrap();
                        if let Some(ref res) = guard.cached_result {
                            match res {
                                Ok(v) => Ok(v.clone()),
                                Err(e) => Err(e.clone().or_at(span)),
                            }
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    (Value::Channel(ch), "send") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new("ch.send(val) expects 1 argument".into())
                                .at(span))
                        } else {
                            ch.send(args[0].clone(), span).map(|_| Value::Null)
                        }
                    }
                    (Value::Channel(ch), "recv") => {
                        let timeout = if args.is_empty() {
                            None
                        } else if args.len() == 1 {
                            match args[0] {
                                Value::Int(ms) if ms >= 0 => Some(ms as u64),
                                Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => {
                                    Some(ms as u64)
                                }
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
                    (Value::Channel(ch), "len") => Ok(Value::Int(ch.len() as i64)),
                    (Value::Channel(ch), "capacity") => {
                        if let Some(c) = ch.capacity() {
                            Ok(Value::Int(c as i64))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    (Value::Map(m), "keys") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("keys() expects 0 arguments".into()).at(span)
                            );
                        }
                        let keys: Vec<Value> = m
                            .read()
                            .unwrap()
                            .keys()
                            .map(|k| Value::String(k.clone()))
                            .collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            keys,
                        ))))
                    }
                    (Value::Map(m), "values") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("values() expects 0 arguments".into()).at(span)
                            );
                        }
                        let values: Vec<Value> = m.read().unwrap().values().cloned().collect();
                        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                            values,
                        ))))
                    }
                    (Value::Map(m), "has" | "contains") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("has() expects 1 argument".into()).at(span)
                            );
                        }
                        if m.read().unwrap().contains_key("_is_set") {
                            let item = &args[0];
                            let guard = m.read().unwrap();
                            if let Some(Value::Array(arr)) = guard.get("items") {
                                let arr_guard = arr.read().unwrap();
                                let set: indexmap::IndexSet<Value> =
                                    arr_guard.iter().cloned().collect();
                                return Ok(Value::Bool(set.contains(item)));
                            } else {
                                return Ok(Value::Bool(false));
                            }
                        }
                        let key = match &args[0] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        Ok(Value::Bool(m.read().unwrap().contains_key(&key)))
                    }
                    (Value::Map(m), "get") => {
                        if args.is_empty() || args.len() > 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "get() expects 1 or 2 arguments (key, [default])".into(),
                            )
                            .at(span));
                        }
                        let key = match &args[0] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let default_val = if args.len() == 2 {
                            args[1].clone()
                        } else {
                            Value::Null
                        };
                        let val = m.read().unwrap().get(&key).cloned().unwrap_or(default_val);
                        Ok(val)
                    }
                    (Value::Map(m), "delete") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("delete() expects 1 argument".into()).at(span)
                            );
                        }
                        if m.read().unwrap().contains_key("_is_set") {
                            let item = &args[0];
                            let guard = m.read().unwrap();
                            if let Some(Value::Array(arr)) = guard.get("items") {
                                let mut arr_guard = arr.write().unwrap();
                                let mut set: indexmap::IndexSet<Value> =
                                    arr_guard.drain(..).collect();
                                let removed = set.shift_remove(item);
                                *arr_guard = set.into_iter().collect();
                                return Ok(Value::Bool(removed));
                            } else {
                                return Ok(Value::Bool(false));
                            }
                        }
                        let key = match &args[0] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let prev = m.write().unwrap().shift_remove(&key).unwrap_or(Value::Null);
                        Ok(prev)
                    }
                    (Value::Map(m), "add") if m.read().unwrap().contains_key("_is_set") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(
                                RuntimeError::new("add() expects 1 argument".into()).at(span)
                            );
                        }
                        let item = args[0].clone();
                        let guard = m.read().unwrap();
                        if let Some(Value::Array(arr)) = guard.get("items") {
                            let mut arr_guard = arr.write().unwrap();
                            let mut set: indexmap::IndexSet<Value> = arr_guard.drain(..).collect();
                            let inserted = set.insert(item);
                            *arr_guard = set.into_iter().collect();
                            Ok(Value::Bool(inserted))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    (Value::Map(m), "size") if m.read().unwrap().contains_key("_is_set") => {
                        let guard = m.read().unwrap();
                        if let Some(Value::Array(arr)) = guard.get("items") {
                            Ok(Value::Int(arr.read().unwrap().len() as i64))
                        } else {
                            Ok(Value::Int(0))
                        }
                    }
                    (Value::Map(m), "to_array") if m.read().unwrap().contains_key("_is_set") => {
                        let guard = m.read().unwrap();
                        if let Some(Value::Array(arr)) = guard.get("items") {
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                arr.read().unwrap().clone(),
                            ))))
                        } else {
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                Vec::new(),
                            ))))
                        }
                    }
                    (Value::Map(m), "clear") if m.read().unwrap().contains_key("_is_set") => {
                        let guard = m.read().unwrap();
                        if let Some(Value::Array(arr)) = guard.get("items") {
                            arr.write().unwrap().clear();
                        }
                        Ok(Value::Null)
                    }
                    (Value::Map(m), "union") if m.read().unwrap().contains_key("_is_set") => {
                        let items_a = match m.read().unwrap().get("items") {
                            Some(Value::Array(a)) => a.read().unwrap().clone(),
                            _ => Vec::new(),
                        };
                        let items_b = if !args.is_empty() {
                            match &args[0] {
                                Value::Array(b) => b.read().unwrap().clone(),
                                Value::Map(other_m)
                                    if other_m.read().unwrap().contains_key("_is_set") =>
                                {
                                    match other_m.read().unwrap().get("items") {
                                        Some(Value::Array(b)) => b.read().unwrap().clone(),
                                        _ => Vec::new(),
                                    }
                                }
                                _ => Vec::new(),
                            }
                        } else {
                            Vec::new()
                        };
                        let set: indexmap::IndexSet<Value> =
                            items_a.into_iter().chain(items_b.into_iter()).collect();
                        let res: Vec<Value> = set.into_iter().collect();
                        Ok(crate::stdlib::make_set_instance(res))
                    }
                    (Value::Map(m), "intersect") if m.read().unwrap().contains_key("_is_set") => {
                        let items_a = match m.read().unwrap().get("items") {
                            Some(Value::Array(a)) => a.read().unwrap().clone(),
                            _ => Vec::new(),
                        };
                        let set_b: indexmap::IndexSet<Value> = if !args.is_empty() {
                            match &args[0] {
                                Value::Array(b) => b.read().unwrap().iter().cloned().collect(),
                                Value::Map(other_m)
                                    if other_m.read().unwrap().contains_key("_is_set") =>
                                {
                                    match other_m.read().unwrap().get("items") {
                                        Some(Value::Array(b)) => {
                                            b.read().unwrap().iter().cloned().collect()
                                        }
                                        _ => indexmap::IndexSet::new(),
                                    }
                                }
                                _ => indexmap::IndexSet::new(),
                            }
                        } else {
                            indexmap::IndexSet::new()
                        };
                        let mut out = indexmap::IndexSet::new();
                        for item in items_a {
                            if set_b.contains(&item) {
                                out.insert(item);
                            }
                        }
                        let res: Vec<Value> = out.into_iter().collect();
                        Ok(crate::stdlib::make_set_instance(res))
                    }
                    (Value::Map(m), "difference") if m.read().unwrap().contains_key("_is_set") => {
                        let items_a = match m.read().unwrap().get("items") {
                            Some(Value::Array(a)) => a.read().unwrap().clone(),
                            _ => Vec::new(),
                        };
                        let set_b: indexmap::IndexSet<Value> = if !args.is_empty() {
                            match &args[0] {
                                Value::Array(b) => b.read().unwrap().iter().cloned().collect(),
                                Value::Map(other_m)
                                    if other_m.read().unwrap().contains_key("_is_set") =>
                                {
                                    match other_m.read().unwrap().get("items") {
                                        Some(Value::Array(b)) => {
                                            b.read().unwrap().iter().cloned().collect()
                                        }
                                        _ => indexmap::IndexSet::new(),
                                    }
                                }
                                _ => indexmap::IndexSet::new(),
                            }
                        } else {
                            indexmap::IndexSet::new()
                        };
                        let mut out = indexmap::IndexSet::new();
                        for item in items_a {
                            if !set_b.contains(&item) {
                                out.insert(item);
                            }
                        }
                        let res: Vec<Value> = out.into_iter().collect();
                        Ok(crate::stdlib::make_set_instance(res))
                    }
                    (Value::Map(m), "execute")
                        if m.read().unwrap().contains_key("_sqlite_handle") =>
                    {
                        let handle = match m.read().unwrap().get("_sqlite_handle") {
                            Some(Value::Int(h)) => *h as u64,
                            _ => {
                                self.depth -= 1;
                                return Err(
                                    RuntimeError::new("Invalid SQLite handle".into()).at(span)
                                );
                            }
                        };
                        if args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "execute(sql, [params]) expects at least 1 argument".into(),
                            )
                            .at(span));
                        }
                        let sql = match &args[0] {
                            Value::String(s) => s.as_str(),
                            other => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!(
                                    "execute expects SQL string, got {}",
                                    other.type_name()
                                ))
                                .at(span));
                            }
                        };
                        let params: Vec<Value> = if args.len() > 1 {
                            match &args[1] {
                                Value::Array(arr) => arr.read().unwrap().clone(),
                                other => vec![other.clone()],
                            }
                        } else {
                            Vec::new()
                        };
                        let changes =
                            crate::sqlite::sqlite_execute(handle, sql, &params).map_err(|e| {
                                self.depth -= 1;
                                RuntimeError::new(e).at(span)
                            })?;
                        Ok(Value::Int(changes))
                    }
                    (Value::Map(m), "query")
                        if m.read().unwrap().contains_key("_sqlite_handle") =>
                    {
                        let handle = match m.read().unwrap().get("_sqlite_handle") {
                            Some(Value::Int(h)) => *h as u64,
                            _ => {
                                self.depth -= 1;
                                return Err(
                                    RuntimeError::new("Invalid SQLite handle".into()).at(span)
                                );
                            }
                        };
                        if args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "query(sql, [params]) expects at least 1 argument".into(),
                            )
                            .at(span));
                        }
                        let sql = match &args[0] {
                            Value::String(s) => s.as_str(),
                            other => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!(
                                    "query expects SQL string, got {}",
                                    other.type_name()
                                ))
                                .at(span));
                            }
                        };
                        let params: Vec<Value> = if args.len() > 1 {
                            match &args[1] {
                                Value::Array(arr) => arr.read().unwrap().clone(),
                                other => vec![other.clone()],
                            }
                        } else {
                            Vec::new()
                        };
                        crate::sqlite::sqlite_query(handle, sql, &params).map_err(|e| {
                            self.depth -= 1;
                            RuntimeError::new(e).at(span)
                        })
                    }
                    (Value::Map(m), "last_insert_id")
                        if m.read().unwrap().contains_key("_sqlite_handle") =>
                    {
                        let handle = match m.read().unwrap().get("_sqlite_handle") {
                            Some(Value::Int(h)) => *h as u64,
                            _ => {
                                self.depth -= 1;
                                return Err(
                                    RuntimeError::new("Invalid SQLite handle".into()).at(span)
                                );
                            }
                        };
                        let id = crate::sqlite::sqlite_last_insert_id(handle).map_err(|e| {
                            self.depth -= 1;
                            RuntimeError::new(e).at(span)
                        })?;
                        Ok(Value::Int(id))
                    }
                    (Value::Map(m), "changes")
                        if m.read().unwrap().contains_key("_sqlite_handle") =>
                    {
                        let handle = match m.read().unwrap().get("_sqlite_handle") {
                            Some(Value::Int(h)) => *h as u64,
                            _ => {
                                self.depth -= 1;
                                return Err(
                                    RuntimeError::new("Invalid SQLite handle".into()).at(span)
                                );
                            }
                        };
                        let count = crate::sqlite::sqlite_changes(handle).map_err(|e| {
                            self.depth -= 1;
                            RuntimeError::new(e).at(span)
                        })?;
                        Ok(Value::Int(count))
                    }
                    (Value::Map(m), "close")
                        if m.read().unwrap().contains_key("_sqlite_handle") =>
                    {
                        let handle = match m.read().unwrap().get("_sqlite_handle") {
                            Some(Value::Int(h)) => *h as u64,
                            _ => {
                                self.depth -= 1;
                                return Err(
                                    RuntimeError::new("Invalid SQLite handle".into()).at(span)
                                );
                            }
                        };
                        crate::sqlite::sqlite_close(handle).map_err(|e| {
                            self.depth -= 1;
                            RuntimeError::new(e).at(span)
                        })?;
                        Ok(Value::Null)
                    }
                    (Value::FfiLibrary(lib), "fn" | "symbol" | "get") => {
                        if args.len() != 3 {
                            self.depth -= 1;
                            return Err(RuntimeError::new(
                                "lib.fn(name, param_types, return_type) expects 3 arguments".into(),
                            )
                            .at(span));
                        }
                        let sym_name = match &args[0] {
                            Value::String(s) => s.clone(),
                            other => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!(
                                    "Expected symbol name string, got {}",
                                    other.type_name()
                                ))
                                .at(span));
                            }
                        };
                        let param_types = match &args[1] {
                            Value::Array(arr) => {
                                let items = arr.read().unwrap();
                                let mut types = Vec::new();
                                for item in items.iter() {
                                    match item {
                                        Value::String(t) => {
                                            let parsed =
                                                t.parse::<crate::ffi::FfiType>().map_err(|e| {
                                                    self.depth -= 1;
                                                    RuntimeError::new(e).at(span)
                                                })?;
                                            types.push(parsed);
                                        }
                                        other => {
                                            self.depth -= 1;
                                            return Err(RuntimeError::new(format!(
                                                "Parameter type must be a string, got {}",
                                                other.type_name()
                                            ))
                                            .at(span));
                                        }
                                    }
                                }
                                types
                            }
                            other => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!(
                                    "Parameter types must be an array, got {}",
                                    other.type_name()
                                ))
                                .at(span));
                            }
                        };
                        let ret_type = match &args[2] {
                            Value::String(s) => s.parse::<crate::ffi::FfiType>().map_err(|e| {
                                self.depth -= 1;
                                RuntimeError::new(e).at(span)
                            })?,
                            other => {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!(
                                    "Return type must be a string, got {}",
                                    other.type_name()
                                ))
                                .at(span));
                            }
                        };
                        let dynamic_fn =
                            lib.get_symbol(&sym_name, param_types, ret_type)
                                .map_err(|e| {
                                    self.depth -= 1;
                                    RuntimeError::new(e).at(span)
                                })?;
                        Ok(Value::FfiFunction(Arc::new(dynamic_fn)))
                    }
                    (Value::FfiLibrary(_), "close") => Ok(Value::Null),

                    // -------------------------------------------------------------
                    // Actor bound methods
                    // -------------------------------------------------------------
                    (Value::Actor(act), "send") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new("send() expects 1 argument".into()).at(span))
                        } else {
                            act.send(args[0].clone())
                                .map(|()| Value::Null)
                                .map_err(|e| e.to_runtime_error(span))
                        }
                    }
                    (Value::Actor(act), "ask") => {
                        let timeout = if args.len() == 1 {
                            Ok(None)
                        } else if args.len() == 2 {
                            match args[1] {
                                Value::Int(ms) if ms >= 0 => {
                                    Ok(Some(std::time::Duration::from_millis(ms as u64)))
                                }
                                Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => {
                                    Ok(Some(std::time::Duration::from_millis(ms as u64)))
                                }
                                Value::Null => Ok(None),
                                _ => Err(RuntimeError::new(
                                    "ask() timeout must be a non-negative number or null".into(),
                                )
                                .at(span)),
                            }
                        } else {
                            Err(RuntimeError::new(
                                "ask() expects 1 or 2 arguments: ask(msg, [timeout_ms])".into(),
                            )
                            .at(span))
                        };
                        match timeout {
                            Ok(to) => act
                                .ask(args[0].clone(), to)
                                .map_err(|e| e.to_runtime_error(span)),
                            Err(e) => Err(e),
                        }
                    }
                    (Value::Actor(act), "stop") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("stop() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Bool(act.stop()))
                        }
                    }
                    (Value::Actor(act), "is_alive" | "isAlive") => {
                        if !args.is_empty() {
                            Err(
                                RuntimeError::new(format!("{}() expects 0 arguments", method))
                                    .at(span),
                            )
                        } else {
                            Ok(Value::Bool(act.is_alive()))
                        }
                    }
                    (Value::Actor(act), "len" | "mailbox_len" | "mailboxLen") => {
                        if !args.is_empty() {
                            Err(
                                RuntimeError::new(format!("{}() expects 0 arguments", method))
                                    .at(span),
                            )
                        } else {
                            Ok(Value::Int(act.mailbox_len() as i64))
                        }
                    }
                    (Value::Actor(act), "id") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("id() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(act.id().to_string()))
                        }
                    }
                    (Value::Actor(act), "status") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("status() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(act.status().as_str().to_string()))
                        }
                    }

                    // -------------------------------------------------------------
                    // RemoteActor bound methods
                    // -------------------------------------------------------------
                    (Value::RemoteActor(rem), "send") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new("send() expects 1 argument".into()).at(span))
                        } else {
                            rem.send(args[0].clone())
                                .map(|()| Value::Null)
                                .map_err(|e| e.to_runtime_error(span))
                        }
                    }
                    (Value::RemoteActor(rem), "ask") => {
                        let timeout = if args.len() == 1 {
                            None
                        } else if args.len() == 2 {
                            match args[1] {
                                Value::Int(ms) if ms >= 0 => {
                                    Some(std::time::Duration::from_millis(ms as u64))
                                }
                                Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => {
                                    Some(std::time::Duration::from_millis(ms as u64))
                                }
                                Value::Null => None,
                                _ => {
                                    return Err(RuntimeError::new(
                                        "ask() timeout must be a non-negative number".into(),
                                    )
                                    .at(span));
                                }
                            }
                        } else {
                            return Err(RuntimeError::new(
                                "ask() expects 1 or 2 arguments: ask(msg, [timeout_ms])".into(),
                            )
                            .at(span));
                        };

                        rem.ask(args[0].clone(), timeout)
                            .map_err(|e| e.to_runtime_error(span))
                    }
                    (Value::RemoteActor(rem), "is_alive" | "isAlive") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("is_alive() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Bool(rem.is_alive()))
                        }
                    }
                    (Value::RemoteActor(rem), "node") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("node() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(rem.node().to_string()))
                        }
                    }
                    (Value::RemoteActor(rem), "name") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("name() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(rem.name().to_string()))
                        }
                    }

                    // -------------------------------------------------------------
                    // ClusterNode bound methods
                    // -------------------------------------------------------------
                    (Value::ClusterNode(node), "connect" | "join") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "connect() expects 1 argument (peer_address)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::String(s) => node
                                    .connect(s)
                                    .map(|_| Value::Bool(true))
                                    .map_err(|e| e.to_runtime_error(span)),
                                other => Err(RuntimeError::new(format!(
                                    "connect() expects string peer_address, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::ClusterNode(node), "peers" | "nodes") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("peers() expects 0 arguments".into()).at(span))
                        } else {
                            let peers = node.peers();
                            let values: Vec<Value> = peers.into_iter().map(Value::String).collect();
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                values,
                            ))))
                        }
                    }
                    (Value::ClusterNode(node), "actor") => {
                        if args.len() != 2 {
                            Err(RuntimeError::new(
                                "actor() expects 2 arguments: actor(name, peer_node)".into(),
                            )
                            .at(span))
                        } else {
                            let name = match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "actor() expects string name, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            let peer = match &args[1] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "actor() expects string peer_node, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            Ok(Value::RemoteActor(node.actor(&name, &peer)))
                        }
                    }
                    (Value::ClusterNode(node), "whereis") => {
                        if args.is_empty() || args.len() > 2 {
                            Err(RuntimeError::new(
                                "whereis() expects 1 or 2 arguments: whereis(name, [node_hint])"
                                    .into(),
                            )
                            .at(span))
                        } else {
                            let name = match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "whereis() expects string name, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            let hint = match args.get(1) {
                                Some(Value::String(s)) => Some(s.clone()),
                                _ => None,
                            };
                            Ok(node.whereis(&name, hint.as_deref()).unwrap_or(Value::Null))
                        }
                    }
                    (Value::ClusterNode(node), "spawn") => {
                        if args.len() == 2 {
                            let name = match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "spawn() expects string name, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            let act = node
                                .spawn_actor(&name, None, args[1].clone())
                                .map_err(|e| e.to_runtime_error(span))?;
                            Ok(Value::Actor(act))
                        } else if args.len() == 3 {
                            let name = match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "spawn() expects string name, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            let act = node
                                .spawn_actor(&name, Some(args[1].clone()), args[2].clone())
                                .map_err(|e| e.to_runtime_error(span))?;
                            Ok(Value::Actor(act))
                        } else {
                            Err(RuntimeError::new(
                                "spawn() expects 2 or 3 arguments: spawn(name, [state], handler)"
                                    .into(),
                            )
                            .at(span))
                        }
                    }
                    (Value::ClusterNode(node), "register") => {
                        if args.len() != 2 {
                            Err(RuntimeError::new(
                                "register() expects 2 arguments: register(name, actor)".into(),
                            )
                            .at(span))
                        } else {
                            let name = match &args[0] {
                                Value::String(s) => s.clone(),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "register() expects string name, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            let act = match &args[1] {
                                Value::Actor(a) => std::sync::Arc::clone(a),
                                other => {
                                    return Err(RuntimeError::new(format!(
                                        "register() expects actor handle, got {}",
                                        other.type_name()
                                    ))
                                    .at(span));
                                }
                            };
                            node.register(&name, act);
                            Ok(Value::Null)
                        }
                    }
                    (Value::ClusterNode(node), "unregister") => {
                        if args.len() != 1 {
                            Err(
                                RuntimeError::new("unregister() expects 1 argument (name)".into())
                                    .at(span),
                            )
                        } else {
                            match &args[0] {
                                Value::String(s) => {
                                    node.unregister(s);
                                    Ok(Value::Null)
                                }
                                other => Err(RuntimeError::new(format!(
                                    "unregister() expects string name, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::ClusterNode(node), "registered") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("registered() expects 0 arguments".into())
                                .at(span))
                        } else {
                            let list = node.registered();
                            let values: Vec<Value> = list.into_iter().map(Value::String).collect();
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                values,
                            ))))
                        }
                    }
                    (Value::ClusterNode(node), "send") => {
                        if args.len() != 2 {
                            Err(RuntimeError::new(
                                "send() expects 2 arguments: send(target, msg)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::RemoteActor(rem) => rem
                                    .send(args[1].clone())
                                    .map(|()| Value::Null)
                                    .map_err(|e| e.to_runtime_error(span)),
                                Value::Actor(act) => act
                                    .send(args[1].clone())
                                    .map(|()| Value::Null)
                                    .map_err(|e| e.to_runtime_error(span)),
                                Value::String(s) => {
                                    if s.contains('@') {
                                        let parts: Vec<&str> = s.splitn(2, '@').collect();
                                        node.send_to_remote(parts[1], parts[0], args[1].clone())
                                            .map(|()| Value::Null)
                                            .map_err(|e| e.to_runtime_error(span))
                                    } else if let Some(Value::RemoteActor(rem)) =
                                        node.whereis(s, None)
                                    {
                                        rem.send(args[1].clone())
                                            .map(|()| Value::Null)
                                            .map_err(|e| e.to_runtime_error(span))
                                    } else if let Some(act) =
                                        crate::actor::ActorRegistry::whereis(s)
                                    {
                                        act.send(args[1].clone())
                                            .map(|()| Value::Null)
                                            .map_err(|e| e.to_runtime_error(span))
                                    } else {
                                        Err(crate::actor::ActorError::NameNotFound(s.clone())
                                            .to_runtime_error(span))
                                    }
                                }
                                other => Err(RuntimeError::new(format!(
                                    "send() target must be an actor or string, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::ClusterNode(node), "ask") => {
                        if args.len() < 2 || args.len() > 3 {
                            Err(RuntimeError::new(
                                "ask() expects 2 or 3 arguments: ask(target, msg, [timeout_ms])"
                                    .into(),
                            )
                            .at(span))
                        } else {
                            let timeout = if args.len() == 3 {
                                match args[2] {
                                    Value::Int(ms) if ms >= 0 => {
                                        Some(std::time::Duration::from_millis(ms as u64))
                                    }
                                    Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => {
                                        Some(std::time::Duration::from_millis(ms as u64))
                                    }
                                    Value::Null => None,
                                    _ => {
                                        return Err(RuntimeError::new(
                                            "ask() timeout must be a non-negative number".into(),
                                        )
                                        .at(span));
                                    }
                                }
                            } else {
                                None
                            };

                            match &args[0] {
                                Value::RemoteActor(rem) => rem
                                    .ask(args[1].clone(), timeout)
                                    .map_err(|e| e.to_runtime_error(span)),
                                Value::Actor(act) => act
                                    .ask(args[1].clone(), timeout)
                                    .map_err(|e| e.to_runtime_error(span)),
                                Value::String(s) => {
                                    if s.contains('@') {
                                        let parts: Vec<&str> = s.splitn(2, '@').collect();
                                        node.ask_remote(
                                            parts[1],
                                            parts[0],
                                            args[1].clone(),
                                            timeout,
                                        )
                                        .map_err(|e| e.to_runtime_error(span))
                                    } else if let Some(Value::RemoteActor(rem)) =
                                        node.whereis(s, None)
                                    {
                                        rem.ask(args[1].clone(), timeout)
                                            .map_err(|e| e.to_runtime_error(span))
                                    } else if let Some(act) =
                                        crate::actor::ActorRegistry::whereis(s)
                                    {
                                        act.ask(args[1].clone(), timeout)
                                            .map_err(|e| e.to_runtime_error(span))
                                    } else {
                                        Err(crate::actor::ActorError::NameNotFound(s.clone())
                                            .to_runtime_error(span))
                                    }
                                }
                                other => Err(RuntimeError::new(format!(
                                    "ask() target must be an actor or string, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::ClusterNode(node), "broadcast") => {
                        if args.len() != 2 {
                            Err(RuntimeError::new(
                                "broadcast() expects 2 arguments: broadcast(topic, msg)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::String(topic) => {
                                    node.broadcast(topic, args[1].clone());
                                    Ok(Value::Null)
                                }
                                other => Err(RuntimeError::new(format!(
                                    "broadcast() expects string topic, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::ClusterNode(node), "on_broadcast") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "on_broadcast() expects 1 argument (callback)".into(),
                            )
                            .at(span))
                        } else {
                            node.on_broadcast(args[0].clone());
                            Ok(Value::Null)
                        }
                    }
                    (Value::ClusterNode(node), "stop") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("stop() expects 0 arguments".into()).at(span))
                        } else {
                            node.stop();
                            Ok(Value::Bool(true))
                        }
                    }
                    (Value::ClusterNode(node), "is_alive" | "isAlive") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("is_alive() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Bool(node.is_alive()))
                        }
                    }
                    (Value::ClusterNode(node), "name") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("name() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(node.name().to_string()))
                        }
                    }
                    (Value::ClusterNode(node), "address") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("address() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(node.address()))
                        }
                    }
                    (Value::ClusterNode(node), "is_tls" | "isTls") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("is_tls() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Bool(node.is_tls()))
                        }
                    }

                    // -------------------------------------------------------------
                    // Supervisor bound methods
                    // -------------------------------------------------------------
                    (Value::Supervisor(sup), "get_child" | "getChild") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "get_child() expects 1 argument (child_id)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::String(s) => match sup.get_child(s) {
                                    Some(a) => Ok(Value::Actor(a)),
                                    None => Ok(Value::Null),
                                },
                                other => Err(RuntimeError::new(format!(
                                    "get_child() expects string child_id, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::Supervisor(sup), "start_child" | "startChild") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "start_child() expects 1 argument (child_spec map)".into(),
                            )
                            .at(span))
                        } else {
                            match crate::stdlib::parse_shae_child_spec(&args[0], self, span) {
                                Ok(child_spec) => sup
                                    .start_child(child_spec)
                                    .map(Value::Actor)
                                    .map_err(|e| e.to_runtime_error(span)),
                                Err(e) => Err(e),
                            }
                        }
                    }
                    (Value::Supervisor(sup), "stop_child" | "stopChild") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "stop_child() expects 1 argument (child_id)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::String(s) => sup
                                    .stop_child(s)
                                    .map(|()| Value::Null)
                                    .map_err(|e| e.to_runtime_error(span)),
                                other => Err(RuntimeError::new(format!(
                                    "stop_child() expects string child_id, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::Supervisor(sup), "restart_child" | "restartChild") => {
                        if args.len() != 1 {
                            Err(RuntimeError::new(
                                "restart_child() expects 1 argument (child_id)".into(),
                            )
                            .at(span))
                        } else {
                            match &args[0] {
                                Value::String(s) => sup
                                    .restart_child(s)
                                    .map(Value::Actor)
                                    .map_err(|e| e.to_runtime_error(span)),
                                other => Err(RuntimeError::new(format!(
                                    "restart_child() expects string child_id, got {}",
                                    other.type_name()
                                ))
                                .at(span)),
                            }
                        }
                    }
                    (Value::Supervisor(sup), "children") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("children() expects 0 arguments".into()).at(span))
                        } else {
                            let children_info = sup.children();
                            let mut list = Vec::with_capacity(children_info.len());
                            for (id, status) in children_info {
                                let mut child_map = indexmap::IndexMap::new();
                                child_map.insert("id".to_string(), Value::String(id.clone()));
                                child_map.insert(
                                    "status".to_string(),
                                    Value::String(status.as_str().to_string()),
                                );
                                if let Some(actor) = sup.get_child(&id) {
                                    child_map.insert("actor".to_string(), Value::Actor(actor));
                                }
                                list.push(Value::Map(std::sync::Arc::new(std::sync::RwLock::new(
                                    child_map,
                                ))));
                            }
                            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                                list,
                            ))))
                        }
                    }
                    (Value::Supervisor(sup), "stop") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("stop() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Bool(sup.stop()))
                        }
                    }
                    (Value::Supervisor(sup), "len") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("len() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::Int(sup.len() as i64))
                        }
                    }
                    (Value::Supervisor(sup), "id") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("id() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(sup.id().to_string()))
                        }
                    }
                    (Value::Supervisor(sup), "status") => {
                        if !args.is_empty() {
                            Err(RuntimeError::new("status() expects 0 arguments".into()).at(span))
                        } else {
                            Ok(Value::String(sup.status().to_string()))
                        }
                    }
                    (Value::Supervisor(sup), "is_alive" | "isAlive") => {
                        if !args.is_empty() {
                            Err(
                                RuntimeError::new(format!("{}() expects 0 arguments", method))
                                    .at(span),
                            )
                        } else {
                            Ok(Value::Bool(sup.is_alive()))
                        }
                    }
                    (Value::ByteBuffer(b), _) => {
                        eval_buffer_bound_method(&b, &method, args, span)
                    }
                    _ => Err(RuntimeError::new(format!("Method {} not found", method)).at(span)),
                }
            }

            Value::FfiFunction(func) => func.invoke(args, span),

            other => Err(RuntimeError::new(format!("Cannot call {}", other.type_name())).at(span)),
        };
        self.depth -= 1;
        match res {
            Ok(v) => Ok(v),
            Err(mut e) => {
                let func_name = match callee {
                    Value::Function { name, .. } => {
                        name.clone().unwrap_or_else(|| "anonymous".to_string())
                    }
                    Value::Builtin { name, .. } => name.clone(),
                    Value::BoundMethod { method, .. } => method.clone(),
                    Value::EnumConstructor { variant_name, .. } => variant_name.clone(),
                    Value::FfiFunction(func) => func.name.clone(),
                    _ => "unknown".to_string(),
                };
                e.stack.push((func_name, span));
                Err(e)
            }
        }
    }

    fn match_pattern(
        &self,
        pattern: &crate::ast::Pattern,
        val: &Value,
    ) -> Option<Vec<(String, Value)>> {
        use crate::ast::{Literal, Pattern};
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
            Pattern::Range {
                start,
                end,
                inclusive,
            } => match (val, start, end) {
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
            },
            Pattern::Enum {
                enum_name,
                variant_name,
                fields,
            } => {
                if let Value::EnumInstance {
                    enum_name: v_enum,
                    variant_name: v_variant,
                    values,
                } = val
                {
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
                            let field_val =
                                map_guard.get(field_name).cloned().unwrap_or(Value::Null);
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
                    Value::StructInstance {
                        fields: struct_fields,
                        ..
                    } => {
                        let fields_guard = struct_fields.read().unwrap();
                        for (field_name, opt_sub) in fields {
                            extracted_keys.insert(field_name.clone());
                            let field_val =
                                fields_guard.get(field_name).cloned().unwrap_or(Value::Null);
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


fn eval_buffer_bound_method(
    b: &std::sync::Arc<std::sync::RwLock<crate::buffer::ByteBuffer>>,
    method: &str,
    args: Vec<Value>,
    span: crate::ast::Span,
) -> Result<Value, RuntimeError> {
    match method {
        "slice" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "slice() expects 1 or 2 arguments (start, [end])".into(),
                )
                .at(span));
            }
            let start = match &args[0] {
                Value::Int(n) if *n >= 0 => *n as usize,
                Value::Float(f) | Value::Number(f) if *f >= 0.0 && f.fract() == 0.0 => *f as usize,
                _ => {
                    return Err(RuntimeError::new(
                        "slice() start must be a non-negative integer".into(),
                    )
                    .at(span));
                }
            };
            let end = if args.len() == 2 {
                match &args[1] {
                    Value::Int(n) if *n >= 0 => Some(*n as usize),
                    Value::Float(f) | Value::Number(f) if *f >= 0.0 && f.fract() == 0.0 => {
                        Some(*f as usize)
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "slice() end must be a non-negative integer".into(),
                        )
                        .at(span));
                    }
                }
            } else {
                None
            };
            let sliced = b
                .read()
                .unwrap()
                .slice(start, end)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::ByteBuffer(std::sync::Arc::new(
                std::sync::RwLock::new(sliced),
            )))
        }
        "copy" => {
            if !args.is_empty() {
                return Err(RuntimeError::new("copy() expects 0 arguments".into()).at(span));
            }
            let copied = b.read().unwrap().copy();
            Ok(Value::ByteBuffer(std::sync::Arc::new(
                std::sync::RwLock::new(copied),
            )))
        }
        "to_hex" | "toHex" => {
            if !args.is_empty() {
                return Err(RuntimeError::new("to_hex() expects 0 arguments".into()).at(span));
            }
            Ok(Value::String(b.read().unwrap().to_hex()))
        }
        "to_utf8" | "toUtf8" => {
            if !args.is_empty() {
                return Err(RuntimeError::new("to_utf8() expects 0 arguments".into()).at(span));
            }
            let s = b
                .read()
                .unwrap()
                .to_utf8()
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::String(s))
        }
        "to_bytes" | "toBytes" => {
            if !args.is_empty() {
                return Err(RuntimeError::new("to_bytes() expects 0 arguments".into()).at(span));
            }
            let bytes = b.read().unwrap().to_bytes();
            let arr: Vec<Value> = bytes
                .into_iter()
                .map(|byte| Value::Int(byte as i64))
                .collect();
            Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
                arr,
            ))))
        }
        // Endian reads (1 argument: pos)
        "read_u8" | "readU8" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u8(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i8" | "readI8" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i8(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u16_le" | "readU16LE" | "read_u16LE" | "readU16Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u16_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u16_be" | "readU16BE" | "read_u16BE" | "readU16Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u16_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i16_le" | "readI16LE" | "read_i16LE" | "readI16Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i16_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i16_be" | "readI16BE" | "read_i16BE" | "readI16Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i16_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u32_le" | "readU32LE" | "read_u32LE" | "readU32Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u32_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u32_be" | "readU32BE" | "read_u32BE" | "readU32Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u32_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i32_le" | "readI32LE" | "read_i32LE" | "readI32Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i32_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i32_be" | "readI32BE" | "read_i32BE" | "readI32Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i32_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u64_le" | "readU64LE" | "read_u64LE" | "readU64Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u64_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_u64_be" | "readU64BE" | "read_u64BE" | "readU64Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_u64_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v as i64))
        }
        "read_i64_le" | "readI64LE" | "read_i64LE" | "readI64Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i64_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v))
        }
        "read_i64_be" | "readI64BE" | "read_i64BE" | "readI64Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_i64_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(v))
        }
        "read_f32_le" | "readF32LE" | "read_f32LE" | "readF32Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_f32_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Float(v as f64))
        }
        "read_f32_be" | "readF32BE" | "read_f32BE" | "readF32Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_f32_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Float(v as f64))
        }
        "read_f64_le" | "readF64LE" | "read_f64LE" | "readF64Le" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_f64_le(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Float(v))
        }
        "read_f64_be" | "readF64BE" | "read_f64BE" | "readF64Be" => {
            let pos = get_read_pos(method, &args, span)?;
            let v = b
                .read()
                .unwrap()
                .read_f64_be(pos)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Float(v))
        }
        // Endian writes (2 arguments: pos, val)
        "write_u8" | "writeU8" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u8(pos, val as u8)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(1))
        }
        "write_i8" | "writeI8" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i8(pos, val as i8)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(1))
        }
        "write_u16_le" | "writeU16LE" | "write_u16LE" | "writeU16Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u16_le(pos, val as u16)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(2))
        }
        "write_u16_be" | "writeU16BE" | "write_u16BE" | "writeU16Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u16_be(pos, val as u16)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(2))
        }
        "write_i16_le" | "writeI16LE" | "write_i16LE" | "writeI16Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i16_le(pos, val as i16)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(2))
        }
        "write_i16_be" | "writeI16BE" | "write_i16BE" | "writeI16Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i16_be(pos, val as i16)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(2))
        }
        "write_u32_le" | "writeU32LE" | "write_u32LE" | "writeU32Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u32_le(pos, val as u32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_u32_be" | "writeU32BE" | "write_u32BE" | "writeU32Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u32_be(pos, val as u32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_i32_le" | "writeI32LE" | "write_i32LE" | "writeI32Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i32_le(pos, val as i32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_i32_be" | "writeI32BE" | "write_i32BE" | "writeI32Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i32_be(pos, val as i32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_u64_le" | "writeU64LE" | "write_u64LE" | "writeU64Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u64_le(pos, val as u64)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        "write_u64_be" | "writeU64BE" | "write_u64BE" | "writeU64Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_u64_be(pos, val as u64)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        "write_i64_le" | "writeI64LE" | "write_i64LE" | "writeI64Le" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i64_le(pos, val)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        "write_i64_be" | "writeI64BE" | "write_i64BE" | "writeI64Be" => {
            let (pos, val) = get_write_pos_int(method, &args, span)?;
            b.write()
                .unwrap()
                .write_i64_be(pos, val)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        "write_f32_le" | "writeF32LE" | "write_f32LE" | "writeF32Le" => {
            let (pos, val) = get_write_pos_float(method, &args, span)?;
            b.write()
                .unwrap()
                .write_f32_le(pos, val as f32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_f32_be" | "writeF32BE" | "write_f32BE" | "writeF32Be" => {
            let (pos, val) = get_write_pos_float(method, &args, span)?;
            b.write()
                .unwrap()
                .write_f32_be(pos, val as f32)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(4))
        }
        "write_f64_le" | "writeF64LE" | "write_f64LE" | "writeF64Le" => {
            let (pos, val) = get_write_pos_float(method, &args, span)?;
            b.write()
                .unwrap()
                .write_f64_le(pos, val)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        "write_f64_be" | "writeF64BE" | "write_f64BE" | "writeF64Be" => {
            let (pos, val) = get_write_pos_float(method, &args, span)?;
            b.write()
                .unwrap()
                .write_f64_be(pos, val)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::Int(8))
        }
        _ => Err(RuntimeError::new(format!(
            "Method {} not found on ByteBuffer",
            method
        ))
        .at(span)),
    }
}

fn get_read_pos(
    method: &str,
    args: &[Value],
    span: crate::ast::Span,
) -> Result<usize, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::new(format!(
            "{}() expects 1 argument (offset)",
            method
        ))
        .at(span));
    }
    match &args[0] {
        Value::Int(n) if *n >= 0 => Ok(*n as usize),
        Value::Float(f) | Value::Number(f) if *f >= 0.0 && f.fract() == 0.0 => Ok(*f as usize),
        Value::Int(n) if *n < 0 => Err(RuntimeError::new(format!(
            "Offset cannot be negative, got {}",
            n
        ))
        .at(span)),
        Value::Float(f) | Value::Number(f) if *f < 0.0 => Err(RuntimeError::new(format!(
            "Offset cannot be negative, got {}",
            f
        ))
        .at(span)),
        other => Err(RuntimeError::new(format!(
            "{}() offset must be an integer, got {}",
            method,
            other.type_name()
        ))
        .at(span)),
    }
}

fn get_write_pos_int(
    method: &str,
    args: &[Value],
    span: crate::ast::Span,
) -> Result<(usize, i64), RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new(format!(
            "{}() expects 2 arguments (offset, value)",
            method
        ))
        .at(span));
    }
    let pos = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Float(f) | Value::Number(f) if *f >= 0.0 && f.fract() == 0.0 => *f as usize,
        Value::Int(n) if *n < 0 => {
            return Err(RuntimeError::new(format!("Offset cannot be negative, got {}", n)).at(span));
        }
        Value::Float(f) | Value::Number(f) if *f < 0.0 => {
            return Err(RuntimeError::new(format!("Offset cannot be negative, got {}", f)).at(span));
        }
        other => {
            return Err(RuntimeError::new(format!(
                "{}() offset must be an integer, got {}",
                method,
                other.type_name()
            ))
            .at(span));
        }
    };
    let val = match &args[1] {
        Value::Int(i) => *i,
        Value::Float(f) | Value::Number(f) if f.fract() == 0.0 => *f as i64,
        other => {
            return Err(RuntimeError::new(format!(
                "{}() value must be an integer, got {}",
                method,
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok((pos, val))
}

fn get_write_pos_float(
    method: &str,
    args: &[Value],
    span: crate::ast::Span,
) -> Result<(usize, f64), RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new(format!(
            "{}() expects 2 arguments (offset, value)",
            method
        ))
        .at(span));
    }
    let pos = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Float(f) | Value::Number(f) if *f >= 0.0 && f.fract() == 0.0 => *f as usize,
        Value::Int(n) if *n < 0 => {
            return Err(RuntimeError::new(format!("Offset cannot be negative, got {}", n)).at(span));
        }
        Value::Float(f) | Value::Number(f) if *f < 0.0 => {
            return Err(RuntimeError::new(format!("Offset cannot be negative, got {}", f)).at(span));
        }
        other => {
            return Err(RuntimeError::new(format!(
                "{}() offset must be an integer, got {}",
                method,
                other.type_name()
            ))
            .at(span));
        }
    };
    let val = match &args[1] {
        Value::Float(f) | Value::Number(f) => *f,
        Value::Int(i) => *i as f64,
        other => {
            return Err(RuntimeError::new(format!(
                "{}() value must be a number, got {}",
                method,
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok((pos, val))
}
