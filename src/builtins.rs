use crate::ast::Span;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::RwLock;
use std::sync::Arc;

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
        ("map", builtin_map),
        ("filter", builtin_filter),
        ("pop", builtin_pop),
        ("keys", builtin_keys),
        ("values", builtin_values),
        ("get", builtin_get),
        ("json_parse", builtin_json_parse),
        ("json_stringify", builtin_json_stringify),
        ("serve", builtin_serve),
        ("read", builtin_read),
        ("write", builtin_write),
        ("fetch", builtin_fetch),
        ("time", builtin_time),
        ("env", builtin_env),
        ("exec", builtin_exec),
        ("spawn", builtin_spawn),
        ("join", builtin_join),
        ("assert", builtin_assert),
        ("assert_eq", builtin_assert_eq),
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
        Value::String(s) => Ok(Value::Int(s.len() as i64)),
        Value::Array(a) => Ok(Value::Int(a.read().unwrap().len() as i64)),
        Value::Map(m) => Ok(Value::Int(m.read().unwrap().len() as i64)),
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
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(n) | Value::Number(n) => Ok(Value::Float(*n)),
        Value::String(s) => {
            if let Ok(i) = s.parse::<i64>() {
                Ok(Value::Int(i))
            } else if let Ok(f) = s.parse::<f64>() {
                Ok(Value::Float(f))
            } else {
                Err(RuntimeError::new(format!("Cannot parse '{}' as number", s)).at(span))
            }
        }
        other => Err(
            RuntimeError::new(format!("Cannot convert {} to number", other.type_name())).at(span),
        ),
    }
}

fn builtin_range(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let all_ints = args.iter().all(|a| matches!(a, Value::Int(_)));
    if all_ints && !args.is_empty() && args.len() <= 3 {
        let (start, end, step) = match args.len() {
            1 => {
                if let Value::Int(e) = args[0] {
                    (0i64, e, 1i64)
                } else {
                    unreachable!()
                }
            }
            2 => {
                if let (Value::Int(s), Value::Int(e)) = (&args[0], &args[1]) {
                    (*s, *e, 1i64)
                } else {
                    unreachable!()
                }
            }
            3 => {
                if let (Value::Int(s), Value::Int(e), Value::Int(st)) = (&args[0], &args[1], &args[2]) {
                    if *st == 0 {
                        return Err(RuntimeError::new("range() step cannot be zero".into()).at(span));
                    }
                    (*s, *e, *st)
                } else {
                    unreachable!()
                }
            }
            _ => unreachable!(),
        };

        let mut current = start;
        let mut res = Vec::new();
        if step > 0 {
            while current < end {
                if res.len() > 100000 {
                    return Err(
                        RuntimeError::new("range() generated too many elements".into()).at(span),
                    );
                }
                res.push(Value::Int(current));
                current += step;
            }
        } else {
            while current > end {
                if res.len() > 100000 {
                    return Err(
                        RuntimeError::new("range() generated too many elements".into()).at(span),
                    );
                }
                res.push(Value::Int(current));
                current += step;
            }
        }
        return Ok(Value::Array(Arc::new(RwLock::new(res))));
    }

    let to_f64 = |v: &Value| match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) | Value::Number(f) => Some(*f),
        _ => None,
    };

    let (start, end, step) = match args.len() {
        1 => {
            if let Some(e) = to_f64(&args[0]) {
                (0.0, e, 1.0)
            } else {
                return Err(RuntimeError::new("range() expects number arguments".into()).at(span));
            }
        }
        2 => {
            if let (Some(s), Some(e)) = (to_f64(&args[0]), to_f64(&args[1])) {
                (s, e, 1.0)
            } else {
                return Err(RuntimeError::new("range() expects number arguments".into()).at(span));
            }
        }
        3 => {
            if let (Some(s), Some(e), Some(st)) = (to_f64(&args[0]), to_f64(&args[1]), to_f64(&args[2])) {
                if st == 0.0 {
                    return Err(RuntimeError::new("range() step cannot be zero".into()).at(span));
                }
                (s, e, st)
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
            res.push(Value::Float(current));
            current += step;
        }
    } else {
        while current > end {
            if res.len() > 100000 {
                return Err(
                    RuntimeError::new("range() generated too many elements".into()).at(span),
                );
            }
            res.push(Value::Float(current));
            current += step;
        }
    }
    Ok(Value::Array(Arc::new(RwLock::new(res))))
}

fn builtin_push(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("push", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        arr.write().unwrap().push(args[1].clone());
        Ok(Value::Null)
    } else {
        Err(RuntimeError::new("First argument to push must be an array".into()).at(span))
    }
}

fn builtin_pop(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("pop", 1, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        arr.write().unwrap()
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
            .read().unwrap()
            .keys()
            .map(|k| Value::String(k.clone()))
            .collect();
        Ok(Value::Array(Arc::new(RwLock::new(keys))))
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
        let values = m.read().unwrap().values().cloned().collect();
        Ok(Value::Array(Arc::new(RwLock::new(values))))
    } else {
        Err(RuntimeError::new("Argument to values must be a map".into()).at(span))
    }
}

fn builtin_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("get", 3, &args, span)?;
    match (&args[0], &args[1]) {
        (Value::Map(m), Value::String(k)) => {
            if let Some(v) = m.read().unwrap().get(k) {
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

    let port = match args[0] {
        Value::Int(p) => {
            if p < 1 || p > 65535 {
                return Err(RuntimeError::new("Port must be a whole number between 1 and 65535".into()).at(span));
            }
            p as u16
        }
        Value::Float(p) | Value::Number(p) => {
            if p.fract() != 0.0 || p < 1.0 || p > 65535.0 {
                return Err(RuntimeError::new("Port must be a whole number between 1 and 65535".into()).at(span));
            }
            p as u16
        }
        _ => return Err(RuntimeError::new("First argument to serve must be a port number".into()).at(span)),
    };

    let handler = args[1].clone();
    if !matches!(handler, Value::Function { .. } | Value::Builtin { .. }) {
        return Err(
            RuntimeError::new("Second argument to serve must be a function".into()).at(span),
        );
    }

    use std::net::TcpListener;

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).map_err(|e| {
        RuntimeError::new(format!("Failed to bind to port {}: {}", port, e)).at(span)
    })?;

    println!("Server running on http://localhost:{}", port);

    let env_root = ev.global_env.clone();
    let handler_root = handler.clone();

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let env_clone = env_root.clone();
            let handler_clone = handler_root.clone();
            std::thread::spawn(move || {
                let mut worker_eval = Evaluator::with_env(env_clone);
                let _ = handle_http_client(&mut stream, &mut worker_eval, &handler_clone, span);
            });
        }
    }
    Ok(Value::Null)
}

fn handle_http_client(
    stream: &mut std::net::TcpStream,
    ev: &mut Evaluator,
    handler: &Value,
    span: Span,
) -> Result<(), ()> {
    use std::io::{Read, Write};

    let mut buffer = [0u8; 8192];
    let size = stream.read(&mut buffer).map_err(|_| ())?;
    if size == 0 {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&buffer[..size]);
    let (head, body_str) = if let Some(idx) = req_str.find("\r\n\r\n") {
        (&req_str[..idx], &req_str[idx + 4..])
    } else if let Some(idx) = req_str.find("\n\n") {
        (&req_str[..idx], &req_str[idx + 2..])
    } else {
        (req_str.as_ref(), "")
    };

    let mut lines = head.lines();
    let request_line = match lines.next() {
        Some(l) => l,
        None => return Ok(()),
    };
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0].to_uppercase();
    let full_path = parts[1].to_string();

    let mut path = full_path.clone();
    let mut query = "".to_string();
    if let Some(idx) = full_path.find('?') {
        path = full_path[..idx].to_string();
        query = full_path[idx + 1..].to_string();
    }

    let mut headers = IndexMap::new();
    for line in lines {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_lowercase();
            let val = line[idx + 1..].trim();
            headers.insert(key, Value::String(val.to_string()));
        }
    }

    let mut req_map = IndexMap::new();
    req_map.insert("method".to_string(), Value::String(method));
    req_map.insert("path".to_string(), Value::String(path));
    req_map.insert("query".to_string(), Value::String(query));
    req_map.insert("headers".to_string(), Value::Map(Arc::new(RwLock::new(headers))));
    req_map.insert("body".to_string(), Value::String(body_str.to_string()));

    let req_val = Value::Map(Arc::new(RwLock::new(req_map)));
    let res = ev.call_value(handler, vec![req_val], span);

    let (status_code, status_text, content_type, custom_headers, resp_body) = match res {
        Ok(val) => format_http_response(val),
        Err(e) => {
            let err_msg = e.to_string();
            eprintln!("Handler error: {}", err_msg);
            (500, "Internal Server Error", "text/plain".to_string(), vec![], err_msg)
        }
    };

    let mut response_head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_code, status_text, content_type, resp_body.len()
    );
    for (k, v) in custom_headers {
        response_head.push_str(&format!("{}: {}\r\n", k, v));
    }
    response_head.push_str("\r\n");

    let _ = stream.write_all(response_head.as_bytes());
    let _ = stream.write_all(resp_body.as_bytes());
    let _ = stream.flush();
    Ok(())
}

fn format_http_response(val: Value) -> (u16, &'static str, String, Vec<(String, String)>, String) {
    match val {
        Value::Map(m) => {
            let map = m.read().unwrap();
            let status_num = map.get("status")
                .or_else(|| map.get("statusCode"))
                .or_else(|| map.get("status_code"))
                .and_then(|v| match v {
                    Value::Int(n) => Some(*n as u16),
                    Value::Float(n) | Value::Number(n) => Some(*n as u16),
                    _ => None,
                });

            if let Some(code) = status_num {
                let text = http_status_text(code);
                let mut custom_headers = Vec::new();
                if let Some(Value::Map(h)) = map.get("headers") {
                    for (hk, hv) in h.read().unwrap().iter() {
                        custom_headers.push((hk.clone(), hv.to_display()));
                    }
                }

                if let Some(body_val) = map.get("body") {
                    let (ct, body_str) = match body_val {
                        Value::Map(_) | Value::Array(_) => (
                            "application/json".to_string(),
                            serde_json::to_string(&body_val.to_json()).unwrap_or_else(|_| "{}".to_string())
                        ),
                        Value::String(s) => ("text/html".to_string(), s.clone()),
                        other => ("text/plain".to_string(), other.to_display()),
                    };
                    return (code, text, ct, custom_headers, body_str);
                } else {
                    let json_str = serde_json::to_string(&Value::Map(m.clone()).to_json()).unwrap_or_else(|_| "{}".to_string());
                    return (code, text, "application/json".to_string(), custom_headers, json_str);
                }
            }

            let body_str = serde_json::to_string(&Value::Map(m.clone()).to_json()).unwrap_or_else(|_| "{}".to_string());
            (200, "OK", "application/json".to_string(), vec![], body_str)
        }
        Value::Array(_) => {
            let body_str = serde_json::to_string(&val.to_json()).unwrap_or_else(|_| "[]".to_string());
            (200, "OK", "application/json".to_string(), vec![], body_str)
        }
        _ => {
            let body_str = val.to_display();
            (200, "OK", "text/html".to_string(), vec![], body_str)
        }
    }
}

fn http_status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Custom Status",
    }
}

fn builtin_read(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("read", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(Value::String(content)),
            Err(e) => Err(RuntimeError::new(format!("Failed to read file '{}': {}", path, e)).at(span)),
        }
    } else {
        Err(RuntimeError::new("read expects a string path".into()).at(span))
    }
}

fn builtin_write(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("write", 2, &args, span)?;
    if let (Value::String(path), Value::String(content)) = (&args[0], &args[1]) {
        match std::fs::write(path, content) {
            Ok(_) => Ok(Value::Null),
            Err(e) => Err(RuntimeError::new(format!("Failed to write to file '{}': {}", path, e)).at(span)),
        }
    } else {
        Err(RuntimeError::new("write expects (string path, string content)".into()).at(span))
    }
}

fn builtin_fetch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("fetch", 1, &args, span)?;
    if let Value::String(url) = &args[0] {
        match reqwest::blocking::get(url) {
            Ok(response) => {
                match response.text() {
                    Ok(text) => Ok(Value::String(text)),
                    Err(e) => Err(RuntimeError::new(format!("Failed to read response from '{}': {}", url, e)).at(span)),
                }
            }
            Err(e) => Err(RuntimeError::new(format!("Failed to fetch '{}': {}", url, e)).at(span)),
        }
    } else {
        Err(RuntimeError::new("fetch expects a string url".into()).at(span))
    }
}

fn builtin_time(_eval: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(Value::Number(duration.as_secs_f64()))
    } else {
        Ok(Value::Number(0.0))
    }
}

fn builtin_env(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("env", 1, &args, span)?;
    if let Value::String(k) = &args[0] {
        if let Ok(val) = std::env::var(k) {
            Ok(Value::String(val))
        } else {
            Ok(Value::Null)
        }
    } else {
        Err(RuntimeError::new("env() key must be a string.".to_string()).at(span))
    }
}

fn builtin_exec(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("exec", 1, &args, span)?;
    if let Value::String(cmd) = &args[0] {
        if let Ok(output) = std::process::Command::new("sh").arg("-c").arg(cmd).output() {
            let out = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(Value::String(out))
        } else {
            Err(RuntimeError::new("Failed to execute command.".to_string()).at(span))
        }
    } else {
        Err(RuntimeError::new("exec() command must be a string.".to_string()).at(span))
    }
}

fn builtin_spawn(eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spawn", 1, &args, span)?;
    let func = args[0].clone();
    let env = eval.global_env.clone();
    let handle = std::thread::spawn(move || {
        let mut spawn_eval = Evaluator::with_env(env);
        spawn_eval.call_value(&func, vec![], span)
    });
    Ok(Value::Task(Arc::new(std::sync::Mutex::new(Some(handle)))))
}

fn builtin_join(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("join", 1, &args, span)?;
    match &args[0] {
        Value::Task(t) => {
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
        other => Err(RuntimeError::new(format!("join() expects a task, got {}", other.type_name())).at(span)),
    }
}

fn builtin_assert(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new("'assert' expects 1 or 2 arguments (condition, [message])".into()).at(span));
    }
    let cond = &args[0];
    if !cond.is_truthy() {
        let msg = if args.len() == 2 {
            args[1].to_display()
        } else {
            "Assertion failed".to_string()
        };
        return Err(RuntimeError::new(msg).at(span));
    }
    Ok(Value::Null)
}

fn builtin_assert_eq(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("'assert_eq' expects 2 or 3 arguments (actual, expected, [message])".into()).at(span));
    }
    let actual = &args[0];
    let expected = &args[1];
    if actual != expected {
        let extra = if args.len() == 3 {
            format!(": {}", args[2].to_display())
        } else {
            String::new()
        };
        return Err(RuntimeError::new(format!(
            "Assertion failed: expected {}, but got {}{}",
            expected.to_repr(),
            actual.to_repr(),
            extra
        )).at(span));
    }
    Ok(Value::Null)
}

fn builtin_map(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("map", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        let func = &args[1];
        let mut new_arr = Vec::new();
        for item in arr.read().unwrap().iter() {
            let mapped = ev.call_value(func, vec![item.clone()], span)?;
            new_arr.push(mapped);
        }
        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(new_arr))))
    } else {
        Err(RuntimeError::new("First argument to map must be an array".into()).at(span))
    }
}

fn builtin_filter(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("filter", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        let func = &args[1];
        let mut new_arr = Vec::new();
        for item in arr.read().unwrap().iter() {
            let cond = ev.call_value(func, vec![item.clone()], span)?;
            if cond.is_truthy() {
                new_arr.push(item.clone());
            }
        }
        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(new_arr))))
    } else {
        Err(RuntimeError::new("First argument to filter must be an array".into()).at(span))
    }
}
