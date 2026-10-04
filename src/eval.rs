use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, UnaryOp};
use crate::env::Environment;
use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum RuntimeError {
    #[error("Shae Runtime Error: {0}")]
    General(String),

    #[error("Shae Math Alert: Dividing by zero creates black holes. Let's keep the universe intact.")]
    DivisionByZero,
}

pub enum Signal {
    None,
    Value(Value),
    Return(Value),
    Break,
    Continue,
}

pub struct Evaluator {
    global_env: Rc<RefCell<Environment>>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Environment::new();
        register_builtins(&global_env);
        Self { global_env }
    }

    pub fn eval_program(&mut self, program: &Program) -> Result<Value, RuntimeError> {
        let mut last_val = Value::Null;
        let env = Rc::clone(&self.global_env);

        for stmt in &program.statements {
            match self.eval_stmt(stmt, &env)? {
                Signal::None => {}
                Signal::Value(val) => last_val = val,
                Signal::Return(val) => return Ok(val),
                Signal::Break => {
                    return Err(RuntimeError::General(
                        "Cannot use 'break' outside of a loop.".to_string(),
                    ));
                }
                Signal::Continue => {
                    return Err(RuntimeError::General(
                        "Cannot use 'continue' outside of a loop.".to_string(),
                    ));
                }
            }
        }

        Ok(last_val)
    }

    fn eval_stmt(
        &mut self,
        stmt: &Stmt,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<Signal, RuntimeError> {
        match stmt {
            Stmt::Let { name, init } => {
                let val = self.eval_expr(init, env)?;
                env.borrow_mut().define(name.clone(), val);
                Ok(Signal::None)
            }

            Stmt::Assign { target, value } => {
                let val = self.eval_expr(value, env)?;
                match target {
                    Expr::Variable(name) => {
                        env.borrow_mut()
                            .set(name, val)
                            .map_err(RuntimeError::General)?;
                    }
                    Expr::Index { target, index } => {
                        let target_val = self.eval_expr(target, env)?;
                        let index_val = self.eval_expr(index, env)?;
                        match (target_val, index_val) {
                            (Value::Array(arr), Value::Number(n)) => {
                                let mut borrowed = arr.borrow_mut();
                                let idx = n as isize;
                                let len = borrowed.len() as isize;
                                let actual_idx = if idx < 0 { len + idx } else { idx };
                                if actual_idx >= 0 && (actual_idx as usize) < borrowed.len() {
                                    borrowed[actual_idx as usize] = val;
                                } else {
                                    return Err(RuntimeError::General(format!(
                                        "Array index {} out of bounds for array of length {}.",
                                        idx,
                                        borrowed.len()
                                    )));
                                }
                            }
                            (Value::Map(map), Value::String(key)) => {
                                map.borrow_mut().insert(key, val);
                            }
                            (other_t, other_i) => {
                                return Err(RuntimeError::General(format!(
                                    "Cannot index assign on '{}' with index '{}'.",
                                    other_t.type_name(),
                                    other_i.type_name()
                                )));
                            }
                        }
                    }
                    Expr::Get {
                        target, property, ..
                    } => {
                        let target_val = self.eval_expr(target, env)?;
                        match target_val {
                            Value::Map(map) => {
                                map.borrow_mut().insert(property.clone(), val);
                            }
                            other => {
                                return Err(RuntimeError::General(format!(
                                    "Cannot set property '{}' on type '{}'.",
                                    property,
                                    other.type_name()
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(RuntimeError::General(
                            "Invalid assignment target.".to_string(),
                        ));
                    }
                }
                Ok(Signal::None)
            }

            Stmt::FnDef { name, params, body } => {
                let func = Value::Function {
                    name: Some(name.clone()),
                    params: params.clone(),
                    body: body.clone(),
                    closure: Rc::clone(env),
                };
                env.borrow_mut().define(name.clone(), func);
                Ok(Signal::None)
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.eval_expr(condition, env)?;
                if cond_val.is_truthy() {
                    self.eval_block(then_branch, env)
                } else if let Some(else_b) = else_branch {
                    self.eval_block(else_b, env)
                } else {
                    Ok(Signal::None)
                }
            }

            Stmt::While { condition, body } => {
                while self.eval_expr(condition, env)?.is_truthy() {
                    match self.eval_block(body, env)? {
                        Signal::None | Signal::Value(_) => {}
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                    }
                }
                Ok(Signal::None)
            }

            Stmt::For {
                item,
                iterable,
                body,
            } => {
                let iter_val = self.eval_expr(iterable, env)?;
                let items: Vec<Value> = match iter_val {
                    Value::Array(arr) => arr.borrow().clone(),
                    Value::Map(map) => map
                        .borrow()
                        .keys()
                        .map(|k| Value::String(k.clone()))
                        .collect(),
                    Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                    other => {
                        return Err(RuntimeError::General(format!(
                            "Cannot loop over non-iterable of type '{}'.",
                            other.type_name()
                        )));
                    }
                };

                for it in items {
                    let loop_env = Environment::with_parent(Rc::clone(env));
                    loop_env.borrow_mut().define(item.clone(), it);
                    match self.eval_block(body, &loop_env)? {
                        Signal::None | Signal::Value(_) => {}
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue => continue,
                    }
                }

                Ok(Signal::None)
            }

            Stmt::Return(opt_expr) => {
                let val = if let Some(expr) = opt_expr {
                    self.eval_expr(expr, env)?
                } else {
                    Value::Null
                };
                Ok(Signal::Return(val))
            }

            Stmt::Break => Ok(Signal::Break),
            Stmt::Continue => Ok(Signal::Continue),

            Stmt::Expr(expr) => {
                let v = self.eval_expr(expr, env)?;
                Ok(Signal::Value(v))
            }
        }
    }

    fn eval_block(
        &mut self,
        stmts: &[Stmt],
        env: &Rc<RefCell<Environment>>,
    ) -> Result<Signal, RuntimeError> {
        let block_env = Environment::with_parent(Rc::clone(env));
        let mut last_sig = Signal::None;
        for stmt in stmts {
            match self.eval_stmt(stmt, &block_env)? {
                Signal::None => {}
                Signal::Value(v) => last_sig = Signal::Value(v),
                other => return Ok(other),
            }
        }
        Ok(last_sig)
    }

    pub fn eval_expr(
        &mut self,
        expr: &Expr,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Literal(lit) => Ok(match lit {
                Literal::Number(n) => Value::Number(*n),
                Literal::String(s) => Value::String(s.clone()),
                Literal::Bool(b) => Value::Bool(*b),
                Literal::Null => Value::Null,
            }),

            Expr::Variable(name) => {
                if let Some(val) = env.borrow().get(name) {
                    Ok(val)
                } else {
                    Err(RuntimeError::General(format!(
                        "Undefined variable '{}'. Did you forget to define it with 'let {} = ...'?",
                        name, name
                    )))
                }
            }

            Expr::Array(items) => {
                let mut vals = Vec::new();
                for item in items {
                    vals.push(self.eval_expr(item, env)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vals))))
            }

            Expr::Map(entries) => {
                let mut map = HashMap::new();
                for (k, v) in entries {
                    let val = self.eval_expr(v, env)?;
                    map.insert(k.clone(), val);
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }

            Expr::Lambda { params, body } => Ok(Value::Function {
                name: None,
                params: params.clone(),
                body: body.clone(),
                closure: Rc::clone(env),
            }),

            Expr::Unary { op, expr } => {
                let val = self.eval_expr(expr, env)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!val.is_truthy())),
                    UnaryOp::Neg => match val {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        other => Err(RuntimeError::General(format!(
                            "Cannot negate non-number of type '{}'.",
                            other.type_name()
                        ))),
                    },
                }
            }

            Expr::Binary { left, op, right } => {
                // Short-circuiting for And, Or, Coalesce
                if *op == BinaryOp::And {
                    let left_val = self.eval_expr(left, env)?;
                    if !left_val.is_truthy() {
                        return Ok(left_val);
                    }
                    return self.eval_expr(right, env);
                }

                if *op == BinaryOp::Or {
                    let left_val = self.eval_expr(left, env)?;
                    if left_val.is_truthy() {
                        return Ok(left_val);
                    }
                    return self.eval_expr(right, env);
                }

                if *op == BinaryOp::Coalesce {
                    let left_val = self.eval_expr(left, env)?;
                    if left_val != Value::Null {
                        return Ok(left_val);
                    }
                    return self.eval_expr(right, env);
                }

                let l = self.eval_expr(left, env)?;
                let r = self.eval_expr(right, env)?;

                match op {
                    BinaryOp::Add => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Number(n1 + n2)),
                        (Value::Array(a1), Value::Array(a2)) => {
                            let mut combined = a1.borrow().clone();
                            combined.extend(a2.borrow().clone());
                            Ok(Value::Array(Rc::new(RefCell::new(combined))))
                        }
                        // String concatenation
                        _ => Ok(Value::String(format!("{}{}", l.to_display(), r.to_display()))),
                    },
                    BinaryOp::Sub => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Number(n1 - n2)),
                        _ => Err(RuntimeError::General(format!(
                            "Cannot subtract '{}' and '{}'.",
                            l.type_name(),
                            r.type_name()
                        ))),
                    },
                    BinaryOp::Mul => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Number(n1 * n2)),
                        (Value::String(s), Value::Number(n)) => {
                            let count = (*n as usize).max(0);
                            Ok(Value::String(s.repeat(count)))
                        }
                        _ => Err(RuntimeError::General(format!(
                            "Cannot multiply '{}' and '{}'.",
                            l.type_name(),
                            r.type_name()
                        ))),
                    },
                    BinaryOp::Div => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => {
                            if *n2 == 0.0 {
                                return Err(RuntimeError::DivisionByZero);
                            }
                            Ok(Value::Number(n1 / n2))
                        }
                        _ => Err(RuntimeError::General(format!(
                            "Cannot divide '{}' and '{}'.",
                            l.type_name(),
                            r.type_name()
                        ))),
                    },
                    BinaryOp::Mod => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => {
                            if *n2 == 0.0 {
                                return Err(RuntimeError::DivisionByZero);
                            }
                            Ok(Value::Number(n1 % n2))
                        }
                        _ => Err(RuntimeError::General(format!(
                            "Cannot modulo '{}' and '{}'.",
                            l.type_name(),
                            r.type_name()
                        ))),
                    },
                    BinaryOp::Eq => Ok(Value::Bool(l == r)),
                    BinaryOp::NotEq => Ok(Value::Bool(l != r)),
                    BinaryOp::Lt => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Bool(n1 < n2)),
                        (Value::String(s1), Value::String(s2)) => Ok(Value::Bool(s1 < s2)),
                        _ => Ok(Value::Bool(false)),
                    },
                    BinaryOp::LtEq => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Bool(n1 <= n2)),
                        (Value::String(s1), Value::String(s2)) => Ok(Value::Bool(s1 <= s2)),
                        _ => Ok(Value::Bool(false)),
                    },
                    BinaryOp::Gt => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Bool(n1 > n2)),
                        (Value::String(s1), Value::String(s2)) => Ok(Value::Bool(s1 > s2)),
                        _ => Ok(Value::Bool(false)),
                    },
                    BinaryOp::GtEq => match (&l, &r) {
                        (Value::Number(n1), Value::Number(n2)) => Ok(Value::Bool(n1 >= n2)),
                        (Value::String(s1), Value::String(s2)) => Ok(Value::Bool(s1 >= s2)),
                        _ => Ok(Value::Bool(false)),
                    },
                    BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce => unreachable!(),
                }
            }

            Expr::Call { callee, args } => {
                let callee_val = self.eval_expr(callee, env)?;
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval_expr(a, env)?);
                }

                match callee_val {
                    Value::Function {
                        params,
                        body,
                        closure,
                        ..
                    } => {
                        let fn_env = Environment::with_parent(closure);
                        for (i, param) in params.iter().enumerate() {
                            let arg = arg_vals.get(i).cloned().unwrap_or(Value::Null);
                            fn_env.borrow_mut().define(param.clone(), arg);
                        }

                        match self.eval_block(&body, &fn_env)? {
                            Signal::Return(val) => Ok(val),
                            Signal::Value(val) => Ok(val),
                            _ => Ok(Value::Null),
                        }
                    }

                    Value::Builtin { func, .. } => {
                        func(arg_vals, Rc::clone(env)).map_err(RuntimeError::General)
                    }

                    other => Err(RuntimeError::General(format!(
                        "Attempted to call a non-function of type '{}'.",
                        other.type_name()
                    ))),
                }
            }

            Expr::Get {
                target,
                property,
                safe,
            } => {
                let target_val = self.eval_expr(target, env)?;
                if target_val == Value::Null {
                    if *safe {
                        return Ok(Value::Null);
                    } else {
                        return Ok(Value::Null); // Shae safe by default
                    }
                }

                match target_val {
                    Value::Map(map) => {
                        let borrowed = map.borrow();
                        Ok(borrowed.get(property).cloned().unwrap_or(Value::Null))
                    }
                    Value::Array(arr) => match property.as_str() {
                        "len" => Ok(Value::Number(arr.borrow().len() as f64)),
                        "first" => Ok(arr.borrow().first().cloned().unwrap_or(Value::Null)),
                        "last" => Ok(arr.borrow().last().cloned().unwrap_or(Value::Null)),
                        _ => Ok(Value::Null),
                    },
                    Value::String(s) => match property.as_str() {
                        "len" => Ok(Value::Number(s.chars().count() as f64)),
                        _ => Ok(Value::Null),
                    },
                    _ => Ok(Value::Null),
                }
            }

            Expr::Index { target, index } => {
                let target_val = self.eval_expr(target, env)?;
                let index_val = self.eval_expr(index, env)?;

                match (target_val, index_val) {
                    (Value::Array(arr), Value::Number(n)) => {
                        let borrowed = arr.borrow();
                        let idx = n as isize;
                        let len = borrowed.len() as isize;
                        let actual_idx = if idx < 0 { len + idx } else { idx };
                        if actual_idx >= 0 && (actual_idx as usize) < borrowed.len() {
                            Ok(borrowed[actual_idx as usize].clone())
                        } else {
                            Ok(Value::Null) // out of bounds returns null without crash
                        }
                    }
                    (Value::Map(map), Value::String(key)) => {
                        let borrowed = map.borrow();
                        Ok(borrowed.get(&key).cloned().unwrap_or(Value::Null))
                    }
                    (Value::String(s), Value::Number(n)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let idx = n as isize;
                        let len = chars.len() as isize;
                        let actual_idx = if idx < 0 { len + idx } else { idx };
                        if actual_idx >= 0 && (actual_idx as usize) < chars.len() {
                            Ok(Value::String(chars[actual_idx as usize].to_string()))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    _ => Ok(Value::Null),
                }
            }
        }
    }
}

fn register_builtins(env: &Rc<RefCell<Environment>>) {
    let mut e = env.borrow_mut();

    // print(...)
    e.define(
        "print".to_string(),
        Value::Builtin {
            name: "print".to_string(),
            func: |args, _| {
                let items: Vec<String> = args.iter().map(|a| a.to_display()).collect();
                println!("{}", items.join(" "));
                Ok(Value::Null)
            },
        },
    );

    // dbg(val)
    e.define(
        "dbg".to_string(),
        Value::Builtin {
            name: "dbg".to_string(),
            func: |args, _| {
                if let Some(val) = args.first() {
                    eprintln!("🔍 [Shae dbg] ({}) => {}", val.type_name(), val.to_repr());
                    Ok(val.clone())
                } else {
                    Ok(Value::Null)
                }
            },
        },
    );

    // len(val)
    e.define(
        "len".to_string(),
        Value::Builtin {
            name: "len".to_string(),
            func: |args, _| {
                let val = args.get(0).unwrap_or(&Value::Null);
                match val {
                    Value::Array(a) => Ok(Value::Number(a.borrow().len() as f64)),
                    Value::String(s) => Ok(Value::Number(s.chars().count() as f64)),
                    Value::Map(m) => Ok(Value::Number(m.borrow().len() as f64)),
                    _ => Ok(Value::Number(0.0)),
                }
            },
        },
    );

    // type(val)
    e.define(
        "type".to_string(),
        Value::Builtin {
            name: "type".to_string(),
            func: |args, _| {
                let val = args.get(0).unwrap_or(&Value::Null);
                Ok(Value::String(val.type_name().to_string()))
            },
        },
    );

    // range(start, end, [step])
    e.define(
        "range".to_string(),
        Value::Builtin {
            name: "range".to_string(),
            func: |args, _| {
                let (start, end, step) = match args.as_slice() {
                    [Value::Number(end)] => (0.0, *end, 1.0),
                    [Value::Number(start), Value::Number(end)] => (*start, *end, 1.0),
                    [Value::Number(start), Value::Number(end), Value::Number(step)] => {
                        (*start, *end, *step)
                    }
                    _ => return Err("range() expects (end) or (start, end, [step]) numbers".into()),
                };

                let mut list = Vec::new();
                let mut cur = start;
                if step > 0.0 {
                    while cur < end {
                        list.push(Value::Number(cur));
                        cur += step;
                    }
                } else if step < 0.0 {
                    while cur > end {
                        list.push(Value::Number(cur));
                        cur += step;
                    }
                }
                Ok(Value::Array(Rc::new(RefCell::new(list))))
            },
        },
    );

    // push(array, value)
    e.define(
        "push".to_string(),
        Value::Builtin {
            name: "push".to_string(),
            func: |args, _| {
                if let (Some(Value::Array(arr)), Some(val)) = (args.get(0), args.get(1)) {
                    arr.borrow_mut().push(val.clone());
                    Ok(Value::Number(arr.borrow().len() as f64))
                } else {
                    Err("push() expects (array, value)".into())
                }
            },
        },
    );

    // pop(array)
    e.define(
        "pop".to_string(),
        Value::Builtin {
            name: "pop".to_string(),
            func: |args, _| {
                if let Some(Value::Array(arr)) = args.get(0) {
                    Ok(arr.borrow_mut().pop().unwrap_or(Value::Null))
                } else {
                    Err("pop() expects (array)".into())
                }
            },
        },
    );

    // keys(map)
    e.define(
        "keys".to_string(),
        Value::Builtin {
            name: "keys".to_string(),
            func: |args, _| {
                if let Some(Value::Map(m)) = args.get(0) {
                    let list: Vec<Value> = m
                        .borrow()
                        .keys()
                        .map(|k| Value::String(k.clone()))
                        .collect();
                    Ok(Value::Array(Rc::new(RefCell::new(list))))
                } else {
                    Err("keys() expects (map)".into())
                }
            },
        },
    );

    // values(map)
    e.define(
        "values".to_string(),
        Value::Builtin {
            name: "values".to_string(),
            func: |args, _| {
                if let Some(Value::Map(m)) = args.get(0) {
                    let list: Vec<Value> = m.borrow().values().cloned().collect();
                    Ok(Value::Array(Rc::new(RefCell::new(list))))
                } else {
                    Err("values() expects (map)".into())
                }
            },
        },
    );

    // json_parse(str)
    e.define(
        "json_parse".to_string(),
        Value::Builtin {
            name: "json_parse".to_string(),
            func: |args, _| {
                if let Some(Value::String(s)) = args.get(0) {
                    let parsed: serde_json::Value = serde_json::from_str(s)
                        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
                    Ok(Value::from_json(&parsed))
                } else {
                    Err("json_parse() expects (string)".into())
                }
            },
        },
    );

    // json_stringify(val)
    e.define(
        "json_stringify".to_string(),
        Value::Builtin {
            name: "json_stringify".to_string(),
            func: |args, _| {
                if let Some(val) = args.get(0) {
                    let json = val.to_json();
                    let s = serde_json::to_string_pretty(&json)
                        .map_err(|e| format!("Failed to stringify JSON: {}", e))?;
                    Ok(Value::String(s))
                } else {
                    Err("json_stringify() expects (value)".into())
                }
            },
        },
    );

    // serve(port, handler_fn) - First-Class HTTP Web Server!
    e.define(
        "serve".to_string(),
        Value::Builtin {
            name: "serve".to_string(),
            func: |args, _| {
                let port = match args.get(0) {
                    Some(Value::Number(n)) => *n as u16,
                    _ => 8080,
                };

                let handler = match args.get(1) {
                    Some(func @ Value::Function { .. }) => func.clone(),
                    _ => return Err("serve() expects (port, handler_function)".into()),
                };

                let addr = format!("127.0.0.1:{}", port);
                let listener = TcpListener::bind(&addr)
                    .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

                println!("\n✨ Shae Web Server active!");
                println!("🌐 Listening at http://{}", addr);
                println!("👉 Press Ctrl+C to stop.\n");

                for stream in listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            let mut reader = BufReader::new(&stream);
                            let mut request_line = String::new();
                            if reader.read_line(&mut request_line).is_ok() {
                                let parts: Vec<&str> = request_line.split_whitespace().collect();
                                let method = parts.get(0).copied().unwrap_or("GET");
                                let path = parts.get(1).copied().unwrap_or("/");

                                // Construct request Map for Shae handler
                                let mut req_map = HashMap::new();
                                req_map.insert("method".to_string(), Value::String(method.to_string()));
                                req_map.insert("path".to_string(), Value::String(path.to_string()));

                                // Invoke handler function
                                let result = match &handler {
                                    Value::Function {
                                        params,
                                        body,
                                        closure,
                                        ..
                                    } => {
                                        let call_env = Environment::with_parent(Rc::clone(closure));
                                        if let Some(param_name) = params.first() {
                                            call_env.borrow_mut().define(
                                                param_name.clone(),
                                                Value::Map(Rc::new(RefCell::new(req_map))),
                                            );
                                        }

                                        let mut eval = Evaluator {
                                            global_env: Rc::clone(&call_env),
                                        };
                                        match eval.eval_block(body, &call_env) {
                                            Ok(Signal::Return(v)) => v,
                                            Ok(_) => Value::Null,
                                            Err(e) => Value::String(format!("Error: {}", e)),
                                        }
                                    }
                                    _ => Value::String("Invalid handler".into()),
                                };

                                let (content_type, body_str) = match result {
                                    Value::Map(_) | Value::Array(_) => (
                                        "application/json",
                                        serde_json::to_string_pretty(&result.to_json())
                                            .unwrap_or_default(),
                                    ),
                                    _ => ("text/html; charset=utf-8", result.to_display()),
                                };

                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    content_type,
                                    body_str.len(),
                                    body_str
                                );

                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                        Err(e) => {
                            eprintln!("Shae Server connection error: {}", e);
                        }
                    }
                }

                Ok(Value::Null)
            },
        },
    );
}

pub fn evaluate(program: &Program) -> Result<Value, RuntimeError> {
    Evaluator::new().eval_program(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    #[test]
    fn test_eval_fibonacci() {
        let code = r#"
            fn fib(n) {
                if n <= 1 {
                    return n
                }
                return fib(n - 1) + fib(n - 2)
            }
            return fib(10)
        "#;
        let tokens = tokenize(code).unwrap();
        let program = parse(tokens).unwrap();
        let res = evaluate(&program).unwrap();
        assert_eq!(res, Value::Number(55.0));
    }

    #[test]
    fn test_eval_loops_and_arrays() {
        let code = r#"
            let sum = 0
            for x in [10, 20, 30] {
                sum += x
            }
            return sum
        "#;
        let tokens = tokenize(code).unwrap();
        let program = parse(tokens).unwrap();
        let res = evaluate(&program).unwrap();
        assert_eq!(res, Value::Number(60.0));
    }

    #[test]
    fn test_eval_map_and_safe_coalesce() {
        let code = r#"
            let user = { name: "Satya", age: 21 }
            let role = user.role ?? "General"
            return user.name + " is " + role
        "#;
        let tokens = tokenize(code).unwrap();
        let program = parse(tokens).unwrap();
        let res = evaluate(&program).unwrap();
        assert_eq!(res, Value::String("Satya is General".into()));
    }
}
