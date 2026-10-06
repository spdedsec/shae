use crate::ast::{BinaryOp, Expr, InterpPart, Literal, Program, Span, Stmt, StmtKind, UnaryOp};
use crate::env::Environment;
use crate::value::{IndexError, Value, resolve_index};
use std::cell::RefCell;
use std::rc::Rc;
use thiserror::Error;

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
    pub global_env: Rc<RefCell<Environment>>,
    max_depth: usize,
    depth: usize,
}

impl Evaluator {
    pub fn new() -> Self {
        let mut env = Environment::new();
        crate::builtins::register(&mut env);
        Self {
            global_env: Rc::new(RefCell::new(env)),
            max_depth: 2000,
            depth: 0,
        }
    }

    pub fn set_max_depth(&mut self, limit: usize) {
        self.max_depth = limit;
    }

    pub fn with_env(env: Rc<RefCell<Environment>>) -> Self {
        Self {
            global_env: env,
            max_depth: 2000,
            depth: 0,
        }
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
        env: &Rc<RefCell<Environment>>,
    ) -> Result<Signal, RuntimeError> {
        match &stmt.kind {
StmtKind::StructDef { name, fields } => {
                env.borrow_mut().define(name.clone(), Value::StructDef {
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
                env.borrow_mut().define(name.clone(), Value::EnumDef {
                    name: name.clone(),
                    variants: std::rc::Rc::new(var_map),
                });
                Ok(Signal::None)
            }
            StmtKind::Expr(expr) => {
                let val = self.eval_expr(expr, env).map_err(|e| e.or_at(stmt.span))?;
                Ok(Signal::Value(val))
            }
            StmtKind::Let { name, init } => {
                let val = self.eval_expr(init, env).map_err(|e| e.or_at(stmt.span))?;
                env.borrow_mut().define(name.clone(), val);
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
                env.borrow_mut().define(name.clone(), func);
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
                let try_env = std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new_with_parent(env.clone())));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new_with_parent(env.clone())));
                        catch_env.borrow_mut().define(catch_ident.clone(), Value::String(e.message));
                        self.eval_block(catch_body, &catch_env)
                    }
                }
            }
            StmtKind::While { condition, body } => {
                let mut last_val = Value::Null;
                loop {
                    let cond_val = self
                        .eval_expr(condition, env)
                        .map_err(|e| e.or_at(stmt.span))?;
                    if !cond_val.is_truthy() {
                        break;
                    }
                    match self.eval_block(body, env)? {
                        Signal::Value(v) => last_val = v,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                        Signal::None => last_val = Value::Null,
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
                    Value::Array(a) => a.borrow().clone(),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "Cannot iterate over {}",
                            other.type_name()
                        ))
                        .at(stmt.span));
                    }
                };
                let mut last_val = Value::Null;
                for elem in elements {
                    let loop_env = Rc::new(RefCell::new(Environment::new_with_parent(env.clone())));
                    loop_env.borrow_mut().define(item.clone(), elem);
                    match self.eval_block(body, &loop_env)? {
                        Signal::Value(v) => last_val = v,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                        Signal::None => last_val = Value::Null,
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
        }
    }

    fn eval_block(
        &mut self,
        block: &[Stmt],
        env: &Rc<RefCell<Environment>>,
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
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), RuntimeError> {
        match target {
            Expr::Variable { name, span } => {
                if !env.borrow_mut().set(name, value.clone()) {
                    let candidates = env.borrow().all_names();
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
                    m.borrow_mut().insert(property.clone(), value);
                    Ok(())
                } else if let Value::StructInstance { name, fields } = base_val {
                    if fields.borrow().contains_key(property) {
                        fields.borrow_mut().insert(property.clone(), value);
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
                        if let Value::Number(n) = idx_val {
                            let mut arr = a.borrow_mut();
                            let len = arr.len();
                            match resolve_index(n, len) {
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
                                    n, len
                                ))
                                .at(*span)),
                            }
                        } else {
                            Err(RuntimeError::new("Array index must be a number".into()).at(*span))
                        }
                    }
                    Value::Map(m) => {
                        if let Value::String(s) = idx_val {
                            m.borrow_mut().insert(s, value);
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
        env: &Rc<RefCell<Environment>>,
    ) -> Result<Value, RuntimeError> {
        let _span = expr.span();
        match expr {
            Expr::Literal(Literal::Null) => Ok(Value::Null),
            Expr::Literal(Literal::Bool(b)) => Ok(Value::Bool(*b)),
            Expr::Literal(Literal::Number(n)) => Ok(Value::Number(*n)),
            Expr::Literal(Literal::String(s)) => Ok(Value::String(s.clone())),
            Expr::Variable { name, span } => {
                if let Some(v) = env.borrow().get(name) {
                    Ok(v)
                } else {
                    let candidates = env.borrow().all_names();
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
                Ok(Value::Array(Rc::new(RefCell::new(vals))))
            }
            Expr::Map(elements) => {
                let mut m = indexmap::IndexMap::new();
                for (k, v_expr) in elements {
                    let val = self.eval_expr(v_expr, env)?;
                    m.insert(k.clone(), val);
                }
                Ok(Value::Map(Rc::new(RefCell::new(m))))
            }
Expr::StructInit { name, fields, span } => {
                let def = env.borrow().get(name);
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
                        fields: std::rc::Rc::new(std::cell::RefCell::new(instance_fields)),
                    })
                } else {
                    Err(RuntimeError::new(format!("'{}' is not a struct.", name)).at(*span))
                }
            }
            Expr::Match { target, arms, span } => {
                let target_val = self.eval_expr(target, env)?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &target_val) {
                        let match_env = std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new_with_parent(env.clone())));
                        for (k, v) in bindings {
                            match_env.borrow_mut().define(k, v);
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
                    _ => return Err(RuntimeError::new("Module path must be a string".into()).at(*span)),
                };
                
                let source = std::fs::read_to_string(&path_str).map_err(|e| {
                    RuntimeError::new(format!("Failed to read module '{}': {}", path_str, e)).at(*span)
                })?;
                
                let mut builtin_env = crate::env::Environment::new();
                crate::builtins::register(&mut builtin_env);
                let builtin_env_rc = std::rc::Rc::new(std::cell::RefCell::new(builtin_env));
                let module_env = std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new_with_parent(builtin_env_rc)));
                
                let mut ev = crate::eval::Evaluator::with_env(module_env.clone());
                
                let tokens = crate::lexer::tokenize(&source).map_err(|e| RuntimeError::new(format!("Lexer error in module '{}': {}", path_str, e)).at(*span))?;
                let program = crate::parser::parse(tokens).map_err(|e| RuntimeError::new(format!("Parser error in module '{}': {}", path_str, e)).at(*span))?;
                ev.eval_program(&program).map_err(|e| RuntimeError::new(format!("Runtime error in module '{}': {}", path_str, e.message)).at(*span))?;
                
                let map = module_env.borrow().export_map();
                Ok(Value::Map(std::rc::Rc::new(std::cell::RefCell::new(map))))
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
                            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
                            (Value::Array(a), Value::Array(b)) => {
                                let mut new_arr = a.borrow().clone();
                                new_arr.extend(b.borrow().clone());
                                Ok(Value::Array(Rc::new(RefCell::new(new_arr))))
                            }
                            _ => Err(RuntimeError::new(format!(
                                "Cannot add {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).with_hint("For string concatenation, use string interpolation: `\"hi {name}\"` or convert explicitly: `str(x) + str(y)`").at(*span))
                        }
                    }
                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                        if let (Value::Number(a), Value::Number(b)) = (&left_val, &right_val) {
                            match op {
                                BinaryOp::Sub => Ok(Value::Number(a - b)),
                                BinaryOp::Mul => Ok(Value::Number(a * b)),
                                BinaryOp::Div => {
                                    if *b == 0.0 {
                                        Err(RuntimeError::new("Dividing by zero creates black holes".into()).at(*span))
                                    } else {
                                        Ok(Value::Number(a / b))
                                    }
                                }
                                BinaryOp::Mod => {
                                    if *b == 0.0 {
                                        Err(RuntimeError::new("Modulo by zero".into()).at(*span))
                                    } else {
                                        Ok(Value::Number(a % b))
                                    }
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            Err(RuntimeError::new(format!(
                                "Arithmetic operation requires numbers, got {} and {}",
                                left_val.type_name(),
                                right_val.type_name()
                            )).at(*span))
                        }
                    }
                    BinaryOp::Eq => Ok(Value::Bool(left_val == right_val)),
                    BinaryOp::NotEq => Ok(Value::Bool(left_val != right_val)),
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        match (&left_val, &right_val) {
                            (Value::Number(a), Value::Number(b)) => {
                                let res = match op {
                                    BinaryOp::Lt => a < b,
                                    BinaryOp::LtEq => a <= b,
                                    BinaryOp::Gt => a > b,
                                    BinaryOp::GtEq => a >= b,
                                    _ => unreachable!(),
                                };
                                Ok(Value::Bool(res))
                            }
                            (Value::String(a), Value::String(b)) => {
                                let res = match op {
                                    BinaryOp::Lt => a < b,
                                    BinaryOp::LtEq => a <= b,
                                    BinaryOp::Gt => a > b,
                                    BinaryOp::GtEq => a >= b,
                                    _ => unreachable!(),
                                };
                                Ok(Value::Bool(res))
                            }
                            _ => Err(RuntimeError::new(format!(
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
                        if let Value::Number(n) = inner {
                            Ok(Value::Number(-n))
                        } else {
                            Err(
                                RuntimeError::new(format!("Cannot negate {}", inner.type_name()))
                                    .at(*span),
                            )
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
        env: &Rc<RefCell<Environment>>,
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
                        if let Some(val) = fields.borrow().get(property) {
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
                        let map = m.borrow();
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
                            Ok(Some(Value::Number(a.borrow().len() as f64)))
                        } else if property == "first" {
                            let val = a.borrow().first().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'first' of an empty array".into()).at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "last" {
                            let val = a.borrow().last().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'last' of an empty array".into()).at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "push" || property == "pop" || property == "map" || property == "filter" || property == "reduce" || property == "sum" || property == "sort" {
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
                            Ok(Some(Value::Number(s.chars().count() as f64)))
                        } else if property == "trim" || property == "upper" || property == "lower" || property == "split" || property == "replace" {
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
                        if let Value::Number(n) = idx_val {
                            let len = a.borrow().len();
                            match resolve_index(n, len) {
                                Ok(i) => Ok(Some(a.borrow()[i].clone())),
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
                                            n, len
                                        ))
                                        .at(*span))
                                    }
                                }
                            }
                        } else {
                            Err(RuntimeError::new("Array index must be a number".into()).at(*span))
                        }
                    }
                    Value::Map(m) => {
                        if let Value::String(s) = idx_val {
                            if let Some(v) = m.borrow().get(&s) {
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
                        if let Value::Number(n) = idx_val {
                            let len = s.len();
                            match resolve_index(n, len) {
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
                                            n, len
                                        ))
                                        .at(*span))
                                    }
                                }
                            }
                        } else {
                            Err(RuntimeError::new("String index must be a number".into()).at(*span))
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
                let call_env = Rc::new(RefCell::new(Environment::new_with_parent(closure.clone())));
                for (param_name, arg_val) in params.iter().zip(args.into_iter()) {
                    call_env.borrow_mut().define(param_name.clone(), arg_val);
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
                        a.borrow_mut().push(args[0].clone());
                        Ok(Value::Null)
                    }
                    (Value::Array(a), "pop") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("pop() expects 0 arguments".into()).at(span));
                        }
                        if let Some(val) = a.borrow_mut().pop() {
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
                        for item in a.borrow().iter() {
                            let mapped = self.call_value(func, vec![item.clone()], span)?;
                            new_arr.push(mapped);
                        }
                        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(new_arr))))
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
                        for item in a.borrow().iter() {
                            let keep = self.call_value(func, vec![item.clone()], span)?;
                            if keep.is_truthy() {
                                new_arr.push(item.clone());
                            }
                        }
                        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(new_arr))))
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
                        for item in a.borrow().iter() {
                            acc = self.call_value(func, vec![acc, item.clone()], span)?;
                        }
                        Ok(acc)
                    }
                    (Value::Array(a), "sum") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sum() expects 0 arguments".into()).at(span));
                        }
                        let mut sum = 0.0;
                        for item in a.borrow().iter() {
                            if let Value::Number(n) = item {
                                sum += n;
                            } else {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!("Cannot sum non-number: {}", item.type_name())).at(span));
                            }
                        }
                        Ok(Value::Number(sum))
                    }
                    (Value::Array(a), "sort") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sort() expects 0 arguments".into()).at(span));
                        }
                        let mut arr = a.borrow_mut();
                        arr.sort_by(|x, y| {
                            match (x, y) {
                                (Value::Number(nx), Value::Number(ny)) => nx.partial_cmp(ny).unwrap_or(std::cmp::Ordering::Equal),
                                (Value::String(sx), Value::String(sy)) => sx.cmp(sy),
                                _ => std::cmp::Ordering::Equal,
                            }
                        });
                        Ok(Value::Array(a.clone()))
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
                            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(parts))))
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
            Pattern::Enum { enum_name, variant_name, fields } => {
                if let Value::EnumInstance { enum_name: v_enum, variant_name: v_variant, values } = val {
                    if enum_name == v_enum && variant_name == v_variant {
                        if fields.len() != values.len() {
                            return None;
                        }
                        let mut bindings = Vec::new();
                        for (f, v) in fields.iter().zip(values.iter()) {
                            bindings.push((f.clone(), v.clone()));
                        }
                        return Some(bindings);
                    }
                }
                None
            }
        }
    }
}
