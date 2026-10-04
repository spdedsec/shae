use crate::ast::Span;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

pub fn expect_args(
    name: &str,
    expected: usize,
    args: &[Value],
    span: Span,
) -> Result<(), RuntimeError> {
    if args.len() != expected {
        Err(RuntimeError::new(format!(
            "'{}' expects {} arguments, but got {}",
            name,
            expected,
            args.len()
        ))
        .at(span))
    } else {
        Ok(())
    }
}

pub fn register(env: &mut Environment) {
    let builtins: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("print", builtin_print),
        ("dbg", builtin_dbg),
        ("len", builtin_len),
        ("type", builtin_type),
        ("str", builtin_str),
        ("num", builtin_num),
        ("range", builtin_range),
        ("push", builtin_push),
        ("pop", builtin_pop),
        ("keys", builtin_keys),
        ("values", builtin_values),
        ("get", builtin_get),
        ("json_parse", builtin_json_parse),
        ("json_stringify", builtin_json_stringify),
        ("serve", builtin_serve),
    ];
    for (name, func) in builtins {
        env.define(
            name.to_string(),
            Value::Builtin {
                name: name.to_string(),
                func,
            },
        );
    }
}

fn builtin_print(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let out: Vec<String> = args.iter().map(|v| v.to_display()).collect();
    println!("{}", out.join(" "));
    Ok(Value::Null)
}

fn builtin_dbg(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let out: Vec<String> = args.iter().map(|v| v.to_repr()).collect();
    println!("DEBUG: {}", out.join(", "));
    Ok(Value::Null)
}

fn builtin_len(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("len", 1, &args, span)?;
    match &args[0] {
        Value::String(s) => Ok(Value::Number(s.len() as f64)),
        Value::Array(a) => Ok(Value::Number(a.borrow().len() as f64)),
        Value::Map(m) => Ok(Value::Number(m.borrow().len() as f64)),
        other => {
            Err(RuntimeError::new(format!("Cannot get length of {}", other.type_name())).at(span))
        }
    }
}

fn builtin_type(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("type", 1, &args, span)?;
    Ok(Value::String(args[0].type_name().to_string()))
}

fn builtin_str(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("str", 1, &args, span)?;
    Ok(Value::String(args[0].to_display()))
}

fn builtin_num(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("num", 1, &args, span)?;
    match &args[0] {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::String(s) => s
            .parse::<f64>()
            .map(Value::Number)
            .map_err(|_| RuntimeError::new(format!("Cannot parse '{}' as number", s)).at(span)),
        other => Err(
            RuntimeError::new(format!("Cannot convert {} to number", other.type_name())).at(span),
        ),
    }
}

fn builtin_range(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let (start, end, step) = match args.len() {
        1 => {
            if let Value::Number(e) = args[0] {
                (0.0, e, 1.0)
            } else {
                return Err(RuntimeError::new("range() expects number arguments".into()).at(span));
            }
        }
        2 => {
            if let (Value::Number(s), Value::Number(e)) = (&args[0], &args[1]) {
                (*s, *e, 1.0)
            } else {
                return Err(RuntimeError::new("range() expects number arguments".into()).at(span));
            }
        }
        3 => {
            if let (Value::Number(s), Value::Number(e), Value::Number(st)) =
                (&args[0], &args[1], &args[2])
            {
                if *st == 0.0 {
                    return Err(RuntimeError::new("range() step cannot be zero".into()).at(span));
                }
                (*s, *e, *st)
            } else {
                return Err(RuntimeError::new("range() expects number arguments".into()).at(span));
            }
        }
        _ => {
            return Err(RuntimeError::new(format!(
                "range() expects 1 to 3 arguments, got {}",
                args.len()
            ))
            .at(span));
        }
    };

    let mut current = start;
    let mut res = Vec::new();
    if step > 0.0 {
        while current < end {
            if res.len() > 100000 {
                return Err(
                    RuntimeError::new("range() generated too many elements".into()).at(span),
                );
            }
            res.push(Value::Number(current));
            current += step;
        }
    } else {
        while current > end {
            if res.len() > 100000 {
                return Err(
                    RuntimeError::new("range() generated too many elements".into()).at(span),
                );
            }
            res.push(Value::Number(current));
            current += step;
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(res))))
}

fn builtin_push(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("push", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        arr.borrow_mut().push(args[1].clone());
        Ok(Value::Null)
    } else {
        Err(RuntimeError::new("First argument to push must be an array".into()).at(span))
    }
}

fn builtin_pop(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("pop", 1, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        arr.borrow_mut()
            .pop()
            .ok_or_else(|| RuntimeError::new("Cannot pop from an empty array".into()).at(span))
    } else {
        Err(RuntimeError::new("Argument to pop must be an array".into()).at(span))
    }
}

fn builtin_keys(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("keys", 1, &args, span)?;
    if let Value::Map(m) = &args[0] {
        let keys = m
            .borrow()
            .keys()
            .map(|k| Value::String(k.clone()))
            .collect();
        Ok(Value::Array(Rc::new(RefCell::new(keys))))
    } else {
        Err(RuntimeError::new("Argument to keys must be a map".into()).at(span))
    }
}

fn builtin_values(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("values", 1, &args, span)?;
    if let Value::Map(m) = &args[0] {
        let values = m.borrow().values().cloned().collect();
        Ok(Value::Array(Rc::new(RefCell::new(values))))
    } else {
        Err(RuntimeError::new("Argument to values must be a map".into()).at(span))
    }
}

fn builtin_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("get", 3, &args, span)?;
    match (&args[0], &args[1]) {
        (Value::Map(m), Value::String(k)) => {
            if let Some(v) = m.borrow().get(k) {
                Ok(v.clone())
            } else {
                Ok(args[2].clone())
            }
        }
        _ => Err(RuntimeError::new("get expects (map, string, default)".into()).at(span)),
    }
}

fn builtin_json_parse(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("json_parse", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        let parsed: serde_json::Value = serde_json::from_str(s)
            .map_err(|e| RuntimeError::new(format!("JSON parse error: {}", e)).at(span))?;
        Ok(Value::from_json(&parsed))
    } else {
        Err(RuntimeError::new("json_parse expects a string".into()).at(span))
    }
}

fn builtin_json_stringify(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let (val, pretty) = match args.len() {
        1 => (&args[0], false),
        2 => {
            if let Value::Bool(b) = &args[1] {
                (&args[0], *b)
            } else {
                return Err(RuntimeError::new(
                    "json_stringify second argument must be boolean".into(),
                )
                .at(span));
            }
        }
        _ => {
            return Err(
                RuntimeError::new("json_stringify expects 1 or 2 arguments".into()).at(span),
            );
        }
    };
    let json = val.to_json();
    let s = if pretty {
        serde_json::to_string_pretty(&json)
    } else {
        serde_json::to_string(&json)
    }
    .map_err(|e| RuntimeError::new(format!("JSON stringify error: {}", e)).at(span))?;
    Ok(Value::String(s))
}

fn builtin_serve(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("serve", 2, &args, span)?;

    let port = if let Value::Number(p) = args[0] {
        if p.fract() != 0.0 || p < 1.0 || p > 65535.0 {
            return Err(RuntimeError::new(
                "Port must be a whole number between 1 and 65535".into(),
            )
            .at(span));
        }
        p as u16
    } else {
        return Err(
            RuntimeError::new("First argument to serve must be a port number".into()).at(span),
        );
    };

    let handler = args[1].clone();
    if !matches!(handler, Value::Function { .. } | Value::Builtin { .. }) {
        return Err(
            RuntimeError::new("Second argument to serve must be a function".into()).at(span),
        );
    }

    // Since serve starts a server and blocks, we just mock the server logic here using standard library TCP
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).map_err(|e| {
        RuntimeError::new(format!("Failed to bind to port {}: {}", port, e)).at(span)
    })?;

    println!("Server running on http://localhost:{}", port);

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let mut buffer = [0; 4096];
            if let Ok(size) = stream.read(&mut buffer) {
                if size == 0 {
                    continue;
                }

                let req_str = String::from_utf8_lossy(&buffer[..size]);
                let lines: Vec<&str> = req_str.lines().collect();
                if lines.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = lines[0].split_whitespace().collect();
                if parts.len() < 2 {
                    continue;
                }

                let method = parts[0].to_string();
                let full_path = parts[1].to_string();

                let mut path = full_path.clone();
                let mut query = "".to_string();
                if let Some(idx) = full_path.find('?') {
                    path = full_path[..idx].to_string();
                    query = full_path[idx + 1..].to_string();
                }

                let mut req_map = IndexMap::new();
                req_map.insert("method".to_string(), Value::String(method));
                req_map.insert("path".to_string(), Value::String(path));
                req_map.insert("query".to_string(), Value::String(query));

                let req_val = Value::Map(Rc::new(RefCell::new(req_map)));

                let res = ev.call_value(&handler, vec![req_val], span);

                match res {
                    Ok(val) => {
                        let (content_type, body) = match val {
                            Value::Map(_) | Value::Array(_) => (
                                "application/json",
                                serde_json::to_string(&val.to_json())
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                            _ => ("text/html", val.to_display()),
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}",
                            content_type,
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        eprintln!("Handler error: {}", err_msg);
                        let response = format!(
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                            err_msg.len(),
                            err_msg
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
            }
        }
    }
    Ok(Value::Null)
}
