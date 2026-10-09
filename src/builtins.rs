use crate::ast::Span;
use crate::env::Environment;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::Arc;
use std::sync::RwLock;

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
        ("println", builtin_print),
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
        ("serve_tls", builtin_serve_tls),
        ("serve_https", builtin_serve_tls),
        ("read", builtin_read),
        ("write", builtin_write),
        ("fetch", builtin_fetch),
        ("time", builtin_time),
        ("env", builtin_env),
        ("exec", builtin_exec),
        ("spawn", builtin_spawn),
        ("join", builtin_join),
        ("channel", builtin_channel),
        ("send", builtin_send),
        ("recv", builtin_recv),
        ("try_recv", builtin_try_recv),
        ("tryRecv", builtin_try_recv),
        ("close", builtin_close),
        ("route_match", builtin_route_match),
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
                if let (Value::Int(s), Value::Int(e), Value::Int(st)) =
                    (&args[0], &args[1], &args[2])
                {
                    if *st == 0 {
                        return Err(
                            RuntimeError::new("range() step cannot be zero".into()).at(span)
                        );
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
            if let (Some(s), Some(e), Some(st)) =
                (to_f64(&args[0]), to_f64(&args[1]), to_f64(&args[2]))
            {
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
        arr.write()
            .unwrap()
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
            .read()
            .unwrap()
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
    if args.len() == 3 || args.len() == 4 {
        if args.len() == 3 {
            if let Value::Map(m) = &args[2] {
                let map = m.read().unwrap();
                if let (Some(Value::String(cert)), Some(Value::String(key))) =
                    (map.get("cert"), map.get("key"))
                {
                    let tls_args = vec![
                        args[0].clone(),
                        args[1].clone(),
                        Value::String(cert.clone()),
                        Value::String(key.clone()),
                    ];
                    return builtin_serve_tls(ev, tls_args, span);
                }
            }
        } else if args.len() == 4 {
            return builtin_serve_tls(ev, args, span);
        }
    }
    expect_args("serve", 2, &args, span)?;

    let (port_val, host_str) = match &args[0] {
        Value::Map(m) => {
            let map = m.read().unwrap();
            let p = map.get("port").cloned().ok_or_else(|| {
                RuntimeError::new("serve config map must contain 'port'".into()).at(span)
            })?;
            let h = match map.get("host") {
                Some(Value::String(s)) => s.clone(),
                _ => "127.0.0.1".to_string(),
            };
            (p, h)
        }
        other => (other.clone(), "127.0.0.1".to_string()),
    };

    let port = match port_val {
        Value::Int(p) => {
            if !(1..=65535).contains(&p) {
                return Err(RuntimeError::new(
                    "Port must be a whole number between 1 and 65535".into(),
                )
                .at(span));
            }
            p as u16
        }
        Value::Float(p) | Value::Number(p) => {
            if p.fract() != 0.0 || !(1.0..=65535.0).contains(&p) {
                return Err(RuntimeError::new(
                    "Port must be a whole number between 1 and 65535".into(),
                )
                .at(span));
            }
            p as u16
        }
        _ => {
            return Err(RuntimeError::new(
                "First argument to serve must be a port number or config map".into(),
            )
            .at(span));
        }
    };

    let handler = args[1].clone();
    if !matches!(
        handler,
        Value::Function { .. } | Value::Builtin { .. } | Value::Map(_)
    ) {
        return Err(RuntimeError::new(
            "Second argument to serve must be a function or route map".into(),
        )
        .at(span));
    }

    use std::net::TcpListener;

    let bind_addr = format!("{}:{}", host_str, port);
    let listener = TcpListener::bind(&bind_addr).map_err(|e| {
        RuntimeError::new(format!("Failed to bind to {}: {}", bind_addr, e)).at(span)
    })?;

    let display_host = if host_str == "0.0.0.0" {
        "localhost"
    } else {
        &host_str
    };
    println!("Server running on http://{}:{}", display_host, port);

    let env_root = ev.global_env.clone();
    let handler_root = handler.clone();
    const MAX_CONCURRENT_CONNECTIONS: usize = 128;
    let active_connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for mut stream in listener.incoming().flatten() {
        let active = active_connections.clone();
        if active.load(std::sync::atomic::Ordering::Relaxed) >= MAX_CONCURRENT_CONNECTIONS {
            use std::io::Write;
            let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nRetry-After: 1\r\n\r\nServer Busy\r\n");
            let _ = stream.flush();
            continue;
        }
        active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let env_clone = env_root.clone();
        let handler_clone = handler_root.clone();
        std::thread::spawn(move || {
            struct ConnGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for ConnGuard {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            let _guard = ConnGuard(active);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let _ = handle_http_io(&mut stream, &env_clone, &handler_clone, span);
        });
    }
    Ok(Value::Null)
}

fn load_tls_config(
    cert_path: &str,
    key_path: &str,
    span: Span,
) -> Result<Arc<rustls::ServerConfig>, RuntimeError> {
    let cert_file = std::fs::File::open(cert_path).map_err(|e| {
        RuntimeError::new(format!(
            "Failed to open certificate file '{}': {}",
            cert_path, e
        ))
        .at(span)
    })?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                RuntimeError::new(format!("Failed to parse certificates: {}", e)).at(span)
            })?;

    if certs.is_empty() {
        return Err(
            RuntimeError::new(format!("No certificates found in '{}'", cert_path)).at(span),
        );
    }

    let key_file = std::fs::File::open(key_path).map_err(|e| {
        RuntimeError::new(format!(
            "Failed to open private key file '{}': {}",
            key_path, e
        ))
        .at(span)
    })?;
    let mut key_reader = std::io::BufReader::new(key_file);
    let key: rustls::pki_types::PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| RuntimeError::new(format!("Failed to parse private key: {}", e)).at(span))?
            .ok_or_else(|| {
                RuntimeError::new(format!("No private key found in '{}'", key_path)).at(span)
            })?;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| RuntimeError::new(format!("TLS configuration error: {}", e)).at(span))?;

    Ok(Arc::new(config))
}

fn builtin_serve_tls(
    ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let (port_val, handler, cert_path, key_path, host) = if args.len() == 2 {
        if let Value::Map(map_lock) = &args[0] {
            let map = map_lock.read().unwrap();
            let port = map.get("port").cloned().ok_or_else(|| {
                RuntimeError::new("serve_tls config map must contain 'port'".into()).at(span)
            })?;
            let host = match map.get("host") {
                Some(Value::String(s)) => s.clone(),
                _ => "127.0.0.1".to_string(),
            };
            let cert = match map.get("cert") {
                Some(Value::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(
                        "serve_tls config map must contain 'cert' path string".into(),
                    )
                    .at(span));
                }
            };
            let key = match map.get("key") {
                Some(Value::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(
                        "serve_tls config map must contain 'key' path string".into(),
                    )
                    .at(span));
                }
            };
            (port, args[1].clone(), cert, key, host)
        } else {
            return Err(RuntimeError::new(
                "serve_tls with 2 arguments expects (config_map, handler)".into(),
            )
            .at(span));
        }
    } else if args.len() == 4 {
        let cert = match &args[2] {
            Value::String(s) => s.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "Third argument to serve_tls must be certificate path string".into(),
                )
                .at(span));
            }
        };
        let key = match &args[3] {
            Value::String(s) => s.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "Fourth argument to serve_tls must be private key path string".into(),
                )
                .at(span));
            }
        };
        (
            args[0].clone(),
            args[1].clone(),
            cert,
            key,
            "127.0.0.1".to_string(),
        )
    } else {
        return Err(RuntimeError::new(format!(
            "'serve_tls' expects 2 arguments (config_map, handler) or 4 arguments (port, handler, cert, key), got {}",
            args.len()
        )).at(span));
    };

    let port = match port_val {
        Value::Int(p) => {
            if !(1..=65535).contains(&p) {
                return Err(RuntimeError::new(
                    "Port must be a whole number between 1 and 65535".into(),
                )
                .at(span));
            }
            p as u16
        }
        Value::Float(p) | Value::Number(p) => {
            if p.fract() != 0.0 || !(1.0..=65535.0).contains(&p) {
                return Err(RuntimeError::new(
                    "Port must be a whole number between 1 and 65535".into(),
                )
                .at(span));
            }
            p as u16
        }
        _ => {
            return Err(RuntimeError::new(
                "First argument to serve_tls must be a port number".into(),
            )
            .at(span));
        }
    };

    if !matches!(
        handler,
        Value::Function { .. } | Value::Builtin { .. } | Value::Map(_)
    ) {
        return Err(
            RuntimeError::new("serve_tls handler must be a function or route map".into()).at(span),
        );
    }

    let tls_config = load_tls_config(&cert_path, &key_path, span)?;

    use std::net::TcpListener;
    let bind_addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&bind_addr).map_err(|e| {
        RuntimeError::new(format!("Failed to bind to {}: {}", bind_addr, e)).at(span)
    })?;

    let display_host = if host == "0.0.0.0" {
        "localhost"
    } else {
        &host
    };
    println!("HTTPS Server running on https://{}:{}", display_host, port);

    let env_root = ev.global_env.clone();
    let handler_root = handler.clone();
    const MAX_CONCURRENT_CONNECTIONS: usize = 128;
    let active_connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for mut tcp_stream in listener.incoming().flatten() {
        let active = active_connections.clone();
        if active.load(std::sync::atomic::Ordering::Relaxed) >= MAX_CONCURRENT_CONNECTIONS {
            continue;
        }
        active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let env_clone = env_root.clone();
        let handler_clone = handler_root.clone();
        let config_clone = tls_config.clone();
        std::thread::spawn(move || {
            struct ConnGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for ConnGuard {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            let _guard = ConnGuard(active);
            let _ = tcp_stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            if let Ok(mut conn) = rustls::ServerConnection::new(config_clone) {
                let mut tls_stream = rustls::Stream::new(&mut conn, &mut tcp_stream);
                let _ = handle_http_io(&mut tls_stream, &env_clone, &handler_clone, span);
            }
        });
    }
    Ok(Value::Null)
}

pub fn match_route_pattern(pattern: &str, path: &str) -> Option<IndexMap<String, Value>> {
    let pattern_norm = pattern.split('?').next().unwrap_or(pattern);
    let path_norm = path.split('?').next().unwrap_or(path);

    let pat_trimmed = pattern_norm.trim_matches('/');
    let path_trimmed = path_norm.trim_matches('/');

    if pat_trimmed.is_empty() && path_trimmed.is_empty() {
        return Some(IndexMap::new());
    }

    let pat_parts: Vec<&str> = if pat_trimmed.is_empty() {
        vec![]
    } else {
        pat_trimmed.split('/').collect()
    };
    let path_parts: Vec<&str> = if path_trimmed.is_empty() {
        vec![]
    } else {
        path_trimmed.split('/').collect()
    };

    if pat_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = IndexMap::new();
    for (pat_seg, path_seg) in pat_parts.iter().zip(path_parts.iter()) {
        if let Some(param_name) = pat_seg.strip_prefix(':') {
            params.insert(param_name.to_string(), Value::String(path_seg.to_string()));
        } else if let Some(param_name) = pat_seg.strip_prefix('*') {
            let name = if param_name.is_empty() {
                "wildcard"
            } else {
                param_name
            };
            params.insert(name.to_string(), Value::String(path_seg.to_string()));
        } else if pat_seg != path_seg {
            return None;
        }
    }
    Some(params)
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex_str = format!("{}{}", c1, c2);
                if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                    bytes.push(byte);
                    continue;
                }
            }
            bytes.push(b'%');
            if let Some(c1) = h1 {
                bytes.extend_from_slice(c1.encode_utf8(&mut [0; 4]).as_bytes());
            }
            if let Some(c2) = h2 {
                bytes.extend_from_slice(c2.encode_utf8(&mut [0; 4]).as_bytes());
            }
        } else if ch == '+' {
            bytes.push(b' ');
        } else {
            bytes.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

fn parse_query_params(query: &str) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(url_decode(k), Value::String(url_decode(v)));
        } else {
            map.insert(url_decode(pair), Value::String("".to_string()));
        }
    }
    map
}

fn dispatch_request(
    ev: &mut Evaluator,
    handler: &Value,
    method: &str,
    path: &str,
    query: &str,
    query_params: &IndexMap<String, Value>,
    headers: &IndexMap<String, Value>,
    body_str: &str,
    span: Span,
) -> Result<Value, RuntimeError> {
    match handler {
        Value::Map(route_map) => {
            let routes = route_map.read().unwrap();
            for (route_pattern, handler_fn) in routes.iter() {
                let (expected_method, path_pattern) =
                    if let Some((m, p)) = route_pattern.split_once(' ') {
                        (Some(m.trim().to_uppercase()), p.trim())
                    } else {
                        (None, route_pattern.as_str())
                    };

                if let Some(exp_m) = expected_method {
                    if exp_m != method {
                        continue;
                    }
                }

                if let Some(params) = match_route_pattern(path_pattern, path) {
                    let mut req_map = IndexMap::new();
                    req_map.insert("method".to_string(), Value::String(method.to_string()));
                    req_map.insert("path".to_string(), Value::String(path.to_string()));
                    req_map.insert("query".to_string(), Value::String(query.to_string()));
                    req_map.insert(
                        "queryParams".to_string(),
                        Value::Map(Arc::new(RwLock::new(query_params.clone()))),
                    );
                    req_map.insert(
                        "query_params".to_string(),
                        Value::Map(Arc::new(RwLock::new(query_params.clone()))),
                    );
                    req_map.insert(
                        "params".to_string(),
                        Value::Map(Arc::new(RwLock::new(params))),
                    );
                    req_map.insert(
                        "headers".to_string(),
                        Value::Map(Arc::new(RwLock::new(headers.clone()))),
                    );
                    req_map.insert("body".to_string(), Value::String(body_str.to_string()));

                    let req_val = Value::Map(Arc::new(RwLock::new(req_map)));
                    return ev.call_value(handler_fn, vec![req_val], span);
                }
            }

            // No route matched
            let mut not_found_map = IndexMap::new();
            not_found_map.insert("status".to_string(), Value::Int(404));
            not_found_map.insert(
                "body".to_string(),
                Value::String(format!("Route '{} {}' not found", method, path)),
            );
            Ok(Value::Map(Arc::new(RwLock::new(not_found_map))))
        }
        _ => {
            let mut req_map = IndexMap::new();
            req_map.insert("method".to_string(), Value::String(method.to_string()));
            req_map.insert("path".to_string(), Value::String(path.to_string()));
            req_map.insert("query".to_string(), Value::String(query.to_string()));
            req_map.insert(
                "queryParams".to_string(),
                Value::Map(Arc::new(RwLock::new(query_params.clone()))),
            );
            req_map.insert(
                "query_params".to_string(),
                Value::Map(Arc::new(RwLock::new(query_params.clone()))),
            );
            req_map.insert(
                "params".to_string(),
                Value::Map(Arc::new(RwLock::new(IndexMap::new()))),
            );
            req_map.insert(
                "headers".to_string(),
                Value::Map(Arc::new(RwLock::new(headers.clone()))),
            );
            req_map.insert("body".to_string(), Value::String(body_str.to_string()));

            let req_val = Value::Map(Arc::new(RwLock::new(req_map)));
            ev.call_value(handler, vec![req_val], span)
        }
    }
}

fn handle_http_io<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    env_root: &Arc<RwLock<Environment>>,
    handler: &Value,
    span: Span,
) -> Result<(), ()> {
    let mut request_count = 0;
    const MAX_REQUESTS: usize = 100;
    const MAX_HEADER_SIZE: usize = 65536; // 64 KiB
    const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MiB
    let mut stream_buf: Vec<u8> = Vec::with_capacity(16384);

    while request_count < MAX_REQUESTS {
        // Read until headers delimiter "\r\n\r\n" or "\n\n"
        let (header_end, header_delim_len) = loop {
            if let Some(pos) = stream_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break (pos, 4);
            }
            if let Some(pos) = stream_buf.windows(2).position(|w| w == b"\n\n") {
                break (pos, 2);
            }
            if stream_buf.len() > MAX_HEADER_SIZE {
                let _ = stream.write_all(
                    b"HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n",
                );
                let _ = stream.flush();
                return Ok(());
            }
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk) {
                Ok(0) => return Ok(()), // Client closed connection / EOF
                Ok(n) => stream_buf.extend_from_slice(&chunk[..n]),
                Err(_) => return Ok(()),
            }
        };

        request_count += 1;

        let head_bytes = &stream_buf[..header_end];
        let head_str = match std::str::from_utf8(head_bytes) {
            Ok(s) => s,
            Err(_) => {
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
                let _ = stream.flush();
                return Ok(());
            }
        };

        let mut lines = head_str.lines();
        let request_line = match lines.next() {
            Some(l) => l,
            None => break,
        };
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            break;
        }

        let method = parts[0].to_uppercase();
        let full_path = parts[1].to_string();
        let http_version = if parts.len() >= 3 {
            parts[2]
        } else {
            "HTTP/1.1"
        };

        let mut path = full_path.clone();
        let mut query = "".to_string();
        if let Some(idx) = full_path.find('?') {
            path = full_path[..idx].to_string();
            query = full_path[idx + 1..].to_string();
        }

        let mut headers = IndexMap::new();
        let mut client_close = http_version == "HTTP/1.0";
        let mut content_length: usize = 0;

        for line in lines {
            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim().to_lowercase();
                let val = line[idx + 1..].trim();
                if key == "connection" {
                    if val.eq_ignore_ascii_case("close") {
                        client_close = true;
                    } else if val.eq_ignore_ascii_case("keep-alive") {
                        client_close = false;
                    }
                } else if key == "content-length" {
                    content_length = val.parse::<usize>().unwrap_or(0);
                }
                headers.insert(key, Value::String(val.to_string()));
            }
        }

        if content_length > MAX_BODY_SIZE {
            let _ =
                stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n");
            let _ = stream.flush();
            return Ok(());
        }

        // Drain the headers from stream_buf
        let body_start = header_end + header_delim_len;
        stream_buf.drain(..body_start);

        // Read remaining body bytes according to Content-Length
        while stream_buf.len() < content_length {
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => stream_buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }

        let body_bytes: Vec<u8> = stream_buf
            .drain(..std::cmp::min(content_length, stream_buf.len()))
            .collect();
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        let query_params = parse_query_params(&query);

        let mut worker_eval = Evaluator::with_env(env_root.clone());
        let res = dispatch_request(
            &mut worker_eval,
            handler,
            &method,
            &path,
            &query,
            &query_params,
            &headers,
            &body_str,
            span,
        );

        let (status_code, status_text, content_type, custom_headers, resp_body) = match res {
            Ok(val) => format_http_response(val),
            Err(e) => {
                let err_msg = e.to_string();
                eprintln!("Handler error: {}", err_msg);
                (
                    500,
                    "Internal Server Error",
                    "text/plain".to_string(),
                    vec![],
                    err_msg,
                )
            }
        };

        let conn_header = if client_close || request_count >= MAX_REQUESTS {
            "close"
        } else {
            "keep-alive"
        };

        let mut response_head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: {}\r\n",
            status_code,
            status_text,
            content_type,
            resp_body.len(),
            conn_header
        );
        if conn_header == "keep-alive" {
            response_head.push_str("Keep-Alive: timeout=5, max=100\r\n");
        }
        for (k, v) in custom_headers {
            // CRLF injection prevention: skip headers containing control characters
            if k.contains('\r')
                || k.contains('\n')
                || k.contains('\0')
                || v.contains('\r')
                || v.contains('\n')
                || v.contains('\0')
            {
                continue;
            }
            if !k
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                continue;
            }
            response_head.push_str(&format!("{}: {}\r\n", k, v));
        }
        response_head.push_str("\r\n");

        if stream.write_all(response_head.as_bytes()).is_err() {
            break;
        }
        if stream.write_all(resp_body.as_bytes()).is_err() {
            break;
        }
        if stream.flush().is_err() {
            break;
        }

        if client_close {
            break;
        }
    }
    Ok(())
}

fn builtin_route_match(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("route_match", 2, &args, span)?;
    if let (Value::String(pattern), Value::String(path)) = (&args[0], &args[1]) {
        if let Some(params) = match_route_pattern(pattern, path) {
            Ok(Value::Map(Arc::new(RwLock::new(params))))
        } else {
            Ok(Value::Null)
        }
    } else {
        Err(RuntimeError::new("route_match expects (string pattern, string path)".into()).at(span))
    }
}

fn format_http_response(val: Value) -> (u16, &'static str, String, Vec<(String, String)>, String) {
    match val {
        Value::Map(m) => {
            let map = m.read().unwrap();
            let status_num = map
                .get("status")
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
                            serde_json::to_string(&body_val.to_json())
                                .unwrap_or_else(|_| "{}".to_string()),
                        ),
                        Value::String(s) => ("text/html".to_string(), s.clone()),
                        other => ("text/plain".to_string(), other.to_display()),
                    };
                    return (code, text, ct, custom_headers, body_str);
                } else {
                    let json_str = serde_json::to_string(&Value::Map(m.clone()).to_json())
                        .unwrap_or_else(|_| "{}".to_string());
                    return (
                        code,
                        text,
                        "application/json".to_string(),
                        custom_headers,
                        json_str,
                    );
                }
            }

            let body_str = serde_json::to_string(&Value::Map(m.clone()).to_json())
                .unwrap_or_else(|_| "{}".to_string());
            (200, "OK", "application/json".to_string(), vec![], body_str)
        }
        Value::Array(_) => {
            let body_str =
                serde_json::to_string(&val.to_json()).unwrap_or_else(|_| "[]".to_string());
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
            Err(e) => {
                Err(RuntimeError::new(format!("Failed to read file '{}': {}", path, e)).at(span))
            }
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
            Err(e) => Err(
                RuntimeError::new(format!("Failed to write to file '{}': {}", path, e)).at(span),
            ),
        }
    } else {
        Err(RuntimeError::new("write expects (string path, string content)".into()).at(span))
    }
}

fn builtin_fetch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("fetch", 1, &args, span)?;
    if let Value::String(url) = &args[0] {
        match reqwest::blocking::get(url) {
            Ok(response) => match response.text() {
                Ok(text) => Ok(Value::String(text)),
                Err(e) => Err(RuntimeError::new(format!(
                    "Failed to read response from '{}': {}",
                    url, e
                ))
                .at(span)),
            },
            Err(e) => Err(RuntimeError::new(format!("Failed to fetch '{}': {}", url, e)).at(span)),
        }
    } else {
        Err(RuntimeError::new("fetch expects a string url".into()).at(span))
    }
}

fn builtin_time(
    _eval: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
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

fn builtin_exec(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
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

fn builtin_spawn(
    eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("spawn", 1, &args, span)?;
    let func = args[0].clone();
    let env = eval.global_env.clone();
    let handle = std::thread::spawn(move || {
        let mut spawn_eval = Evaluator::with_env(env);
        spawn_eval.call_value(&func, vec![], span)
    });
    Ok(Value::Task(Arc::new(std::sync::Mutex::new(Some(handle)))))
}

fn builtin_join(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
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
        other => Err(RuntimeError::new(format!(
            "join() expects a task, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn builtin_channel(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let capacity = match args.len() {
        0 => None,
        1 => match args[0] {
            Value::Int(n) => {
                if n < 0 {
                    return Err(
                        RuntimeError::new("channel capacity must be non-negative".into()).at(span),
                    );
                }
                Some(n as usize)
            }
            Value::Float(n) | Value::Number(n) => {
                if n < 0.0 {
                    return Err(
                        RuntimeError::new("channel capacity must be non-negative".into()).at(span),
                    );
                }
                Some(n as usize)
            }
            Value::Null => None,
            _ => return Err(RuntimeError::new("channel expects integer capacity".into()).at(span)),
        },
        _ => return Err(RuntimeError::new("channel expects 0 or 1 argument".into()).at(span)),
    };
    Ok(Value::Channel(crate::channel::Channel::new(capacity)))
}

fn builtin_send(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("send", 2, &args, span)?;
    if let Value::Channel(ch) = &args[0] {
        ch.send(args[1].clone(), span)?;
        Ok(Value::Null)
    } else {
        Err(RuntimeError::new("send() expects a channel as first argument".into()).at(span))
    }
}

fn builtin_recv(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "recv() expects 1 or 2 arguments: recv(channel, [timeout_ms])".into(),
        )
        .at(span));
    }
    if let Value::Channel(ch) = &args[0] {
        let timeout = if args.len() == 2 {
            match args[1] {
                Value::Int(ms) if ms >= 0 => Some(ms as u64),
                Value::Float(ms) | Value::Number(ms) if ms >= 0.0 => Some(ms as u64),
                Value::Null => None,
                _ => {
                    return Err(RuntimeError::new(
                        "recv() timeout must be non-negative integer".into(),
                    )
                    .at(span));
                }
            }
        } else {
            None
        };
        ch.recv(timeout, span)
    } else {
        Err(RuntimeError::new("recv() expects a channel as first argument".into()).at(span))
    }
}

fn builtin_try_recv(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("try_recv", 1, &args, span)?;
    if let Value::Channel(ch) = &args[0] {
        ch.try_recv(span)
    } else {
        Err(RuntimeError::new("try_recv() expects a channel as first argument".into()).at(span))
    }
}

fn builtin_close(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("close", 1, &args, span)?;
    if let Value::Channel(ch) = &args[0] {
        ch.close();
        Ok(Value::Null)
    } else {
        Err(RuntimeError::new("close() expects a channel as first argument".into()).at(span))
    }
}

fn builtin_assert(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "'assert' expects 1 or 2 arguments (condition, [message])".into(),
        )
        .at(span));
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

fn builtin_assert_eq(
    _eval: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "'assert_eq' expects 2 or 3 arguments (actual, expected, [message])".into(),
        )
        .at(span));
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
        ))
        .at(span));
    }
    Ok(Value::Null)
}

fn builtin_map(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("map", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        let items: Vec<Value> = arr.read().unwrap().clone();
        let func = &args[1];
        let mut new_arr = Vec::with_capacity(items.len());
        for item in items {
            let mapped = ev.call_value(func, vec![item], span)?;
            new_arr.push(mapped);
        }
        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
            new_arr,
        ))))
    } else {
        Err(RuntimeError::new("First argument to map must be an array".into()).at(span))
    }
}

fn builtin_filter(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("filter", 2, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        let items: Vec<Value> = arr.read().unwrap().clone();
        let func = &args[1];
        let mut new_arr = Vec::new();
        for item in items {
            let cond = ev.call_value(func, vec![item.clone()], span)?;
            if cond.is_truthy() {
                new_arr.push(item);
            }
        }
        Ok(Value::Array(std::sync::Arc::new(std::sync::RwLock::new(
            new_arr,
        ))))
    } else {
        Err(RuntimeError::new("First argument to filter must be an array".into()).at(span))
    }
}
