use crate::actor::{
    ActorError, ActorRef, ActorRegistry, ChildSpec, RestartPolicy, RestartStrategy,
    SupervisorConfig, SupervisorRef,
};
use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use indexmap::IndexMap;
use ring::rand::SecureRandom;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

pub fn load_std_module(name: &str, span: Span) -> Result<Value, RuntimeError> {
    let clean_name = name
        .strip_prefix("std:")
        .or_else(|| name.strip_prefix("std::"))
        .unwrap_or(name);

    match clean_name {
        "fs" => Ok(build_fs_module()),
        "path" => Ok(build_path_module()),
        "sys" => Ok(build_sys_module()),
        "time" => Ok(build_time_module()),
        "crypto" => Ok(build_crypto_module()),
        "codec" => Ok(build_codec_module()),
        "regex" => Ok(build_regex_module()),
        "http" => Ok(build_http_module()),
        "env" => Ok(build_env_module()),
        "cli" | "args" => Ok(build_cli_module()),
        "collections" | "set" => Ok(build_collections_module()),
        "ffi" => Ok(build_ffi_module()),
        "csv" => Ok(build_csv_module()),
        "sqlite" | "sql" => Ok(build_sqlite_module()),
        "unicode" => Ok(build_unicode_module()),
        "actor" | "actors" => Ok(build_actor_module()),
        "actor:cluster" | "cluster" => Ok(build_cluster_module()),
        "async" => Ok(build_async_module()),
        "test" => Ok(build_test_module()),
        _ => Err(RuntimeError::new(format!(
            "Unknown standard library module 'std:{}'. Available: std:fs, std:path, std:sys, std:time, std:crypto, std:codec, std:regex, std:http, std:env, std:cli, std:collections, std:ffi, std:csv, std:sqlite, std:unicode, std:actor, std:actor:cluster, std:async, std:test",
            clean_name
        ))
        .at(span)),
    }
}

fn make_map(entries: Vec<(&str, crate::value::BuiltinFn)>) -> Value {
    let mut map = IndexMap::new();
    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: name.to_string(),
                func,
            },
        );
    }
    Value::Map(Arc::new(RwLock::new(map)))
}

// -------------------------------------------------------------
// std:fs
// -------------------------------------------------------------
fn build_fs_module() -> Value {
    make_map(vec![
        ("read", fs_read),
        ("write", fs_write),
        ("append", fs_append),
        ("exists", fs_exists),
        ("remove", fs_remove),
        ("mkdir", fs_mkdir),
        ("readDir", fs_read_dir),
        ("read_dir", fs_read_dir),
        ("isFile", fs_is_file),
        ("is_file", fs_is_file),
        ("isDir", fs_is_dir),
        ("is_dir", fs_is_dir),
        ("write_bytes", fs_write_bytes),
        ("writeBytes", fs_write_bytes),
        ("read_bytes", fs_read_bytes),
        ("readBytes", fs_read_bytes),
    ])
}

fn fs_read(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("read", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Value::String(content)),
            Err(e) => Err(
                RuntimeError::new(format!("fs.read: Failed to read '{}': {}", path, e)).at(span),
            ),
        }
    } else {
        Err(RuntimeError::new("fs.read expects a string path".into()).at(span))
    }
}

fn fs_write(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("write", 2, &args, span)?;
    if let (Value::String(path), Value::String(content)) = (&args[0], &args[1]) {
        match fs::write(path, content) {
            Ok(_) => Ok(Value::Bool(true)),
            Err(e) => Err(RuntimeError::new(format!(
                "fs.write: Failed to write '{}': {}",
                path, e
            ))
            .at(span)),
        }
    } else {
        Err(RuntimeError::new("fs.write expects (string path, string content)".into()).at(span))
    }
}

fn fs_append(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("append", 2, &args, span)?;
    use std::io::Write;
    if let (Value::String(path), Value::String(content)) = (&args[0], &args[1]) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| {
                RuntimeError::new(format!("fs.append: Failed to open '{}': {}", path, e)).at(span)
            })?;
        file.write_all(content.as_bytes()).map_err(|e| {
            RuntimeError::new(format!("fs.append: Failed to write '{}': {}", path, e)).at(span)
        })?;
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new("fs.append expects (string path, string content)".into()).at(span))
    }
}

fn fs_write_bytes(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("write_bytes", 2, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.clone(),
        _ => {
            return Err(RuntimeError::new(
                "fs.write_bytes expects path string as first argument".into(),
            )
            .at(span));
        }
    };
    let bytes: Vec<u8> = match &args[1] {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut buf = Vec::with_capacity(guard.len());
            for (idx, item) in guard.iter().enumerate() {
                match item {
                    Value::Int(n) => {
                        if *n < 0 || *n > 255 {
                            return Err(RuntimeError::new(format!(
                                "fs.write_bytes: byte at index {} out of range [0, 255]: {}",
                                idx, n
                            ))
                            .at(span));
                        }
                        buf.push(*n as u8);
                    }
                    Value::Float(f) => {
                        let n = *f as i64;
                        if n < 0 || n > 255 {
                            return Err(RuntimeError::new(format!(
                                "fs.write_bytes: byte at index {} out of range [0, 255]: {}",
                                idx, f
                            ))
                            .at(span));
                        }
                        buf.push(n as u8);
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "fs.write_bytes: expected integer byte at index {}, got {}",
                            idx,
                            other.type_name()
                        ))
                        .at(span));
                    }
                }
            }
            buf
        }
        Value::String(s) => s.as_bytes().to_vec(),
        other => {
            return Err(RuntimeError::new(format!(
                "fs.write_bytes expects byte array or string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    match fs::write(&path, &bytes) {
        Ok(_) => Ok(Value::Bool(true)),
        Err(e) => Err(RuntimeError::new(format!(
            "fs.write_bytes: Failed to write '{}': {}",
            path, e
        ))
        .at(span)),
    }
}

fn fs_read_bytes(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("read_bytes", 1, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("fs.read_bytes expects path string".into()).at(span)),
    };
    match fs::read(&path) {
        Ok(bytes) => {
            let items: Vec<Value> = bytes.into_iter().map(|b| Value::Int(b as i64)).collect();
            Ok(Value::Array(Arc::new(RwLock::new(items))))
        }
        Err(e) => Err(RuntimeError::new(format!(
            "fs.read_bytes: Failed to read '{}': {}",
            path, e
        ))
        .at(span)),
    }
}

fn fs_exists(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("exists", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        Ok(Value::Bool(Path::new(path).exists()))
    } else {
        Err(RuntimeError::new("fs.exists expects a string path".into()).at(span))
    }
}

fn fs_remove(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("remove", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        let p = Path::new(path);
        let res = if p.is_dir() {
            fs::remove_dir_all(p)
        } else {
            fs::remove_file(p)
        };
        match res {
            Ok(_) => Ok(Value::Bool(true)),
            Err(e) => Err(RuntimeError::new(format!(
                "fs.remove: Failed to delete '{}': {}",
                path, e
            ))
            .at(span)),
        }
    } else {
        Err(RuntimeError::new("fs.remove expects a string path".into()).at(span))
    }
}

fn fs_mkdir(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("mkdir", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        match fs::create_dir_all(path) {
            Ok(_) => Ok(Value::Bool(true)),
            Err(e) => Err(RuntimeError::new(format!(
                "fs.mkdir: Failed to create '{}': {}",
                path, e
            ))
            .at(span)),
        }
    } else {
        Err(RuntimeError::new("fs.mkdir expects a string path".into()).at(span))
    }
}

fn fs_read_dir(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("readDir", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        match fs::read_dir(path) {
            Ok(entries) => {
                let mut list = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    list.push(Value::String(name));
                }
                list.sort_by(|a, b| a.to_display().cmp(&b.to_display()));
                Ok(Value::Array(Arc::new(RwLock::new(list))))
            }
            Err(e) => Err(RuntimeError::new(format!(
                "fs.readDir: Failed to read directory '{}': {}",
                path, e
            ))
            .at(span)),
        }
    } else {
        Err(RuntimeError::new("fs.readDir expects a string path".into()).at(span))
    }
}

fn fs_is_file(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("isFile", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        Ok(Value::Bool(Path::new(path).is_file()))
    } else {
        Err(RuntimeError::new("fs.isFile expects a string path".into()).at(span))
    }
}

fn fs_is_dir(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("isDir", 1, &args, span)?;
    if let Value::String(path) = &args[0] {
        Ok(Value::Bool(Path::new(path).is_dir()))
    } else {
        Err(RuntimeError::new("fs.isDir expects a string path".into()).at(span))
    }
}

// -------------------------------------------------------------
// std:path
// -------------------------------------------------------------
fn build_path_module() -> Value {
    make_map(vec![
        ("join", path_join),
        ("basename", path_basename),
        ("dirname", path_dirname),
        ("ext", path_ext),
        ("isAbs", path_is_abs),
        ("is_abs", path_is_abs),
    ])
}

fn path_join(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Ok(Value::String("".to_string()));
    }
    let mut pb = std::path::PathBuf::new();
    for arg in args {
        if let Value::String(s) = arg {
            pb.push(s);
        } else {
            return Err(RuntimeError::new("path.join expects string components".into()).at(span));
        }
    }
    Ok(Value::String(pb.to_string_lossy().to_string()))
}

fn path_basename(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("basename", 1, &args, span)?;
    if let Value::String(p) = &args[0] {
        let name = Path::new(p)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(Value::String(name))
    } else {
        Err(RuntimeError::new("path.basename expects a string path".into()).at(span))
    }
}

fn path_dirname(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("dirname", 1, &args, span)?;
    if let Value::String(p) = &args[0] {
        let parent = Path::new(p)
            .parent()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(Value::String(parent))
    } else {
        Err(RuntimeError::new("path.dirname expects a string path".into()).at(span))
    }
}

fn path_ext(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ext", 1, &args, span)?;
    if let Value::String(p) = &args[0] {
        let ext = Path::new(p)
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(Value::String(ext))
    } else {
        Err(RuntimeError::new("path.ext expects a string path".into()).at(span))
    }
}

fn path_is_abs(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("isAbs", 1, &args, span)?;
    if let Value::String(p) = &args[0] {
        Ok(Value::Bool(Path::new(p).is_absolute()))
    } else {
        Err(RuntimeError::new("path.isAbs expects a string path".into()).at(span))
    }
}

// -------------------------------------------------------------
// std:sys
// -------------------------------------------------------------
fn build_sys_module() -> Value {
    make_map(vec![
        ("args", sys_args),
        ("env", sys_env),
        ("getEnv", sys_env),
        ("get_env", sys_env),
        ("setEnv", sys_set_env),
        ("set_env", sys_set_env),
        ("platform", sys_platform),
        ("arch", sys_arch),
        ("cwd", sys_cwd),
        ("exit", sys_exit),
        ("on_signal", sys_on_signal),
        ("onSignal", sys_on_signal),
        ("trigger_signal", sys_trigger_signal),
        ("triggerSignal", sys_trigger_signal),
        ("clear_signals", sys_clear_signals),
        ("clearSignals", sys_clear_signals),
        ("poll_signals", sys_poll_signals),
        ("pollSignals", sys_poll_signals),
        ("signals", sys_signals),
    ])
}

fn sys_args(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let args: Vec<Value> = std::env::args().map(Value::String).collect();
    Ok(Value::Array(Arc::new(RwLock::new(args))))
}

fn sys_env(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("env", 1, &args, span)?;
    if let Value::String(key) = &args[0] {
        match std::env::var(key) {
            Ok(v) => Ok(Value::String(v)),
            Err(_) => Ok(Value::Null),
        }
    } else {
        Err(RuntimeError::new("sys.env expects a string variable name".into()).at(span))
    }
}

fn sys_set_env(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("setEnv", 2, &args, span)?;
    if let (Value::String(k), Value::String(v)) = (&args[0], &args[1]) {
        unsafe {
            std::env::set_var(k, v);
        }
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new("sys.setEnv expects (string key, string value)".into()).at(span))
    }
}

fn sys_platform(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    Ok(Value::String(std::env::consts::OS.to_string()))
}

fn sys_arch(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    Ok(Value::String(std::env::consts::ARCH.to_string()))
}

fn sys_cwd(_ev: &mut Evaluator, _args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    match std::env::current_dir() {
        Ok(p) => Ok(Value::String(p.to_string_lossy().to_string())),
        Err(e) => {
            Err(RuntimeError::new(format!("sys.cwd: Failed to get current dir: {}", e)).at(span))
        }
    }
}

fn sys_exit(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let code = if args.is_empty() {
        0
    } else if let Value::Int(c) = args[0] {
        c as i32
    } else if let Value::Number(c) = args[0] {
        c as i32
    } else {
        return Err(RuntimeError::new("sys.exit expects an integer exit code".into()).at(span));
    };
    std::process::exit(code);
}

fn get_signal_handlers() -> &'static RwLock<IndexMap<String, Vec<Value>>> {
    static HANDLERS: OnceLock<RwLock<IndexMap<String, Vec<Value>>>> = OnceLock::new();
    HANDLERS.get_or_init(|| RwLock::new(IndexMap::new()))
}

fn get_pending_signals() -> &'static Mutex<Vec<String>> {
    static PENDING: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

fn init_os_signal_hook() {
    static HOOK: OnceLock<()> = OnceLock::new();
    HOOK.get_or_init(|| {
        let _ = ctrlc::set_handler(move || {
            if let Ok(mut pending) = get_pending_signals().lock() {
                pending.push("SIGINT".to_string());
                pending.push("SIGTERM".to_string());
            }
        });
    });
}

fn normalize_signal_name(sig: &str) -> String {
    let clean = sig.trim().to_uppercase();
    match clean.as_str() {
        "INT" | "SIGINT" | "INTERRUPT" => "SIGINT".to_string(),
        "TERM" | "SIGTERM" | "TERMINATE" | "TERMINATION" => "SIGTERM".to_string(),
        "HUP" | "SIGHUP" | "HANGUP" => "SIGHUP".to_string(),
        other => {
            if !other.starts_with("SIG") {
                format!("SIG{}", other)
            } else {
                other.to_string()
            }
        }
    }
}

fn sys_on_signal(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("on_signal", 2, &args, span)?;
    let sig_name = match &args[0] {
        Value::String(s) => normalize_signal_name(s),
        _ => {
            return Err(RuntimeError::new(
                "sys.on_signal expects signal name as string (e.g. 'SIGINT' or 'SIGTERM')".into(),
            )
            .at(span));
        }
    };

    let callback = args[1].clone();
    match &callback {
        Value::Function { .. }
        | Value::CompiledFunction(_)
        | Value::Closure(_)
        | Value::Builtin { .. }
        | Value::BoundMethod { .. } => {}
        _ => {
            return Err(RuntimeError::new(
                "sys.on_signal expects callable function or closure as second argument".into(),
            )
            .at(span));
        }
    }

    {
        let handlers = get_signal_handlers();
        let mut map = handlers.write().unwrap();
        map.entry(sig_name).or_default().push(callback);
    }

    init_os_signal_hook();

    Ok(Value::Bool(true))
}

fn sys_trigger_signal(
    ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("trigger_signal", 1, &args, span)?;
    let sig_name = match &args[0] {
        Value::String(s) => normalize_signal_name(s),
        _ => {
            return Err(RuntimeError::new(
                "sys.trigger_signal expects signal name as string".into(),
            )
            .at(span));
        }
    };

    let handlers_to_run = {
        let handlers = get_signal_handlers();
        let map = handlers.read().unwrap();
        map.get(&sig_name).cloned().unwrap_or_default()
    };

    if handlers_to_run.is_empty() {
        return Ok(Value::Bool(false));
    }

    let mut last_res = Value::Bool(true);
    for handler in handlers_to_run {
        let arity = match &handler {
            Value::Function { params, .. } => params.len(),
            Value::Closure(c) => c.function.arity,
            Value::CompiledFunction(f) => f.arity,
            _ => 1,
        };
        let call_args = if arity == 0 {
            vec![]
        } else {
            vec![Value::String(sig_name.clone())]
        };
        match ev.call_value(&handler, call_args, span) {
            Ok(v) => last_res = v,
            Err(e) if e.message.to_lowercase().contains("0 arguments") => {
                last_res = ev.call_value(&handler, vec![], span)?;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(last_res)
}

fn sys_clear_signals(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let handlers = get_signal_handlers();
    let mut map = handlers.write().unwrap();
    if args.is_empty() {
        map.clear();
    } else if let Value::String(sig) = &args[0] {
        let norm = normalize_signal_name(sig);
        map.shift_remove(&norm);
    } else {
        return Err(RuntimeError::new(
            "sys.clear_signals expects optional signal name as string".into(),
        )
        .at(span));
    }
    Ok(Value::Bool(true))
}

fn sys_poll_signals(
    ev: &mut Evaluator,
    _args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let pending: Vec<String> = {
        if let Ok(mut lock) = get_pending_signals().lock() {
            let drained: Vec<String> = lock.drain(..).collect();
            drained
        } else {
            Vec::new()
        }
    };

    let mut invoked_count: i64 = 0;
    for sig in pending {
        let handlers_to_run = {
            let handlers = get_signal_handlers();
            let map = handlers.read().unwrap();
            map.get(&sig).cloned().unwrap_or_default()
        };

        for handler in handlers_to_run {
            invoked_count += 1;
            let arity = match &handler {
                Value::Function { params, .. } => params.len(),
                Value::Closure(c) => c.function.arity,
                Value::CompiledFunction(f) => f.arity,
                _ => 1,
            };
            let call_args = if arity == 0 {
                vec![]
            } else {
                vec![Value::String(sig.clone())]
            };
            match ev.call_value(&handler, call_args, span) {
                Ok(_) => {}
                Err(e) if e.message.to_lowercase().contains("0 arguments") => {
                    let _ = ev.call_value(&handler, vec![], span);
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(Value::Int(invoked_count))
}

fn sys_signals(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let sigs = vec![
        Value::String("SIGINT".to_string()),
        Value::String("SIGTERM".to_string()),
        Value::String("SIGHUP".to_string()),
    ];
    Ok(Value::Array(Arc::new(RwLock::new(sigs))))
}

// -------------------------------------------------------------
// std:time
// -------------------------------------------------------------
fn build_time_module() -> Value {
    make_map(vec![
        ("now", time_now),
        ("nowMs", time_now_ms),
        ("now_ms", time_now_ms),
        ("sleep", time_sleep),
    ])
}

fn time_now(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(Value::Number(duration.as_secs_f64()))
    } else {
        Ok(Value::Number(0.0))
    }
}

fn time_now_ms(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(Value::Int(duration.as_millis() as i64))
    } else {
        Ok(Value::Int(0))
    }
}

fn time_sleep(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("sleep", 1, &args, span)?;
    let ms = match &args[0] {
        Value::Int(i) => {
            if *i < 0 {
                return Err(
                    RuntimeError::new("time.sleep expects non-negative duration".into()).at(span),
                );
            }
            *i as u64
        }
        Value::Float(f) | Value::Number(f) => {
            if *f < 0.0 || f.is_nan() || f.is_infinite() {
                return Err(RuntimeError::new(
                    "time.sleep expects non-negative finite duration".into(),
                )
                .at(span));
            }
            *f as u64
        }
        _ => {
            return Err(
                RuntimeError::new("time.sleep expects milliseconds as number".into()).at(span),
            );
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Ok(Value::Null)
}

// -------------------------------------------------------------
// std:crypto
// -------------------------------------------------------------
fn build_crypto_module() -> Value {
    make_map(vec![
        ("sha256", crypto_sha256),
        ("sha512", crypto_sha512),
        ("hmac_sha256", crypto_hmac_sha256),
        ("hmacSha256", crypto_hmac_sha256),
        ("hmac_sha512", crypto_hmac_sha512),
        ("hmacSha512", crypto_hmac_sha512),
        ("random_bytes", crypto_random_bytes),
        ("randomBytes", crypto_random_bytes),
        ("random_hex", crypto_random_hex),
        ("randomHex", crypto_random_hex),
        ("uuid", crypto_uuid),
        ("uuid_v4", crypto_uuid),
        ("uuidV4", crypto_uuid),
        ("random_uuid", crypto_uuid),
        ("randomUuid", crypto_uuid),
        ("siphash", crypto_siphash),
        ("siphash64", crypto_siphash),
        ("siphash_hex", crypto_siphash_hex),
        ("siphashHex", crypto_siphash_hex),
        ("encrypt_aes_gcm", crypto_encrypt_aes_gcm),
        ("encryptAesGcm", crypto_encrypt_aes_gcm),
        ("decrypt_aes_gcm", crypto_decrypt_aes_gcm),
        ("decryptAesGcm", crypto_decrypt_aes_gcm),
        ("encrypt_chacha20", crypto_encrypt_chacha20),
        ("encryptChacha20", crypto_encrypt_chacha20),
        ("encrypt_chacha20_poly1305", crypto_encrypt_chacha20),
        ("encryptChacha20Poly1305", crypto_encrypt_chacha20),
        ("decrypt_chacha20", crypto_decrypt_chacha20),
        ("decryptChacha20", crypto_decrypt_chacha20),
        ("decrypt_chacha20_poly1305", crypto_decrypt_chacha20),
        ("decryptChacha20Poly1305", crypto_decrypt_chacha20),
        ("pbkdf2_sha256", crypto_pbkdf2_sha256),
        ("pbkdf2Sha256", crypto_pbkdf2_sha256),
        ("pbkdf2_sha512", crypto_pbkdf2_sha512),
        ("pbkdf2Sha512", crypto_pbkdf2_sha512),
        ("hkdf_sha256", crypto_hkdf_sha256),
        ("hkdfSha256", crypto_hkdf_sha256),
        ("hkdf_sha512", crypto_hkdf_sha512),
        ("hkdfSha512", crypto_hkdf_sha512),
        ("timing_safe_equal", crypto_timing_safe_equal),
        ("timingSafeEqual", crypto_timing_safe_equal),
    ])
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{:02x}", b);
    }
    out
}

fn crypto_sha256(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("crypto.sha256", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        let digest = ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
        Ok(Value::String(bytes_to_hex(digest.as_ref())))
    } else {
        Err(RuntimeError::new("crypto.sha256 expects a string argument".into()).at(span))
    }
}

fn crypto_sha512(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("crypto.sha512", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        let digest = ring::digest::digest(&ring::digest::SHA512, s.as_bytes());
        Ok(Value::String(bytes_to_hex(digest.as_ref())))
    } else {
        Err(RuntimeError::new("crypto.sha512 expects a string argument".into()).at(span))
    }
}

fn crypto_hmac_sha256(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.hmac_sha256", 2, &args, span)?;
    if let (Value::String(key), Value::String(data)) = (&args[0], &args[1]) {
        let s_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
        let tag = ring::hmac::sign(&s_key, data.as_bytes());
        Ok(Value::String(bytes_to_hex(tag.as_ref())))
    } else {
        Err(
            RuntimeError::new("crypto.hmac_sha256 expects (string key, string data)".into())
                .at(span),
        )
    }
}

fn crypto_hmac_sha512(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.hmac_sha512", 2, &args, span)?;
    if let (Value::String(key), Value::String(data)) = (&args[0], &args[1]) {
        let s_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA512, key.as_bytes());
        let tag = ring::hmac::sign(&s_key, data.as_bytes());
        Ok(Value::String(bytes_to_hex(tag.as_ref())))
    } else {
        Err(
            RuntimeError::new("crypto.hmac_sha512 expects (string key, string data)".into())
                .at(span),
        )
    }
}

fn crypto_random_bytes(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.random_bytes", 1, &args, span)?;
    let len = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Float(n) | Value::Number(n) if *n >= 0.0 => *n as usize,
        _ => {
            return Err(RuntimeError::new(
                "crypto.random_bytes expects non-negative integer length".into(),
            )
            .at(span));
        }
    };
    let rng = ring::rand::SystemRandom::new();
    let mut buf = vec![0u8; len];
    rng.fill(&mut buf)
        .map_err(|e| RuntimeError::new(format!("crypto.random_bytes error: {}", e)).at(span))?;
    let list: Vec<Value> = buf.into_iter().map(|b| Value::Int(b as i64)).collect();
    Ok(Value::Array(Arc::new(RwLock::new(list))))
}

fn crypto_random_hex(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.random_hex", 1, &args, span)?;
    let len = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as usize,
        Value::Float(n) | Value::Number(n) if *n >= 0.0 => *n as usize,
        _ => {
            return Err(RuntimeError::new(
                "crypto.random_hex expects non-negative integer length".into(),
            )
            .at(span));
        }
    };
    let rng = ring::rand::SystemRandom::new();
    let mut buf = vec![0u8; len];
    rng.fill(&mut buf)
        .map_err(|e| RuntimeError::new(format!("crypto.random_hex error: {}", e)).at(span))?;
    Ok(Value::String(bytes_to_hex(&buf)))
}

fn crypto_uuid(_ev: &mut Evaluator, _args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let rng = ring::rand::SystemRandom::new();
    let mut buf = [0u8; 16];
    rng.fill(&mut buf)
        .map_err(|e| RuntimeError::new(format!("crypto.uuid error: {}", e)).at(span))?;
    buf[6] = (buf[6] & 0x0f) | 0x40; // RFC 4122 v4
    buf[8] = (buf[8] & 0x3f) | 0x80; // variant 1
    let uuid = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0],
        buf[1],
        buf[2],
        buf[3],
        buf[4],
        buf[5],
        buf[6],
        buf[7],
        buf[8],
        buf[9],
        buf[10],
        buf[11],
        buf[12],
        buf[13],
        buf[14],
        buf[15]
    );
    Ok(Value::String(uuid))
}

pub fn siphash_2_4(key: [u8; 16], data: &[u8]) -> u64 {
    let k0 = u64::from_le_bytes(key[0..8].try_into().unwrap());
    let k1 = u64::from_le_bytes(key[8..16].try_into().unwrap());

    let mut v0 = k0 ^ 0x736f6d6570736575;
    let mut v1 = k1 ^ 0x646f72616e646f6d;
    let mut v2 = k0 ^ 0x6c7967656e657261;
    let mut v3 = k1 ^ 0x7465646279746573;

    let len = data.len();
    let num_words = len / 8;
    for i in 0..num_words {
        let chunk = &data[i * 8..(i + 1) * 8];
        let m = u64::from_le_bytes(chunk.try_into().unwrap());
        v3 ^= m;
        for _ in 0..2 {
            sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        }
        v0 ^= m;
    }

    let remainder = &data[num_words * 8..];
    let mut b: u64 = ((len as u64) & 0xff) << 56;
    for (i, &byte) in remainder.iter().enumerate() {
        b |= (byte as u64) << (8 * i);
    }

    v3 ^= b;
    for _ in 0..2 {
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    }
    v0 ^= b;

    v2 ^= 0xff;
    for _ in 0..4 {
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    }

    v0 ^ v1 ^ v2 ^ v3
}

#[inline(always)]
fn sip_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);

    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;

    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;

    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}

fn parse_siphash_key(val: &Value) -> [u8; 16] {
    let mut key = [0u8; 16];
    match val {
        Value::String(s) => {
            let bytes = s.as_bytes();
            let copy_len = bytes.len().min(16);
            key[..copy_len].copy_from_slice(&bytes[..copy_len]);
        }
        Value::Int(n) => {
            let b = n.to_le_bytes();
            key[0..8].copy_from_slice(&b);
            key[8..16].copy_from_slice(&b);
        }
        Value::Float(f) | Value::Number(f) => {
            let b = f.to_bits().to_le_bytes();
            key[0..8].copy_from_slice(&b);
            key[8..16].copy_from_slice(&b);
        }
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (i, v) in guard.iter().take(16).enumerate() {
                if let Value::Int(b) = v {
                    key[i] = *b as u8;
                }
            }
        }
        _ => {}
    }
    key
}

fn crypto_siphash(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.siphash", 2, &args, span)?;
    let key = parse_siphash_key(&args[0]);
    let h = match &args[1] {
        Value::String(s) => siphash_2_4(key, s.as_bytes()),
        other => {
            let s = other.to_string();
            siphash_2_4(key, s.as_bytes())
        }
    };
    Ok(Value::Int(h as i64))
}

fn crypto_siphash_hex(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.siphash_hex", 2, &args, span)?;
    let key = parse_siphash_key(&args[0]);
    let h = match &args[1] {
        Value::String(s) => siphash_2_4(key, s.as_bytes()),
        other => {
            let s = other.to_string();
            siphash_2_4(key, s.as_bytes())
        }
    };
    Ok(Value::String(format!("{:016x}", h)))
}

fn crypto_value_to_bytes(v: &Value, name: &str, span: Span) -> Result<Vec<u8>, RuntimeError> {
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut out = Vec::with_capacity(guard.len());
            for (idx, item) in guard.iter().enumerate() {
                let byte_val = match item {
                    Value::Int(n) => *n,
                    Value::Float(f) => *f as i64,
                    Value::Number(n) => *n as i64,
                    _ => {
                        return Err(RuntimeError::new(format!(
                            "{}: expected integer byte in array at index {}, got {}",
                            name,
                            idx,
                            item.type_name()
                        ))
                        .at(span));
                    }
                };
                if !(0..=255).contains(&byte_val) {
                    return Err(RuntimeError::new(format!(
                        "{}: byte at index {} out of range [0, 255]: {}",
                        name, idx, byte_val
                    ))
                    .at(span));
                }
                out.push(byte_val as u8);
            }
            Ok(out)
        }
        _ => Err(RuntimeError::new(format!(
            "{} expects string or byte array, got {}",
            name,
            v.type_name()
        ))
        .at(span)),
    }
}

fn crypto_bytes_to_value(bytes: &[u8]) -> Value {
    let list: Vec<Value> = bytes.iter().map(|&b| Value::Int(b as i64)).collect();
    Value::Array(Arc::new(RwLock::new(list)))
}

fn crypto_make_encryption_result(ciphertext: &[u8], nonce: &[u8], tag: &[u8]) -> Value {
    let mut combined = Vec::with_capacity(ciphertext.len() + tag.len());
    combined.extend_from_slice(ciphertext);
    combined.extend_from_slice(tag);

    let mut map = IndexMap::new();
    map.insert("ciphertext".to_string(), crypto_bytes_to_value(ciphertext));
    map.insert("nonce".to_string(), crypto_bytes_to_value(nonce));
    map.insert("tag".to_string(), crypto_bytes_to_value(tag));
    map.insert("combined".to_string(), crypto_bytes_to_value(&combined));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn aead_encrypt(
    alg: &'static ring::aead::Algorithm,
    func_name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 3 || args.len() > 4 {
        return Err(RuntimeError::new(format!(
            "{} expects 3 or 4 arguments: (key, nonce, plaintext, [aad])",
            func_name
        ))
        .at(span));
    }

    let key_bytes = crypto_value_to_bytes(&args[0], &format!("{} key", func_name), span)?;
    if key_bytes.len() != 32 {
        return Err(RuntimeError::new(format!(
            "{}: key must be exactly 32 bytes (256-bit), got {} bytes",
            func_name,
            key_bytes.len()
        ))
        .at(span));
    }

    let nonce_bytes = match &args[1] {
        Value::Null => {
            let rng = ring::rand::SystemRandom::new();
            let mut buf = [0u8; 12];
            rng.fill(&mut buf).map_err(|e| {
                RuntimeError::new(format!(
                    "{}: random nonce generation error: {}",
                    func_name, e
                ))
                .at(span)
            })?;
            buf.to_vec()
        }
        v => {
            let bytes = crypto_value_to_bytes(v, &format!("{} nonce", func_name), span)?;
            if bytes.len() != 12 {
                return Err(RuntimeError::new(format!(
                    "{}: nonce must be exactly 12 bytes (96-bit), got {} bytes",
                    func_name,
                    bytes.len()
                ))
                .at(span));
            }
            bytes
        }
    };

    let plaintext_bytes =
        crypto_value_to_bytes(&args[2], &format!("{} plaintext", func_name), span)?;

    let aad_bytes = if args.len() >= 4 && !matches!(args[3], Value::Null) {
        crypto_value_to_bytes(&args[3], &format!("{} aad", func_name), span)?
    } else {
        Vec::new()
    };

    let unbound_key = ring::aead::UnboundKey::new(alg, &key_bytes).map_err(|e| {
        RuntimeError::new(format!("{}: key initialization failed: {}", func_name, e)).at(span)
    })?;
    let less_safe_key = ring::aead::LessSafeKey::new(unbound_key);
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|e| {
        RuntimeError::new(format!("{}: nonce initialization failed: {}", func_name, e)).at(span)
    })?;

    let mut in_out = plaintext_bytes;
    let tag = less_safe_key
        .seal_in_place_separate_tag(nonce, ring::aead::Aad::from(&aad_bytes), &mut in_out)
        .map_err(|e| {
            RuntimeError::new(format!("{}: encryption failed: {}", func_name, e)).at(span)
        })?;

    Ok(crypto_make_encryption_result(
        &in_out,
        &nonce_bytes,
        tag.as_ref(),
    ))
}

fn aead_decrypt(
    alg: &'static ring::aead::Algorithm,
    func_name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(RuntimeError::new(format!(
            "{} expects 3 to 5 arguments: (key, nonce, ciphertext, [tag], [aad])",
            func_name
        ))
        .at(span));
    }

    let key_bytes = crypto_value_to_bytes(&args[0], &format!("{} key", func_name), span)?;
    if key_bytes.len() != 32 {
        return Err(RuntimeError::new(format!(
            "{}: key must be exactly 32 bytes (256-bit), got {} bytes",
            func_name,
            key_bytes.len()
        ))
        .at(span));
    }

    let nonce_bytes = crypto_value_to_bytes(&args[1], &format!("{} nonce", func_name), span)?;
    if nonce_bytes.len() != 12 {
        return Err(RuntimeError::new(format!(
            "{}: nonce must be exactly 12 bytes (96-bit), got {} bytes",
            func_name,
            nonce_bytes.len()
        ))
        .at(span));
    }

    let ciphertext_bytes =
        crypto_value_to_bytes(&args[2], &format!("{} ciphertext", func_name), span)?;

    let maybe_tag = if args.len() >= 4 && !matches!(args[3], Value::Null) {
        Some(crypto_value_to_bytes(
            &args[3],
            &format!("{} tag", func_name),
            span,
        )?)
    } else {
        None
    };

    let aad_bytes = if args.len() >= 5 && !matches!(args[4], Value::Null) {
        crypto_value_to_bytes(&args[4], &format!("{} aad", func_name), span)?
    } else {
        Vec::new()
    };

    let mut in_out = ciphertext_bytes;
    if let Some(tag_bytes) = maybe_tag {
        if tag_bytes.len() != 16 {
            return Err(RuntimeError::new(format!(
                "{}: authentication tag must be exactly 16 bytes, got {} bytes",
                func_name,
                tag_bytes.len()
            ))
            .at(span));
        }
        in_out.extend_from_slice(&tag_bytes);
    } else if in_out.len() < 16 {
        return Err(RuntimeError::new(format!(
            "{}: combined ciphertext too short (must be at least 16 bytes to contain tag)",
            func_name
        ))
        .at(span));
    }

    let unbound_key = ring::aead::UnboundKey::new(alg, &key_bytes).map_err(|e| {
        RuntimeError::new(format!("{}: key initialization failed: {}", func_name, e)).at(span)
    })?;
    let less_safe_key = ring::aead::LessSafeKey::new(unbound_key);
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|e| {
        RuntimeError::new(format!("{}: nonce initialization failed: {}", func_name, e)).at(span)
    })?;

    let plaintext = less_safe_key
        .open_in_place(nonce, ring::aead::Aad::from(&aad_bytes), &mut in_out)
        .map_err(|_| {
            RuntimeError::new(format!(
                "{}: authentication failed (tampered ciphertext, invalid tag, or mismatched key/nonce/aad)",
                func_name
            ))
            .at(span)
        })?;

    Ok(crypto_bytes_to_value(plaintext))
}

fn crypto_encrypt_aes_gcm(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    aead_encrypt(
        &ring::aead::AES_256_GCM,
        "crypto.encrypt_aes_gcm",
        args,
        span,
    )
}

fn crypto_decrypt_aes_gcm(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    aead_decrypt(
        &ring::aead::AES_256_GCM,
        "crypto.decrypt_aes_gcm",
        args,
        span,
    )
}

fn crypto_encrypt_chacha20(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    aead_encrypt(
        &ring::aead::CHACHA20_POLY1305,
        "crypto.encrypt_chacha20",
        args,
        span,
    )
}

fn crypto_decrypt_chacha20(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    aead_decrypt(
        &ring::aead::CHACHA20_POLY1305,
        "crypto.decrypt_chacha20",
        args,
        span,
    )
}

fn pbkdf2_derive(
    alg: ring::pbkdf2::Algorithm,
    func_name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args(func_name, 4, &args, span)?;
    let password = crypto_value_to_bytes(&args[0], &format!("{} password", func_name), span)?;
    let salt = crypto_value_to_bytes(&args[1], &format!("{} salt", func_name), span)?;
    let iters = match &args[2] {
        Value::Int(n) if *n > 0 => *n as u32,
        Value::Float(f) if *f > 0.0 => *f as u32,
        Value::Number(n) if *n > 0.0 => *n as u32,
        _ => {
            return Err(RuntimeError::new(format!(
                "{}: iterations must be a positive integer",
                func_name
            ))
            .at(span));
        }
    };
    let non_zero_iters = std::num::NonZeroU32::new(iters).ok_or_else(|| {
        RuntimeError::new(format!("{}: iterations must be non-zero", func_name)).at(span)
    })?;

    let out_len = match &args[3] {
        Value::Int(n) if *n > 0 => *n as usize,
        Value::Float(f) if *f > 0.0 => *f as usize,
        Value::Number(n) if *n > 0.0 => *n as usize,
        _ => {
            return Err(RuntimeError::new(format!(
                "{}: output_len must be a positive integer",
                func_name
            ))
            .at(span));
        }
    };

    let mut out = vec![0u8; out_len];
    ring::pbkdf2::derive(alg, non_zero_iters, &salt, &password, &mut out);
    Ok(crypto_bytes_to_value(&out))
}

fn crypto_pbkdf2_sha256(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    pbkdf2_derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        "crypto.pbkdf2_sha256",
        args,
        span,
    )
}

fn crypto_pbkdf2_sha512(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    pbkdf2_derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA512,
        "crypto.pbkdf2_sha512",
        args,
        span,
    )
}

struct CryptoHkdfLen(usize);
impl ring::hkdf::KeyType for CryptoHkdfLen {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf_derive(
    alg: ring::hkdf::Algorithm,
    func_name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args(func_name, 4, &args, span)?;
    let secret = crypto_value_to_bytes(&args[0], &format!("{} secret", func_name), span)?;
    let salt = if matches!(args[1], Value::Null) {
        Vec::new()
    } else {
        crypto_value_to_bytes(&args[1], &format!("{} salt", func_name), span)?
    };
    let info = if matches!(args[2], Value::Null) {
        Vec::new()
    } else {
        crypto_value_to_bytes(&args[2], &format!("{} info", func_name), span)?
    };

    let out_len = match &args[3] {
        Value::Int(n) if *n > 0 => *n as usize,
        Value::Float(f) if *f > 0.0 => *f as usize,
        Value::Number(n) if *n > 0.0 => *n as usize,
        _ => {
            return Err(RuntimeError::new(format!(
                "{}: output_len must be a positive integer",
                func_name
            ))
            .at(span));
        }
    };

    let salt_handler = ring::hkdf::Salt::new(alg, &salt);
    let prk = salt_handler.extract(&secret);
    let info_slices = [&info[..]];
    let okm = prk
        .expand(&info_slices, CryptoHkdfLen(out_len))
        .map_err(|e| {
            RuntimeError::new(format!("{}: HKDF expand failed: {}", func_name, e)).at(span)
        })?;

    let mut out = vec![0u8; out_len];
    okm.fill(&mut out).map_err(|e| {
        RuntimeError::new(format!("{}: HKDF fill failed: {}", func_name, e)).at(span)
    })?;

    Ok(crypto_bytes_to_value(&out))
}

fn crypto_hkdf_sha256(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    hkdf_derive(ring::hkdf::HKDF_SHA256, "crypto.hkdf_sha256", args, span)
}

fn crypto_hkdf_sha512(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    hkdf_derive(ring::hkdf::HKDF_SHA512, "crypto.hkdf_sha512", args, span)
}

fn crypto_timing_safe_equal(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("crypto.timing_safe_equal", 2, &args, span)?;
    let a = crypto_value_to_bytes(&args[0], "crypto.timing_safe_equal a", span)?;
    let b = crypto_value_to_bytes(&args[1], "crypto.timing_safe_equal b", span)?;
    #[allow(deprecated)]
    let is_equal = ring::constant_time::verify_slices_are_equal(&a, &b).is_ok();
    Ok(Value::Bool(is_equal))
}

// -------------------------------------------------------------
// std:codec
// -------------------------------------------------------------
fn build_codec_module() -> Value {
    make_map(vec![
        ("base64_encode", codec_base64_encode),
        ("base64Encode", codec_base64_encode),
        ("b64_encode", codec_base64_encode),
        ("b64Encode", codec_base64_encode),
        ("base64_decode", codec_base64_decode),
        ("base64Decode", codec_base64_decode),
        ("b64_decode", codec_base64_decode),
        ("b64Decode", codec_base64_decode),
        ("base64_url_encode", codec_base64_url_encode),
        ("base64UrlEncode", codec_base64_url_encode),
        ("b64_url_encode", codec_base64_url_encode),
        ("b64UrlEncode", codec_base64_url_encode),
        ("base64_url_decode", codec_base64_url_decode),
        ("base64UrlDecode", codec_base64_url_decode),
        ("b64_url_decode", codec_base64_url_decode),
        ("b64UrlDecode", codec_base64_url_decode),
        ("url_encode", codec_url_encode),
        ("urlEncode", codec_url_encode),
        ("url_decode", codec_url_decode),
        ("urlDecode", codec_url_decode),
        ("hex_encode", codec_hex_encode),
        ("hexEncode", codec_hex_encode),
        ("hex_decode", codec_hex_decode),
        ("hexDecode", codec_hex_decode),
    ])
}

fn codec_base64_encode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.base64_encode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        Ok(Value::String(BASE64_STANDARD.encode(s.as_bytes())))
    } else {
        Err(RuntimeError::new("codec.base64_encode expects string argument".into()).at(span))
    }
}

fn codec_base64_decode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.base64_decode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        let bytes = BASE64_STANDARD.decode(s.as_bytes()).map_err(|e| {
            RuntimeError::new(format!("codec.base64_decode failed: {}", e)).at(span)
        })?;
        let text = String::from_utf8(bytes).map_err(|e| {
            RuntimeError::new(format!("codec.base64_decode invalid UTF-8: {}", e)).at(span)
        })?;
        Ok(Value::String(text))
    } else {
        Err(RuntimeError::new("codec.base64_decode expects string argument".into()).at(span))
    }
}

fn codec_base64_url_encode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.base64_url_encode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        Ok(Value::String(BASE64_URL_SAFE_NO_PAD.encode(s.as_bytes())))
    } else {
        Err(RuntimeError::new("codec.base64_url_encode expects string argument".into()).at(span))
    }
}

fn codec_base64_url_decode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.base64_url_decode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s.as_bytes()).map_err(|e| {
            RuntimeError::new(format!("codec.base64_url_decode failed: {}", e)).at(span)
        })?;
        let text = String::from_utf8(bytes).map_err(|e| {
            RuntimeError::new(format!("codec.base64_url_decode invalid UTF-8: {}", e)).at(span)
        })?;
        Ok(Value::String(text))
    } else {
        Err(RuntimeError::new("codec.base64_url_decode expects string argument".into()).at(span))
    }
}

fn codec_url_encode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.url_encode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        use std::fmt::Write;
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                b' ' => out.push('+'),
                other => {
                    let _ = write!(out, "%{:02X}", other);
                }
            }
        }
        Ok(Value::String(out))
    } else {
        Err(RuntimeError::new("codec.url_encode expects string argument".into()).at(span))
    }
}

fn codec_url_decode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.url_decode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
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
        Ok(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    } else {
        Err(RuntimeError::new("codec.url_decode expects string argument".into()).at(span))
    }
}

fn codec_hex_encode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.hex_encode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        Ok(Value::String(bytes_to_hex(s.as_bytes())))
    } else {
        Err(RuntimeError::new("codec.hex_encode expects string argument".into()).at(span))
    }
}

fn codec_hex_decode(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("codec.hex_decode", 1, &args, span)?;
    if let Value::String(s) = &args[0] {
        if s.len() % 2 != 0 {
            return Err(RuntimeError::new(
                "codec.hex_decode: hex string must have even length".into(),
            )
            .at(span));
        }
        let mut bytes = Vec::with_capacity(s.len() / 2);
        let chars: Vec<char> = s.chars().collect();
        for chunk in chars.chunks(2) {
            let pair: String = chunk.iter().collect();
            let byte = u8::from_str_radix(&pair, 16).map_err(|e| {
                RuntimeError::new(format!("codec.hex_decode invalid hex: {}", e)).at(span)
            })?;
            bytes.push(byte);
        }
        let text = String::from_utf8(bytes).map_err(|e| {
            RuntimeError::new(format!("codec.hex_decode invalid UTF-8: {}", e)).at(span)
        })?;
        Ok(Value::String(text))
    } else {
        Err(RuntimeError::new("codec.hex_decode expects string argument".into()).at(span))
    }
}

// -------------------------------------------------------------
// std:regex
// -------------------------------------------------------------
fn build_regex_module() -> Value {
    make_map(vec![
        ("is_match", regex_is_match),
        ("isMatch", regex_is_match),
        ("test", regex_is_match),
        ("find", regex_find),
        ("find_all", regex_find_all),
        ("findAll", regex_find_all),
        ("replace", regex_replace),
        ("replace_all", regex_replace_all),
        ("replaceAll", regex_replace_all),
        ("split", regex_split),
        ("captures", regex_captures),
    ])
}

fn compile_regex(pattern: &str, span: Span) -> Result<regex::Regex, RuntimeError> {
    regex::Regex::new(pattern).map_err(|e| {
        RuntimeError::new(format!("Invalid regex pattern '{}': {}", pattern, e)).at(span)
    })
}

fn regex_is_match(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("regex.is_match", 2, &args, span)?;
    if let (Value::String(pat), Value::String(text)) = (&args[0], &args[1]) {
        let re = compile_regex(pat, span)?;
        Ok(Value::Bool(re.is_match(text)))
    } else {
        Err(
            RuntimeError::new("regex.is_match expects (string pattern, string text)".into())
                .at(span),
        )
    }
}

fn regex_find(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("regex.find", 2, &args, span)?;
    if let (Value::String(pat), Value::String(text)) = (&args[0], &args[1]) {
        let re = compile_regex(pat, span)?;
        if let Some(m) = re.find(text) {
            let mut map = IndexMap::new();
            map.insert("match".to_string(), Value::String(m.as_str().to_string()));
            map.insert("start".to_string(), Value::Int(m.start() as i64));
            map.insert("end".to_string(), Value::Int(m.end() as i64));
            Ok(Value::Map(Arc::new(RwLock::new(map))))
        } else {
            Ok(Value::Null)
        }
    } else {
        Err(RuntimeError::new("regex.find expects (string pattern, string text)".into()).at(span))
    }
}

fn regex_find_all(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("regex.find_all", 2, &args, span)?;
    if let (Value::String(pat), Value::String(text)) = (&args[0], &args[1]) {
        let re = compile_regex(pat, span)?;
        let mut list = Vec::new();
        for m in re.find_iter(text) {
            let mut map = IndexMap::new();
            map.insert("match".to_string(), Value::String(m.as_str().to_string()));
            map.insert("start".to_string(), Value::Int(m.start() as i64));
            map.insert("end".to_string(), Value::Int(m.end() as i64));
            list.push(Value::Map(Arc::new(RwLock::new(map))));
        }
        Ok(Value::Array(Arc::new(RwLock::new(list))))
    } else {
        Err(
            RuntimeError::new("regex.find_all expects (string pattern, string text)".into())
                .at(span),
        )
    }
}

fn regex_replace(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("regex.replace", 3, &args, span)?;
    if let (Value::String(pat), Value::String(repl), Value::String(text)) =
        (&args[0], &args[1], &args[2])
    {
        let re = compile_regex(pat, span)?;
        Ok(Value::String(re.replace(text, repl.as_str()).to_string()))
    } else {
        Err(RuntimeError::new(
            "regex.replace expects (string pattern, string replacement, string text)".into(),
        )
        .at(span))
    }
}

fn regex_replace_all(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("regex.replace_all", 3, &args, span)?;
    if let (Value::String(pat), Value::String(repl), Value::String(text)) =
        (&args[0], &args[1], &args[2])
    {
        let re = compile_regex(pat, span)?;
        Ok(Value::String(
            re.replace_all(text, repl.as_str()).to_string(),
        ))
    } else {
        Err(RuntimeError::new(
            "regex.replace_all expects (string pattern, string replacement, string text)".into(),
        )
        .at(span))
    }
}

fn regex_split(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("regex.split", 2, &args, span)?;
    if let (Value::String(pat), Value::String(text)) = (&args[0], &args[1]) {
        let re = compile_regex(pat, span)?;
        let list: Vec<Value> = re
            .split(text)
            .map(|s| Value::String(s.to_string()))
            .collect();
        Ok(Value::Array(Arc::new(RwLock::new(list))))
    } else {
        Err(RuntimeError::new("regex.split expects (string pattern, string text)".into()).at(span))
    }
}

fn regex_captures(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("regex.captures", 2, &args, span)?;
    if let (Value::String(pat), Value::String(text)) = (&args[0], &args[1]) {
        let re = compile_regex(pat, span)?;
        if let Some(caps) = re.captures(text) {
            let mut list = Vec::new();
            for g in caps.iter() {
                match g {
                    Some(m) => list.push(Value::String(m.as_str().to_string())),
                    None => list.push(Value::Null),
                }
            }
            Ok(Value::Array(Arc::new(RwLock::new(list))))
        } else {
            Ok(Value::Null)
        }
    } else {
        Err(
            RuntimeError::new("regex.captures expects (string pattern, string text)".into())
                .at(span),
        )
    }
}

// -------------------------------------------------------------
// std:http
// -------------------------------------------------------------
fn url_encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            other => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", other);
            }
        }
    }
    out
}

struct HttpOptions {
    headers: Option<IndexMap<String, Value>>,
    query: Option<IndexMap<String, Value>>,
    json: Option<Value>,
    timeout_ms: Option<u64>,
}

fn extract_http_options(opt_val: Option<&Value>) -> HttpOptions {
    let mut opts = HttpOptions {
        headers: None,
        query: None,
        json: None,
        timeout_ms: None,
    };
    if let Some(Value::Map(m)) = opt_val {
        let guard = m.read().unwrap();
        opts.headers = guard.get("headers").and_then(|v| match v {
            Value::Map(hm) => Some(hm.read().unwrap().clone()),
            _ => None,
        });
        opts.query = guard
            .get("query")
            .or_else(|| guard.get("params"))
            .and_then(|v| match v {
                Value::Map(qm) => Some(qm.read().unwrap().clone()),
                _ => None,
            });
        opts.json = guard.get("json").cloned();
        opts.timeout_ms = guard.get("timeout_ms").and_then(|v| match v {
            Value::Int(n) if *n > 0 => Some(*n as u64),
            Value::Number(n) if *n > 0.0 => Some(*n as u64),
            Value::Float(n) if *n > 0.0 => Some(*n as u64),
            _ => None,
        });
    }
    opts
}

pub(crate) fn execute_http_request(
    method: &str,
    url: &str,
    headers_opt: Option<&IndexMap<String, Value>>,
    body_opt: Option<&Value>,
    query_opt: Option<&IndexMap<String, Value>>,
    timeout_ms: Option<u64>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let client_builder = reqwest::blocking::Client::builder();
    let client = if let Some(ms) = timeout_ms {
        client_builder.timeout(std::time::Duration::from_millis(ms))
    } else {
        client_builder.timeout(std::time::Duration::from_secs(30))
    }
    .build()
    .map_err(|e| RuntimeError::new(format!("http: client initialization error: {}", e)).at(span))?;

    let final_url = if let Some(query) = query_opt {
        if query.is_empty() {
            url.to_string()
        } else {
            let mut qs = String::new();
            for (i, (k, v)) in query.iter().enumerate() {
                if i > 0 {
                    qs.push('&');
                }
                let val_str = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                qs.push_str(&url_encode_component(k));
                qs.push('=');
                qs.push_str(&url_encode_component(&val_str));
            }
            if url.contains('?') {
                format!("{}&{}", url, qs)
            } else {
                format!("{}?{}", url, qs)
            }
        }
    } else {
        url.to_string()
    };

    let method_upper = method.to_uppercase();
    let req_method = match method_upper.as_str() {
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        _ => reqwest::Method::GET,
    };

    let mut req = client.request(req_method, &final_url);

    let mut content_type_set = false;
    if let Some(headers) = headers_opt {
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-type") {
                content_type_set = true;
            }
            let val_str = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            req = req.header(k.as_str(), val_str);
        }
    }

    if let Some(b) = body_opt {
        let (body_str, is_json) = match b {
            Value::String(s) => (s.clone(), false),
            Value::Map(_) | Value::Array(_) => (b.to_json().to_string(), true),
            other => (other.to_string(), false),
        };
        if is_json && !content_type_set {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(body_str);
    }

    let response = req.send().map_err(|e| {
        RuntimeError::new(format!("http: request to '{}' failed: {}", final_url, e)).at(span)
    })?;

    let status = response.status().as_u16() as i64;
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let is_ok = response.status().is_success();

    let mut resp_headers = IndexMap::new();
    for (k, v) in response.headers() {
        if let Ok(v_str) = v.to_str() {
            resp_headers.insert(k.as_str().to_string(), Value::String(v_str.to_string()));
        }
    }

    let body_text = response.text().unwrap_or_default();
    let parsed_json = serde_json::from_str::<serde_json::Value>(&body_text)
        .ok()
        .map(|j| Value::from_json(&j))
        .unwrap_or(Value::Null);

    let mut out = IndexMap::new();
    out.insert("status".to_string(), Value::Int(status));
    out.insert("status_text".to_string(), Value::String(status_text));
    out.insert("ok".to_string(), Value::Bool(is_ok));
    out.insert(
        "headers".to_string(),
        Value::Map(Arc::new(RwLock::new(resp_headers))),
    );
    out.insert("body".to_string(), Value::String(body_text.clone()));
    out.insert("text".to_string(), Value::String(body_text));
    out.insert("json".to_string(), parsed_json);

    Ok(Value::Map(Arc::new(RwLock::new(out))))
}

fn http_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("http.get expects (url, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.get expects a string url".into()).at(span)),
    };
    let opts = extract_http_options(args.get(1));
    execute_http_request(
        "GET",
        url,
        opts.headers.as_ref(),
        opts.json.as_ref(),
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_post(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("http.post expects (url, body, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.post expects a string url".into()).at(span)),
    };
    let body = &args[1];
    let opts = extract_http_options(args.get(2));
    let effective_body = if matches!(body, Value::Null) && opts.json.is_some() {
        opts.json.as_ref()
    } else {
        Some(body)
    };
    execute_http_request(
        "POST",
        url,
        opts.headers.as_ref(),
        effective_body,
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_put(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("http.put expects (url, body, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.put expects a string url".into()).at(span)),
    };
    let body = &args[1];
    let opts = extract_http_options(args.get(2));
    let effective_body = if matches!(body, Value::Null) && opts.json.is_some() {
        opts.json.as_ref()
    } else {
        Some(body)
    };
    execute_http_request(
        "PUT",
        url,
        opts.headers.as_ref(),
        effective_body,
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_delete(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("http.delete expects (url, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.delete expects a string url".into()).at(span)),
    };
    let opts = extract_http_options(args.get(1));
    execute_http_request(
        "DELETE",
        url,
        opts.headers.as_ref(),
        opts.json.as_ref(),
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_patch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("http.patch expects (url, body, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.patch expects a string url".into()).at(span)),
    };
    let body = &args[1];
    let opts = extract_http_options(args.get(2));
    let effective_body = if matches!(body, Value::Null) && opts.json.is_some() {
        opts.json.as_ref()
    } else {
        Some(body)
    };
    execute_http_request(
        "PATCH",
        url,
        opts.headers.as_ref(),
        effective_body,
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_head(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("http.head expects (url, [opts])".into()).at(span));
    }
    let url = match &args[0] {
        Value::String(u) => u.as_str(),
        _ => return Err(RuntimeError::new("http.head expects a string url".into()).at(span)),
    };
    let opts = extract_http_options(args.get(1));
    execute_http_request(
        "HEAD",
        url,
        opts.headers.as_ref(),
        None,
        opts.query.as_ref(),
        opts.timeout_ms,
        span,
    )
}

fn http_get_json(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let res = http_get(ev, args, span)?;
    if let Value::Map(m) = res {
        let guard = m.read().unwrap();
        Ok(guard.get("json").cloned().unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}

fn http_post_json(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let res = http_post(ev, args, span)?;
    if let Value::Map(m) = res {
        let guard = m.read().unwrap();
        Ok(guard.get("json").cloned().unwrap_or(Value::Null))
    } else {
        Ok(Value::Null)
    }
}

fn http_request(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("http.request", 1, &args, span)?;
    match &args[0] {
        Value::Map(m) => {
            let guard = m.read().unwrap();
            let url = guard
                .get("url")
                .and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .ok_or_else(|| {
                    RuntimeError::new("http.request expects 'url' string property".into()).at(span)
                })?;
            let method = guard
                .get("method")
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "GET".to_string());
            let headers_opt = guard.get("headers").and_then(|v| match v {
                Value::Map(hm) => Some(hm.read().unwrap().clone()),
                _ => None,
            });
            let query_opt =
                guard
                    .get("query")
                    .or_else(|| guard.get("params"))
                    .and_then(|v| match v {
                        Value::Map(qm) => Some(qm.read().unwrap().clone()),
                        _ => None,
                    });
            let body_opt = guard.get("body").or_else(|| guard.get("json"));
            let timeout_ms = guard.get("timeout_ms").and_then(|v| match v {
                Value::Int(n) if *n > 0 => Some(*n as u64),
                Value::Number(n) if *n > 0.0 => Some(*n as u64),
                Value::Float(n) if *n > 0.0 => Some(*n as u64),
                _ => None,
            });
            execute_http_request(
                &method,
                url,
                headers_opt.as_ref(),
                body_opt,
                query_opt.as_ref(),
                timeout_ms,
                span,
            )
        }
        _ => Err(RuntimeError::new("http.request expects an options map".into()).at(span)),
    }
}

fn build_http_module() -> Value {
    make_map(vec![
        ("get", http_get),
        ("post", http_post),
        ("put", http_put),
        ("delete", http_delete),
        ("patch", http_patch),
        ("head", http_head),
        ("request", http_request),
        ("fetch", http_get),
        ("get_json", http_get_json),
        ("getJson", http_get_json),
        ("post_json", http_post_json),
        ("postJson", http_post_json),
    ])
}

// -------------------------------------------------------------
// std:env
// -------------------------------------------------------------
fn env_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("env.get expects at least 1 argument".into()).at(span));
    }
    if let Value::String(key) = &args[0] {
        let default = if args.len() > 1 {
            args[1].clone()
        } else {
            Value::Null
        };
        match std::env::var(key) {
            Ok(v) => Ok(Value::String(v)),
            Err(_) => Ok(default),
        }
    } else {
        Err(RuntimeError::new("env.get expects string key".into()).at(span))
    }
}

fn env_set(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("env.set", 2, &args, span)?;
    if let (Value::String(k), Value::String(v)) = (&args[0], &args[1]) {
        unsafe {
            std::env::set_var(k, v);
        }
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new("env.set expects (string key, string value)".into()).at(span))
    }
}

fn env_has(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("env.has", 1, &args, span)?;
    if let Value::String(k) = &args[0] {
        Ok(Value::Bool(std::env::var(k).is_ok()))
    } else {
        Err(RuntimeError::new("env.has expects string key".into()).at(span))
    }
}

fn env_remove(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("env.remove", 1, &args, span)?;
    if let Value::String(k) = &args[0] {
        unsafe {
            std::env::remove_var(k);
        }
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new("env.remove expects string key".into()).at(span))
    }
}

fn env_all(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut map = IndexMap::new();
    for (k, v) in std::env::vars() {
        map.insert(k, Value::String(v));
    }
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn parse_dotenv_content(content: &str) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let clean = if let Some(stripped) = trimmed.strip_prefix("export ") {
            stripped.trim()
        } else {
            trimmed
        };
        if let Some((k, v)) = clean.split_once('=') {
            let key = k.trim().to_string();
            let mut val = v.trim();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                if val.len() >= 2 {
                    val = &val[1..val.len() - 1];
                }
            }
            map.insert(key, Value::String(val.to_string()));
        }
    }
    map
}

fn env_parse_dotenv(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("env.parse_dotenv", 1, &args, span)?;
    if let Value::String(content) = &args[0] {
        let map = parse_dotenv_content(content);
        Ok(Value::Map(Arc::new(RwLock::new(map))))
    } else {
        Err(RuntimeError::new("env.parse_dotenv expects string content".into()).at(span))
    }
}

fn env_load_dotenv(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let path = if !args.is_empty() {
        match &args[0] {
            Value::String(p) => p.clone(),
            _ => ".env".to_string(),
        }
    } else {
        ".env".to_string()
    };
    if let Ok(content) = std::fs::read_to_string(&path) {
        let map = parse_dotenv_content(&content);
        for (k, v) in &map {
            if let Value::String(val_str) = v {
                unsafe {
                    std::env::set_var(k, val_str);
                }
            }
        }
        Ok(Value::Map(Arc::new(RwLock::new(map))))
    } else {
        Ok(Value::Map(Arc::new(RwLock::new(IndexMap::new()))))
    }
}

fn build_env_module() -> Value {
    make_map(vec![
        ("get", env_get),
        ("set", env_set),
        ("has", env_has),
        ("remove", env_remove),
        ("all", env_all),
        ("parse_dotenv", env_parse_dotenv),
        ("load_dotenv", env_load_dotenv),
    ])
}

// -------------------------------------------------------------
// std:cli / std:args
// -------------------------------------------------------------
fn cli_args(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let args: Vec<Value> = std::env::args().map(Value::String).collect();
    Ok(Value::Array(Arc::new(RwLock::new(args))))
}

fn cli_parse(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("cli.parse expects an array of arguments".into()).at(span));
    }
    let items: Vec<String> = match &args[0] {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            guard
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        }
        _ => {
            return Err(RuntimeError::new("cli.parse expects an array of strings".into()).at(span));
        }
    };

    let mut options = IndexMap::new();
    let mut flags = IndexMap::new();
    let mut positional = Vec::new();

    let mut i = 0;
    while i < items.len() {
        let arg = &items[i];
        if let Some(rest) = arg.strip_prefix("--") {
            if let Some((key, val)) = rest.split_once('=') {
                options.insert(key.to_string(), Value::String(val.to_string()));
            } else if i + 1 < items.len() && !items[i + 1].starts_with('-') {
                options.insert(rest.to_string(), Value::String(items[i + 1].clone()));
                i += 1;
            } else {
                flags.insert(rest.to_string(), Value::Bool(true));
            }
        } else if let Some(rest) = arg.strip_prefix('-') {
            if rest.len() == 1 {
                if i + 1 < items.len() && !items[i + 1].starts_with('-') {
                    options.insert(rest.to_string(), Value::String(items[i + 1].clone()));
                    i += 1;
                } else {
                    flags.insert(rest.to_string(), Value::Bool(true));
                }
            } else {
                for ch in rest.chars() {
                    flags.insert(ch.to_string(), Value::Bool(true));
                }
            }
        } else {
            positional.push(Value::String(arg.clone()));
        }
        i += 1;
    }

    let mut res = IndexMap::new();
    res.insert(
        "options".to_string(),
        Value::Map(Arc::new(RwLock::new(options))),
    );
    res.insert(
        "flags".to_string(),
        Value::Map(Arc::new(RwLock::new(flags))),
    );
    res.insert(
        "positional".to_string(),
        Value::Array(Arc::new(RwLock::new(positional.clone()))),
    );
    let sub = if !positional.is_empty() {
        positional[0].clone()
    } else {
        Value::Null
    };
    res.insert("subcommand".to_string(), sub);

    Ok(Value::Map(Arc::new(RwLock::new(res))))
}

fn build_cli_module() -> Value {
    make_map(vec![("args", cli_args), ("parse", cli_parse)])
}

// -------------------------------------------------------------
// std:collections / std:set
// -------------------------------------------------------------
pub fn make_set_instance(items: Vec<Value>) -> Value {
    let mut map = IndexMap::new();
    let mut set = indexmap::IndexSet::new();
    for item in items {
        set.insert(item);
    }
    let unique_items: Vec<Value> = set.into_iter().collect();
    map.insert("_is_set".to_string(), Value::Bool(true));
    map.insert(
        "items".to_string(),
        Value::Array(Arc::new(RwLock::new(unique_items))),
    );
    Value::Map(Arc::new(RwLock::new(map)))
}

fn col_set_new(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let mut initial = Vec::new();
    if !args.is_empty() {
        match &args[0] {
            Value::Array(arr) => {
                initial = arr.read().unwrap().clone();
            }
            Value::Map(m) if m.read().unwrap().contains_key("_is_set") => {
                if let Some(Value::Array(arr)) = m.read().unwrap().get("items") {
                    initial = arr.read().unwrap().clone();
                }
            }
            other => {
                return Err(RuntimeError::new(format!(
                    "collections.set expects array, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        }
    }
    Ok(make_set_instance(initial))
}

fn col_unique(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("collections.unique", 1, &args, span)?;
    if let Value::Array(arr) = &args[0] {
        let guard = arr.read().unwrap();
        let mut set = indexmap::IndexSet::new();
        for item in guard.iter() {
            set.insert(item.clone());
        }
        let unique_items: Vec<Value> = set.into_iter().collect();
        Ok(Value::Array(Arc::new(RwLock::new(unique_items))))
    } else {
        Err(RuntimeError::new("collections.unique expects array".into()).at(span))
    }
}

fn col_union(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("collections.union", 2, &args, span)?;
    if let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) {
        let a_guard = a.read().unwrap();
        let b_guard = b.read().unwrap();
        let mut set = indexmap::IndexSet::new();
        for item in a_guard.iter().chain(b_guard.iter()) {
            set.insert(item.clone());
        }
        let out: Vec<Value> = set.into_iter().collect();
        Ok(Value::Array(Arc::new(RwLock::new(out))))
    } else {
        Err(RuntimeError::new("collections.union expects (array, array)".into()).at(span))
    }
}

fn col_intersect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("collections.intersect", 2, &args, span)?;
    if let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) {
        let a_guard = a.read().unwrap();
        let b_guard = b.read().unwrap();
        let b_set: indexmap::IndexSet<Value> = b_guard.iter().cloned().collect();
        let mut out = indexmap::IndexSet::new();
        for item in a_guard.iter() {
            if b_set.contains(item) {
                out.insert(item.clone());
            }
        }
        let res: Vec<Value> = out.into_iter().collect();
        Ok(Value::Array(Arc::new(RwLock::new(res))))
    } else {
        Err(RuntimeError::new("collections.intersect expects (array, array)".into()).at(span))
    }
}

fn col_difference(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("collections.difference", 2, &args, span)?;
    if let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) {
        let a_guard = a.read().unwrap();
        let b_guard = b.read().unwrap();
        let b_set: indexmap::IndexSet<Value> = b_guard.iter().cloned().collect();
        let mut out = indexmap::IndexSet::new();
        for item in a_guard.iter() {
            if !b_set.contains(item) {
                out.insert(item.clone());
            }
        }
        let res: Vec<Value> = out.into_iter().collect();
        Ok(Value::Array(Arc::new(RwLock::new(res))))
    } else {
        Err(RuntimeError::new("collections.difference expects (array, array)".into()).at(span))
    }
}

fn col_chunk(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("collections.chunk", 2, &args, span)?;
    let size = match &args[1] {
        Value::Int(n) if *n > 0 => *n as usize,
        Value::Number(n) if *n > 0.0 => *n as usize,
        _ => {
            return Err(RuntimeError::new(
                "collections.chunk size must be a positive integer".into(),
            )
            .at(span));
        }
    };
    if let Value::Array(arr) = &args[0] {
        let guard = arr.read().unwrap();
        let chunks: Vec<Value> = guard
            .chunks(size)
            .map(|c| Value::Array(Arc::new(RwLock::new(c.to_vec()))))
            .collect();
        Ok(Value::Array(Arc::new(RwLock::new(chunks))))
    } else {
        Err(RuntimeError::new("collections.chunk expects array".into()).at(span))
    }
}

fn col_zip(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("collections.zip", 2, &args, span)?;
    if let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) {
        let a_guard = a.read().unwrap();
        let b_guard = b.read().unwrap();
        let zipped: Vec<Value> = a_guard
            .iter()
            .zip(b_guard.iter())
            .map(|(x, y)| Value::Array(Arc::new(RwLock::new(vec![x.clone(), y.clone()]))))
            .collect();
        Ok(Value::Array(Arc::new(RwLock::new(zipped))))
    } else {
        Err(RuntimeError::new("collections.zip expects (array, array)".into()).at(span))
    }
}

fn build_collections_module() -> Value {
    make_map(vec![
        ("unique", col_unique),
        ("dedup", col_unique),
        ("union", col_union),
        ("intersect", col_intersect),
        ("difference", col_difference),
        ("chunk", col_chunk),
        ("zip", col_zip),
        ("set", col_set_new),
        ("new_set", col_set_new),
        ("Set", col_set_new),
        ("set_from_array", col_unique),
        ("set_union", col_union),
        ("set_intersect", col_intersect),
        ("set_difference", col_difference),
    ])
}

// -------------------------------------------------------------
// std:ffi
// -------------------------------------------------------------
fn ffi_open(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let path = if args.is_empty() {
        "".to_string()
    } else {
        match &args[0] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "ffi.open expects string or no arguments, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        }
    };

    let lib = crate::ffi::DynamicLibrary::open(&path).map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::FfiLibrary(Arc::new(lib)))
}

fn ffi_current_process(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let lib = crate::ffi::DynamicLibrary::this().map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::FfiLibrary(Arc::new(lib)))
}

fn ffi_load(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() != 4 {
        return Err(RuntimeError::new(
            "ffi.load(lib, symbol, params, return_type) expects 4 arguments".into(),
        )
        .at(span));
    }

    let lib_arc = match &args[0] {
        Value::FfiLibrary(l) => l.clone(),
        Value::String(path) => {
            let lib = crate::ffi::DynamicLibrary::open(path)
                .map_err(|e| RuntimeError::new(e).at(span))?;
            Arc::new(lib)
        }
        other => {
            return Err(RuntimeError::new(format!(
                "ffi.load expects library or path as first argument, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let sym_name = match &args[1] {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::new(format!(
                "ffi.load expects symbol name string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let param_types = match &args[2] {
        Value::Array(arr) => {
            let items = arr.read().unwrap();
            let mut types = Vec::new();
            for item in items.iter() {
                match item {
                    Value::String(t) => {
                        let parsed = t
                            .parse::<crate::ffi::FfiType>()
                            .map_err(|e| RuntimeError::new(e).at(span))?;
                        types.push(parsed);
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ffi.load parameter type must be a string, got {}",
                            other.type_name()
                        ))
                        .at(span));
                    }
                }
            }
            types
        }
        other => {
            return Err(RuntimeError::new(format!(
                "ffi.load parameter types must be an array, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let ret_type = match &args[3] {
        Value::String(s) => s
            .parse::<crate::ffi::FfiType>()
            .map_err(|e| RuntimeError::new(e).at(span))?,
        other => {
            return Err(RuntimeError::new(format!(
                "ffi.load return type must be a string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let dynamic_fn = lib_arc
        .get_symbol(&sym_name, param_types, ret_type)
        .map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::FfiFunction(Arc::new(dynamic_fn)))
}

fn ffi_alloc(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.alloc", 1, &args, span)?;
    let size = match &args[0] {
        Value::Int(n) if *n > 0 => *n as usize,
        Value::Float(n) | Value::Number(n) if *n > 0.0 => *n as usize,
        _ => return Err(RuntimeError::new("ffi.alloc expects positive size".into()).at(span)),
    };
    let ptr = crate::ffi::memory_alloc(size);
    Ok(Value::Int(ptr as usize as i64))
}

fn ffi_free(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("ffi.free expects at least 1 argument (pointer)".into()).at(span),
        );
    }
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => return Err(RuntimeError::new("ffi.free expects integer pointer".into()).at(span)),
    };
    let size = if args.len() >= 2 {
        match &args[1] {
            Value::Int(n) if *n >= 0 => *n as usize,
            _ => 8,
        }
    } else {
        8
    };
    crate::ffi::memory_free(ptr, size);
    Ok(Value::Null)
}

fn ffi_read_string(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("ffi.read_string", 1, &args, span)?;
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => {
            return Err(
                RuntimeError::new("ffi.read_string expects integer pointer".into()).at(span),
            );
        }
    };
    match crate::ffi::memory_read_string(ptr) {
        Some(s) => Ok(Value::String(s)),
        None => Ok(Value::Null),
    }
}

fn ffi_read_i32(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.read_i32", 1, &args, span)?;
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => return Err(RuntimeError::new("ffi.read_i32 expects integer pointer".into()).at(span)),
    };
    Ok(Value::Int(crate::ffi::memory_read_i32(ptr) as i64))
}

fn ffi_read_i64(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.read_i64", 1, &args, span)?;
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => return Err(RuntimeError::new("ffi.read_i64 expects integer pointer".into()).at(span)),
    };
    Ok(Value::Int(crate::ffi::memory_read_i64(ptr)))
}

fn ffi_write_i32(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.write_i32", 2, &args, span)?;
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => {
            return Err(RuntimeError::new("ffi.write_i32 expects integer pointer".into()).at(span));
        }
    };
    let val = match &args[1] {
        Value::Int(n) => *n as i32,
        Value::Float(n) | Value::Number(n) => *n as i32,
        _ => return Err(RuntimeError::new("ffi.write_i32 expects integer value".into()).at(span)),
    };
    crate::ffi::memory_write_i32(ptr, val);
    Ok(Value::Null)
}

fn ffi_write_i64(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.write_i64", 2, &args, span)?;
    let ptr = match &args[0] {
        Value::Int(n) => *n as usize,
        _ => {
            return Err(RuntimeError::new("ffi.write_i64 expects integer pointer".into()).at(span));
        }
    };
    let val = match &args[1] {
        Value::Int(n) => *n,
        Value::Float(n) | Value::Number(n) => *n as i64,
        _ => return Err(RuntimeError::new("ffi.write_i64 expects integer value".into()).at(span)),
    };
    crate::ffi::memory_write_i64(ptr, val);
    Ok(Value::Null)
}

fn ffi_sizeof(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ffi.sizeof", 1, &args, span)?;
    let type_str = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "ffi.sizeof expects string type name, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let parsed: crate::ffi::FfiType = type_str
        .parse()
        .map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::Int(parsed.size() as i64))
}

fn build_ffi_module() -> Value {
    make_map(vec![
        ("open", ffi_open),
        ("current_process", ffi_current_process),
        ("load", ffi_load),
        ("fn", ffi_load),
        ("alloc", ffi_alloc),
        ("free", ffi_free),
        ("read_string", ffi_read_string),
        ("read_str", ffi_read_string),
        ("read_i32", ffi_read_i32),
        ("read_i64", ffi_read_i64),
        ("write_i32", ffi_write_i32),
        ("write_i64", ffi_write_i64),
        ("sizeof", ffi_sizeof),
    ])
}

// -------------------------------------------------------------
// std:csv
// -------------------------------------------------------------
fn build_csv_module() -> Value {
    make_map(vec![("parse", csv_parse), ("stringify", csv_stringify)])
}

fn csv_parse(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new(
            "csv.parse(text, [options]) expects at least 1 argument".into(),
        )
        .at(span));
    }
    let input = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "csv.parse expects string input, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let mut has_headers = true;
    let mut delimiter = ',';
    let mut typed = true;

    if args.len() > 1 {
        if let Value::Map(opts) = &args[1] {
            let guard = opts.read().unwrap();
            if let Some(Value::Bool(b)) = guard.get("headers") {
                has_headers = *b;
            }
            if let Some(Value::String(d)) = guard.get("delimiter") {
                if let Some(first_char) = d.chars().next() {
                    delimiter = first_char;
                }
            }
            if let Some(Value::Bool(t)) = guard.get("typed") {
                typed = *t;
            }
        }
    }

    crate::csv::parse_csv(input, has_headers, delimiter, typed)
        .map_err(|e| RuntimeError::new(format!("csv.parse error: {}", e)).at(span))
}

fn csv_stringify(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new(
            "csv.stringify(rows, [options]) expects at least 1 argument".into(),
        )
        .at(span));
    }
    let rows_val = &args[0];

    let mut delimiter = ',';
    let mut headers = None;

    if args.len() > 1 {
        if let Value::Map(opts) = &args[1] {
            let guard = opts.read().unwrap();
            if let Some(Value::String(d)) = guard.get("delimiter") {
                if let Some(first_char) = d.chars().next() {
                    delimiter = first_char;
                }
            }
            if let Some(Value::Array(h_arr)) = guard.get("headers") {
                let h_guard = h_arr.read().unwrap();
                let custom: Vec<String> = h_guard.iter().map(|v| v.to_display()).collect();
                headers = Some(custom);
            }
        }
    }

    crate::csv::stringify_csv(rows_val, delimiter, headers)
        .map(Value::String)
        .map_err(|e| RuntimeError::new(format!("csv.stringify error: {}", e)).at(span))
}

// -------------------------------------------------------------
// std:sqlite
// -------------------------------------------------------------
fn build_sqlite_module() -> Value {
    make_map(vec![
        ("open", sqlite_open),
        ("execute", sqlite_execute),
        ("query", sqlite_query),
        ("last_insert_id", sqlite_last_insert_id),
        ("changes", sqlite_changes),
        ("close", sqlite_close),
    ])
}

fn extract_sqlite_handle(val: &Value, span: Span) -> Result<u64, RuntimeError> {
    match val {
        Value::Int(n) if *n > 0 => Ok(*n as u64),
        Value::Map(m) => {
            let guard = m.read().unwrap();
            match guard.get("_sqlite_handle") {
                Some(Value::Int(n)) if *n > 0 => Ok(*n as u64),
                _ => Err(RuntimeError::new(
                    "Invalid SQLite connection map: missing '_sqlite_handle'".into(),
                )
                .at(span)),
            }
        }
        other => Err(RuntimeError::new(format!(
            "Expected SQLite connection or handle integer, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn sqlite_open(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("sqlite.open", 1, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "sqlite.open expects string path, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let handle = crate::sqlite::sqlite_open(path)
        .map_err(|e| RuntimeError::new(format!("sqlite.open error: {}", e)).at(span))?;

    let mut map = IndexMap::new();
    map.insert("_sqlite_handle".to_string(), Value::Int(handle as i64));
    map.insert("path".to_string(), Value::String(path.to_string()));

    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn sqlite_execute(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "sqlite.execute(conn, sql, [params]) expects at least 2 arguments".into(),
        )
        .at(span));
    }
    let handle = extract_sqlite_handle(&args[0], span)?;
    let sql = match &args[1] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "sqlite.execute expects SQL string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let params: Vec<Value> = if args.len() > 2 {
        match &args[2] {
            Value::Array(arr) => arr.read().unwrap().clone(),
            other => vec![other.clone()],
        }
    } else {
        Vec::new()
    };

    let changes = crate::sqlite::sqlite_execute(handle, sql, &params)
        .map_err(|e| RuntimeError::new(format!("sqlite.execute error: {}", e)).at(span))?;

    Ok(Value::Int(changes))
}

fn sqlite_query(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "sqlite.query(conn, sql, [params]) expects at least 2 arguments".into(),
        )
        .at(span));
    }
    let handle = extract_sqlite_handle(&args[0], span)?;
    let sql = match &args[1] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "sqlite.query expects SQL string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let params: Vec<Value> = if args.len() > 2 {
        match &args[2] {
            Value::Array(arr) => arr.read().unwrap().clone(),
            other => vec![other.clone()],
        }
    } else {
        Vec::new()
    };

    crate::sqlite::sqlite_query(handle, sql, &params)
        .map_err(|e| RuntimeError::new(format!("sqlite.query error: {}", e)).at(span))
}

fn sqlite_last_insert_id(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("sqlite.last_insert_id", 1, &args, span)?;
    let handle = extract_sqlite_handle(&args[0], span)?;
    let id = crate::sqlite::sqlite_last_insert_id(handle)
        .map_err(|e| RuntimeError::new(format!("sqlite.last_insert_id error: {}", e)).at(span))?;
    Ok(Value::Int(id))
}

fn sqlite_changes(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("sqlite.changes", 1, &args, span)?;
    let handle = extract_sqlite_handle(&args[0], span)?;
    let count = crate::sqlite::sqlite_changes(handle)
        .map_err(|e| RuntimeError::new(format!("sqlite.changes error: {}", e)).at(span))?;
    Ok(Value::Int(count))
}

fn sqlite_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("sqlite.close", 1, &args, span)?;
    let handle = extract_sqlite_handle(&args[0], span)?;
    crate::sqlite::sqlite_close(handle)
        .map_err(|e| RuntimeError::new(format!("sqlite.close error: {}", e)).at(span))?;
    Ok(Value::Null)
}

// -------------------------------------------------------------
// std:unicode
// -------------------------------------------------------------
fn build_unicode_module() -> Value {
    make_map(vec![
        ("graphemes", unicode_graphemes),
        ("grapheme_len", unicode_grapheme_len),
        ("graphemeLen", unicode_grapheme_len),
        ("byte_len", unicode_byte_len),
        ("byteLen", unicode_byte_len),
        ("slice", unicode_slice_graphemes),
        ("slice_graphemes", unicode_slice_graphemes),
        ("sliceGraphemes", unicode_slice_graphemes),
        ("runes", unicode_runes),
        ("codepoints", unicode_codepoints),
        ("from_codepoints", unicode_from_codepoints),
        ("fromCodepoints", unicode_from_codepoints),
        ("is_ascii", unicode_is_ascii),
        ("isAscii", unicode_is_ascii),
        ("category", unicode_category),
    ])
}

fn unicode_graphemes(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.graphemes", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.graphemes expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let items: Vec<Value> = crate::unicode::graphemes(s)
        .into_iter()
        .map(|g| Value::String(g.to_string()))
        .collect();
    Ok(Value::Array(Arc::new(RwLock::new(items))))
}

fn unicode_grapheme_len(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.grapheme_len", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.grapheme_len expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok(Value::Int(crate::unicode::grapheme_len(s) as i64))
}

fn unicode_byte_len(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.byte_len", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.byte_len expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok(Value::Int(s.len() as i64))
}

fn unicode_slice_graphemes(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(
            RuntimeError::new("unicode.slice expects (string, start, [end])".into()).at(span),
        );
    }
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.slice expects string target, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let start = match &args[1] {
        Value::Int(n) => *n,
        Value::Float(f) | Value::Number(f) => *f as i64,
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.slice expects integer start, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let end = if args.len() == 3 {
        match &args[2] {
            Value::Int(n) => Some(*n),
            Value::Float(f) | Value::Number(f) => Some(*f as i64),
            Value::Null => None,
            other => {
                return Err(RuntimeError::new(format!(
                    "unicode.slice expects integer end, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        }
    } else {
        None
    };
    Ok(Value::String(crate::unicode::slice_graphemes(
        s, start, end,
    )))
}

fn unicode_runes(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("unicode.runes", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.runes expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let items: Vec<Value> = crate::unicode::runes(s)
        .into_iter()
        .map(Value::String)
        .collect();
    Ok(Value::Array(Arc::new(RwLock::new(items))))
}

fn unicode_codepoints(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.codepoints", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.codepoints expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let items: Vec<Value> = crate::unicode::codepoints(s)
        .into_iter()
        .map(Value::Int)
        .collect();
    Ok(Value::Array(Arc::new(RwLock::new(items))))
}

fn unicode_from_codepoints(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.from_codepoints", 1, &args, span)?;
    let cps = match &args[0] {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut list = Vec::with_capacity(guard.len());
            for v in guard.iter() {
                match v {
                    Value::Int(n) => list.push(*n),
                    Value::Float(f) | Value::Number(f) => list.push(*f as i64),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "unicode.from_codepoints array must contain integers, got {}",
                            other.type_name()
                        ))
                        .at(span));
                    }
                }
            }
            list
        }
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.from_codepoints expects array of codepoints, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    crate::unicode::from_codepoints(&cps)
        .map(Value::String)
        .map_err(|e| RuntimeError::new(e).at(span))
}

fn unicode_is_ascii(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.is_ascii", 1, &args, span)?;
    let s = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.is_ascii expects string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok(Value::Bool(crate::unicode::is_ascii(s)))
}

fn unicode_category(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unicode.category", 1, &args, span)?;
    let ch = match &args[0] {
        Value::String(s) => {
            if let Some(c) = s.chars().next() {
                c
            } else {
                return Err(
                    RuntimeError::new("unicode.category expects non-empty string".into()).at(span),
                );
            }
        }
        Value::Int(n) => {
            if *n >= 0 && *n <= 0x10FFFF {
                match char::from_u32(*n as u32) {
                    Some(c) => c,
                    None => {
                        return Err(RuntimeError::new(format!(
                            "Invalid Unicode codepoint {:#x}",
                            n
                        ))
                        .at(span));
                    }
                }
            } else {
                return Err(
                    RuntimeError::new(format!("Unicode codepoint {:#x} out of range", n)).at(span),
                );
            }
        }
        other => {
            return Err(RuntimeError::new(format!(
                "unicode.category expects character string or integer codepoint, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok(Value::String(
        crate::unicode::unicode_category(ch).to_string(),
    ))
}

// -------------------------------------------------------------
// std:actor
// -------------------------------------------------------------

fn build_actor_module() -> Value {
    let mut map = IndexMap::new();
    let entries = vec![
        ("spawn", actor_spawn as crate::value::BuiltinFn),
        ("spawn_named", actor_spawn_named),
        ("spawnNamed", actor_spawn_named),
        ("send", actor_send),
        ("ask", actor_ask),
        ("stop", actor_stop),
        ("is_alive", actor_is_alive),
        ("isAlive", actor_is_alive),
        ("mailbox_len", actor_mailbox_len),
        ("mailboxLen", actor_mailbox_len),
        ("len", actor_mailbox_len),
        ("self_pid", actor_self_pid),
        ("selfPid", actor_self_pid),
        ("self", actor_self_pid),
        ("supervisor", actor_supervisor),
        ("register", actor_register),
        ("whereis", actor_whereis),
        ("unregister", actor_unregister),
        ("registered", actor_registered),
        ("cluster_init", cluster_start),
        ("cluster_start", cluster_start),
        ("cluster_join", cluster_join),
        ("cluster_nodes", cluster_peers),
        ("cluster_whereis", cluster_whereis),
    ];
    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: name.to_string(),
                func,
            },
        );
    }
    map.insert("cluster".to_string(), build_cluster_module());
    Value::Map(Arc::new(RwLock::new(map)))
}

pub fn build_cluster_module() -> Value {
    make_map(vec![
        ("node", cluster_node_init),
        ("start", cluster_start),
        ("connect", cluster_join),
        ("join", cluster_join),
        ("peers", cluster_peers),
        ("nodes", cluster_peers),
        ("actor", cluster_actor),
        ("whereis", cluster_whereis),
        ("send", actor_send),
        ("ask", actor_ask),
        ("broadcast", cluster_broadcast),
        ("on_broadcast", cluster_on_broadcast),
        ("stop", cluster_stop),
        ("is_tls", cluster_is_tls),
        ("isTls", cluster_is_tls),
    ])
}

pub(crate) fn parse_shae_child_spec(
    val: &Value,
    ev: &Evaluator,
    span: Span,
) -> Result<ChildSpec, RuntimeError> {
    let map = match val {
        Value::Map(m) => {
            let g = match m.read() {
                Ok(guard) => guard,
                Err(p) => p.into_inner(),
            };
            g.clone()
        }
        _ => {
            return Err(RuntimeError::new(
                "Child spec must be a map with 'id' and 'handler' (or 'start')".into(),
            )
            .at(span));
        }
    };

    let id = match map.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => {
            return Err(RuntimeError::new("Child spec must contain a string 'id'".into()).at(span));
        }
    };

    let handler = map
        .get("handler")
        .or_else(|| map.get("start"))
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(format!(
                "Child spec '{}' must contain a callable 'handler' or 'start'",
                id
            ))
            .at(span)
        })?;

    if !handler.is_callable() {
        return Err(
            RuntimeError::new(format!("Child spec '{}' handler must be a function", id)).at(span),
        );
    }

    let initial_state = map
        .get("state")
        .or_else(|| map.get("initial_state"))
        .map(|v| v.deep_clone());

    let restart_policy = if let Some(r) = map.get("restart").or_else(|| map.get("restart_policy")) {
        match r {
            Value::String(s) => {
                Some(RestartPolicy::from_str_loose(s).map_err(|e| e.to_runtime_error(span))?)
            }
            _ => {
                return Err(RuntimeError::new(
                    "Child spec 'restart' policy must be a string ('permanent', 'transient', 'temporary')".into(),
                )
                .at(span));
            }
        }
    } else {
        None
    };

    let worker_eval = Evaluator::with_env_cache_file(
        ev.global_env.clone(),
        ev.module_cache.clone(),
        ev.current_file.clone(),
    );

    let mut spec = ChildSpec::from_shae(id, initial_state, handler, Some(worker_eval));
    if let Some(p) = restart_policy {
        spec = spec.with_restart(p);
    }
    Ok(spec)
}

fn actor_spawn(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "spawn() expects 1 or 2 arguments: spawn(handler) or spawn(initial_state, handler)"
                .into(),
        )
        .at(span));
    }

    let (initial_state, handler) = if args.len() == 1 {
        let h = args[0].clone();
        if !h.is_callable() {
            return Err(RuntimeError::new("spawn() handler must be a function".into()).at(span));
        }
        (None, h)
    } else {
        let st = args[0].deep_clone();
        let h = args[1].clone();
        if !h.is_callable() {
            return Err(RuntimeError::new("spawn() handler must be a function".into()).at(span));
        }
        (Some(st), h)
    };

    let actor = ActorRef::spawn_with_eval(ev, None, initial_state, handler)
        .map_err(|e| e.to_runtime_error(span))?;
    Ok(Value::Actor(actor))
}

fn actor_spawn_named(
    ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "spawn_named() expects 2 or 3 arguments: spawn_named(name, handler) or spawn_named(name, initial_state, handler)".into(),
        )
        .at(span));
    }

    let name = match &args[0] {
        Value::String(s) => s.clone(),
        _ => {
            return Err(RuntimeError::new(
                "spawn_named() first argument must be a string name".into(),
            )
            .at(span));
        }
    };

    let (initial_state, handler) = if args.len() == 2 {
        let h = args[1].clone();
        if !h.is_callable() {
            return Err(
                RuntimeError::new("spawn_named() handler must be a function".into()).at(span),
            );
        }
        (None, h)
    } else {
        let st = args[1].deep_clone();
        let h = args[2].clone();
        if !h.is_callable() {
            return Err(
                RuntimeError::new("spawn_named() handler must be a function".into()).at(span),
            );
        }
        (Some(st), h)
    };

    if ActorRegistry::whereis(&name).is_some() {
        return Err(ActorError::NameAlreadyRegistered(name).to_runtime_error(span));
    }

    let actor = ActorRef::spawn_with_eval(ev, Some(&name), initial_state, handler)
        .map_err(|e| e.to_runtime_error(span))?;

    Ok(Value::Actor(actor))
}

fn actor_send(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("send", 2, &args, span)?;
    match &args[0] {
        Value::Actor(act) => {
            act.send(args[1].clone())
                .map_err(|e| e.to_runtime_error(span))?;
            Ok(Value::Null)
        }
        Value::RemoteActor(rem) => {
            rem.send(args[1].clone())
                .map_err(|e| e.to_runtime_error(span))?;
            Ok(Value::Null)
        }
        Value::String(name) => {
            if let Some(act) = ActorRegistry::whereis(name) {
                act.send(args[1].clone())
                    .map_err(|e| e.to_runtime_error(span))?;
                Ok(Value::Null)
            } else if name.contains('@') {
                let parts: Vec<&str> = name.splitn(2, '@').collect();
                if let Some(node) = crate::actor_cluster::get_global_cluster() {
                    node.send_to_remote(parts[1], parts[0], args[1].clone())
                        .map_err(|e| e.to_runtime_error(span))?;
                    Ok(Value::Null)
                } else {
                    Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
                }
            } else if let Some(node) = crate::actor_cluster::get_global_cluster() {
                if let Some(Value::RemoteActor(rem)) = node.whereis(name, None) {
                    rem.send(args[1].clone())
                        .map_err(|e| e.to_runtime_error(span))?;
                    Ok(Value::Null)
                } else {
                    Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
                }
            } else {
                Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
            }
        }
        Value::Channel(ch) => {
            ch.send(args[1].clone(), span)?;
            Ok(Value::Null)
        }
        _ => Err(RuntimeError::new(
            "send expects an actor, channel, or registered actor name as first argument".into(),
        )
        .at(span)),
    }
}

fn actor_ask(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "ask() expects 2 or 3 arguments: ask(target, msg, [timeout_ms])".into(),
        )
        .at(span));
    }

    let timeout = if args.len() == 3 {
        match &args[2] {
            Value::Int(ms) if *ms >= 0 => Some(Duration::from_millis(*ms as u64)),
            Value::Float(ms) | Value::Number(ms) if *ms >= 0.0 => {
                Some(Duration::from_millis(*ms as u64))
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
        Value::Actor(act) => {
            let reply = act
                .ask(args[1].clone(), timeout)
                .map_err(|e| e.to_runtime_error(span))?;
            Ok(reply)
        }
        Value::RemoteActor(rem) => {
            let reply = rem
                .ask(args[1].clone(), timeout)
                .map_err(|e| e.to_runtime_error(span))?;
            Ok(reply)
        }
        Value::String(name) => {
            if let Some(act) = ActorRegistry::whereis(name) {
                let reply = act
                    .ask(args[1].clone(), timeout)
                    .map_err(|e| e.to_runtime_error(span))?;
                Ok(reply)
            } else if name.contains('@') {
                let parts: Vec<&str> = name.splitn(2, '@').collect();
                if let Some(node) = crate::actor_cluster::get_global_cluster() {
                    node.ask_remote(parts[1], parts[0], args[1].clone(), timeout)
                        .map_err(|e| e.to_runtime_error(span))
                } else {
                    Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
                }
            } else if let Some(node) = crate::actor_cluster::get_global_cluster() {
                if let Some(Value::RemoteActor(rem)) = node.whereis(name, None) {
                    rem.ask(args[1].clone(), timeout)
                        .map_err(|e| e.to_runtime_error(span))
                } else {
                    Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
                }
            } else {
                Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
            }
        }
        _ => Err(RuntimeError::new(
            "ask expects an actor or registered actor name as first argument".into(),
        )
        .at(span)),
    }
}

fn actor_stop(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("stop", 1, &args, span)?;
    match &args[0] {
        Value::Actor(act) => Ok(Value::Bool(act.stop())),
        Value::Supervisor(sup) => Ok(Value::Bool(sup.stop())),
        Value::String(name) => {
            if let Some(act) = ActorRegistry::whereis(name) {
                let stopped = act.stop();
                ActorRegistry::unregister(name);
                Ok(Value::Bool(stopped))
            } else {
                Ok(Value::Bool(false))
            }
        }
        _ => Err(RuntimeError::new(
            "stop expects an actor, supervisor, or registered actor name as first argument".into(),
        )
        .at(span)),
    }
}

fn actor_is_alive(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("is_alive", 1, &args, span)?;
    match &args[0] {
        Value::Actor(act) => Ok(Value::Bool(act.is_alive())),
        Value::Supervisor(sup) => Ok(Value::Bool(sup.is_alive())),
        Value::String(name) => Ok(Value::Bool(
            ActorRegistry::whereis(name)
                .map(|a| a.is_alive())
                .unwrap_or(false),
        )),
        _ => Err(RuntimeError::new(
            "is_alive expects an actor, supervisor, or registered actor name as first argument"
                .into(),
        )
        .at(span)),
    }
}

fn actor_mailbox_len(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("mailbox_len", 1, &args, span)?;
    match &args[0] {
        Value::Actor(act) => Ok(Value::Int(act.mailbox_len() as i64)),
        Value::String(name) => {
            if let Some(act) = ActorRegistry::whereis(name) {
                Ok(Value::Int(act.mailbox_len() as i64))
            } else {
                Err(ActorError::NameNotFound(name.clone()).to_runtime_error(span))
            }
        }
        _ => Err(RuntimeError::new(
            "mailbox_len expects an actor or registered actor name as first argument".into(),
        )
        .at(span)),
    }
}

fn actor_self_pid(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("self_pid", 0, &args, span)?;
    match crate::actor::self_pid() {
        Some(act) => Ok(Value::Actor(act)),
        None => Ok(Value::Null),
    }
}

fn actor_register(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("register", 2, &args, span)?;
    let name = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("register() name must be a string".into()).at(span)),
    };
    let actor = match &args[1] {
        Value::Actor(act) => Arc::clone(act),
        _ => {
            return Err(
                RuntimeError::new("register() second argument must be an actor".into()).at(span),
            );
        }
    };
    ActorRegistry::register(&name, actor).map_err(|e| e.to_runtime_error(span))?;
    Ok(Value::Bool(true))
}

fn actor_whereis(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("whereis", 1, &args, span)?;
    let name = match &args[0] {
        Value::String(s) => s,
        _ => return Err(RuntimeError::new("whereis() name must be a string".into()).at(span)),
    };
    match ActorRegistry::whereis(name) {
        Some(act) => Ok(Value::Actor(act)),
        None => Ok(Value::Null),
    }
}

fn actor_unregister(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("unregister", 1, &args, span)?;
    let name = match &args[0] {
        Value::String(s) => s,
        _ => return Err(RuntimeError::new("unregister() name must be a string".into()).at(span)),
    };
    match ActorRegistry::unregister(name) {
        Some(act) => Ok(Value::Actor(act)),
        None => Ok(Value::Null),
    }
}

fn actor_registered(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("registered", 0, &args, span)?;
    let names = ActorRegistry::registered();
    let arr = names.into_iter().map(Value::String).collect();
    Ok(Value::Array(Arc::new(RwLock::new(arr))))
}

fn actor_supervisor(
    ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "supervisor() expects 1 or 2 arguments: supervisor(config) or supervisor(config, children)".into(),
        )
        .at(span));
    }

    let mut config = SupervisorConfig::default();
    let mut custom_id: Option<String> = None;
    let mut raw_children: Vec<Value> = Vec::new();

    if args.len() == 1 {
        match &args[0] {
            Value::Array(arr) => {
                let g = match arr.read() {
                    Ok(guard) => guard,
                    Err(p) => p.into_inner(),
                };
                raw_children = g.clone();
            }
            Value::Map(m) => {
                let g = match m.read() {
                    Ok(guard) => guard,
                    Err(p) => p.into_inner(),
                };
                if let Some(strat_val) = g.get("strategy") {
                    if let Value::String(s) = strat_val {
                        config.strategy = RestartStrategy::from_str_loose(s)
                            .map_err(|e| e.to_runtime_error(span))?;
                    }
                }
                if let Some(mr_val) = g.get("max_restarts").or_else(|| g.get("maxRestarts")) {
                    match mr_val {
                        Value::Int(n) if *n >= 0 => config.max_restarts = *n as usize,
                        _ => {
                            return Err(RuntimeError::new(
                                "max_restarts must be a non-negative integer".into(),
                            )
                            .at(span));
                        }
                    }
                }
                if let Some(ws_val) = g.get("within_seconds").or_else(|| g.get("within")) {
                    match ws_val {
                        Value::Float(f) | Value::Number(f) if *f > 0.0 => {
                            config.within_seconds = *f
                        }
                        Value::Int(n) if *n > 0 => config.within_seconds = *n as f64,
                        _ => {
                            return Err(RuntimeError::new(
                                "within_seconds must be a positive number".into(),
                            )
                            .at(span));
                        }
                    }
                }
                if let Some(id_val) = g.get("id").or_else(|| g.get("name")) {
                    if let Value::String(s) = id_val {
                        custom_id = Some(s.clone());
                    }
                }
                if let Some(children_val) = g.get("children").or_else(|| g.get("specs")) {
                    if let Value::Array(arr) = children_val {
                        let cg = match arr.read() {
                            Ok(guard) => guard,
                            Err(p) => p.into_inner(),
                        };
                        raw_children = cg.clone();
                    }
                }
            }
            _ => {
                return Err(RuntimeError::new(
                    "supervisor() argument must be a config map or array of child specs".into(),
                )
                .at(span));
            }
        }
    } else {
        if let Value::Map(m) = &args[0] {
            let g = match m.read() {
                Ok(guard) => guard,
                Err(p) => p.into_inner(),
            };
            if let Some(strat_val) = g.get("strategy") {
                if let Value::String(s) = strat_val {
                    config.strategy =
                        RestartStrategy::from_str_loose(s).map_err(|e| e.to_runtime_error(span))?;
                }
            }
            if let Some(mr_val) = g.get("max_restarts").or_else(|| g.get("maxRestarts")) {
                match mr_val {
                    Value::Int(n) if *n >= 0 => config.max_restarts = *n as usize,
                    _ => {
                        return Err(RuntimeError::new(
                            "max_restarts must be a non-negative integer".into(),
                        )
                        .at(span));
                    }
                }
            }
            if let Some(ws_val) = g.get("within_seconds").or_else(|| g.get("within")) {
                match ws_val {
                    Value::Float(f) | Value::Number(f) if *f > 0.0 => config.within_seconds = *f,
                    Value::Int(n) if *n > 0 => config.within_seconds = *n as f64,
                    _ => {
                        return Err(RuntimeError::new(
                            "within_seconds must be a positive number".into(),
                        )
                        .at(span));
                    }
                }
            }
            if let Some(id_val) = g.get("id").or_else(|| g.get("name")) {
                if let Value::String(s) = id_val {
                    custom_id = Some(s.clone());
                }
            }
        } else {
            return Err(RuntimeError::new("supervisor() config must be a map".into()).at(span));
        }

        match &args[1] {
            Value::Array(arr) => {
                let g = match arr.read() {
                    Ok(guard) => guard,
                    Err(p) => p.into_inner(),
                };
                raw_children = g.clone();
            }
            _ => {
                return Err(RuntimeError::new(
                    "supervisor() children must be an array of child specs".into(),
                )
                .at(span));
            }
        }
    }

    let mut child_specs = Vec::with_capacity(raw_children.len());
    for child_val in &raw_children {
        child_specs.push(parse_shae_child_spec(child_val, ev, span)?);
    }

    let sup = if let Some(name) = custom_id {
        SupervisorRef::start_named(name, config, child_specs)
    } else {
        SupervisorRef::start(config, child_specs)
    }
    .map_err(|e| e.to_runtime_error(span))?;

    Ok(Value::Supervisor(sup))
}

// -------------------------------------------------------------
// std:actor:cluster builtin functions
// -------------------------------------------------------------

fn parse_cluster_tls_from_val(
    val: &Value,
    span: Span,
) -> Result<Option<crate::actor_cluster::ClusterTlsConfig>, RuntimeError> {
    match val {
        Value::Map(m) => {
            let g = m.read().unwrap();
            let cert = match g.get("cert").or_else(|| g.get("certificate")) {
                Some(Value::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(
                        "Cluster TLS options require string 'cert' or 'certificate'".into(),
                    )
                    .at(span));
                }
            };
            let key = match g.get("key").or_else(|| g.get("private_key")) {
                Some(Value::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(
                        "Cluster TLS options require string 'key' or 'private_key'".into(),
                    )
                    .at(span));
                }
            };
            let ca = g
                .get("ca")
                .or_else(|| g.get("ca_cert"))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
            let verify_peer =
                g.get("verify_peer")
                    .or_else(|| g.get("verify"))
                    .and_then(|v| match v {
                        Value::Bool(b) => Some(*b),
                        _ => None,
                    });
            let server_name = g
                .get("server_name")
                .or_else(|| g.get("sni"))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
            Ok(Some(crate::actor_cluster::ClusterTlsConfig {
                cert,
                key,
                ca,
                verify_peer,
                server_name,
            }))
        }
        Value::Null => Ok(None),
        other => Err(RuntimeError::new(format!(
            "Cluster TLS options must be a Map, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn cluster_is_tls(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        let global = crate::actor_cluster::get_global_cluster().ok_or_else(|| {
            RuntimeError::new("No active cluster node found. Start a node first with cluster.node() or cluster.start()".into()).at(span)
        })?;
        Ok(Value::Bool(global.is_tls()))
    } else {
        match &args[0] {
            Value::ClusterNode(node) => Ok(Value::Bool(node.is_tls())),
            other => Err(RuntimeError::new(format!(
                "cluster.is_tls() expects ClusterNode, got {}",
                other.type_name()
            ))
            .at(span)),
        }
    }
}

fn cluster_node_init(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "cluster.node() expects 2 or 3 arguments: node(name, address, [options_or_cookie])"
                .into(),
        )
        .at(span));
    }

    let name = match &args[0] {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::new(format!(
                "cluster.node() expects string name, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let address = match &args[1] {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::new(format!(
                "cluster.node() expects string address, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };

    let (cookie, tls_config) = if args.len() == 3 {
        match &args[2] {
            Value::String(s) => (Some(s.clone()), None),
            Value::Map(m) => {
                let g = m.read().unwrap();
                let cookie = g
                    .get("cookie")
                    .or_else(|| g.get("secret"))
                    .and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    });
                let tls = if let Some(tls_val) = g.get("tls") {
                    parse_cluster_tls_from_val(tls_val, span)?
                } else {
                    None
                };
                (cookie, tls)
            }
            Value::Null => (None, None),
            _ => {
                return Err(RuntimeError::new(
                    "cluster.node() 3rd argument must be a string cookie or options map".into(),
                )
                .at(span));
            }
        }
    } else {
        (None, None)
    };

    let node = crate::actor_cluster::ClusterNode::start_with_tls(name, address, cookie, tls_config)
        .map_err(|e| e.to_runtime_error(span))?;

    if crate::actor_cluster::get_global_cluster().is_none() {
        crate::actor_cluster::set_global_cluster(Arc::clone(&node));
    }

    Ok(Value::ClusterNode(node))
}

fn cluster_start(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let val = cluster_node_init(ev, args, span)?;
    if let Value::ClusterNode(ref node) = val {
        crate::actor_cluster::set_global_cluster(Arc::clone(node));
    }
    Ok(val)
}

fn cluster_join(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "cluster.join() expects 1 or 2 arguments: join([node], peer_address)".into(),
        )
        .at(span));
    }

    let (node, peer_addr) = if args.len() == 1 {
        let global = crate::actor_cluster::get_global_cluster().ok_or_else(|| {
            RuntimeError::new("No active cluster node found. Start a node first with cluster.node() or cluster.start()".into()).at(span)
        })?;
        let addr = match &args[0] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "join() expects string address, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (global, addr)
    } else {
        let n = match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            other => {
                return Err(RuntimeError::new(format!(
                    "join() expects cluster_node as first argument, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        let addr = match &args[1] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "join() expects string address, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (n, addr)
    };

    node.connect(&peer_addr)
        .map_err(|e| e.to_runtime_error(span))?;
    Ok(Value::Bool(true))
}

fn cluster_peers(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let node = if args.is_empty() {
        crate::actor_cluster::get_global_cluster()
            .ok_or_else(|| RuntimeError::new("No active cluster node found".into()).at(span))?
    } else {
        match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            other => {
                return Err(RuntimeError::new(format!(
                    "peers() expects cluster_node, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        }
    };

    let peers = node.peers();
    let values: Vec<Value> = peers.into_iter().map(Value::String).collect();
    Ok(Value::Array(Arc::new(RwLock::new(values))))
}

fn cluster_actor(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "cluster.actor() expects 2 or 3 arguments: actor([node], actor_name, peer_node)".into(),
        )
        .at(span));
    }

    let (node, name, peer) = if args.len() == 2 {
        let global = crate::actor_cluster::get_global_cluster()
            .ok_or_else(|| RuntimeError::new("No active cluster node found".into()).at(span))?;
        let name_str = match &args[0] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "actor() expects string name, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        let peer_str = match &args[1] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "actor() expects string peer_node, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (global, name_str, peer_str)
    } else {
        let n = match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            other => {
                return Err(RuntimeError::new(format!(
                    "actor() expects cluster_node, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        let name_str = match &args[1] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "actor() expects string name, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        let peer_str = match &args[2] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "actor() expects string peer_node, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (n, name_str, peer_str)
    };

    Ok(Value::RemoteActor(node.actor(&name, &peer)))
}

fn cluster_whereis(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 3 {
        return Err(RuntimeError::new(
            "cluster.whereis() expects 1, 2, or 3 arguments: whereis([node], actor_name, [node_hint])".into(),
        )
        .at(span));
    }

    let (node, name, hint) = if matches!(args.first(), Some(Value::ClusterNode(_))) {
        let n = match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            _ => unreachable!(),
        };
        let name_str = match args.get(1) {
            Some(Value::String(s)) => s.clone(),
            _ => {
                return Err(
                    RuntimeError::new("whereis() expects string actor_name".into()).at(span),
                );
            }
        };
        let hint_str = match args.get(2) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        (n, name_str, hint_str)
    } else {
        let global = crate::actor_cluster::get_global_cluster()
            .ok_or_else(|| RuntimeError::new("No active cluster node found".into()).at(span))?;
        let name_str = match &args[0] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "whereis() expects string actor_name, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        let hint_str = match args.get(1) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        (global, name_str, hint_str)
    };

    Ok(node.whereis(&name, hint.as_deref()).unwrap_or(Value::Null))
}

fn cluster_broadcast(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new(
            "cluster.broadcast() expects 2 or 3 arguments: broadcast([node], topic, message)"
                .into(),
        )
        .at(span));
    }

    let (node, topic, msg) = if matches!(args.first(), Some(Value::ClusterNode(_))) {
        let n = match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            _ => unreachable!(),
        };
        let topic_str = match &args[1] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "broadcast() expects string topic, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (n, topic_str, args[2].clone())
    } else {
        let global = crate::actor_cluster::get_global_cluster()
            .ok_or_else(|| RuntimeError::new("No active cluster node found".into()).at(span))?;
        let topic_str = match &args[0] {
            Value::String(s) => s.clone(),
            other => {
                return Err(RuntimeError::new(format!(
                    "broadcast() expects string topic, got {}",
                    other.type_name()
                ))
                .at(span));
            }
        };
        (global, topic_str, args[1].clone())
    };

    node.broadcast(&topic, msg);
    Ok(Value::Null)
}

fn cluster_on_broadcast(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new(
            "cluster.on_broadcast() expects 1 or 2 arguments: on_broadcast([node], callback)"
                .into(),
        )
        .at(span));
    }

    let (node, cb) = if matches!(args.first(), Some(Value::ClusterNode(_))) {
        let n = match &args[0] {
            Value::ClusterNode(node) => Arc::clone(node),
            _ => unreachable!(),
        };
        (n, args[1].clone())
    } else {
        let global = crate::actor_cluster::get_global_cluster()
            .ok_or_else(|| RuntimeError::new("No active cluster node found".into()).at(span))?;
        (global, args[0].clone())
    };

    node.on_broadcast(cb);
    Ok(Value::Null)
}

fn cluster_stop(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        crate::actor_cluster::clear_global_cluster();
        Ok(Value::Bool(true))
    } else {
        match &args[0] {
            Value::ClusterNode(node) => {
                node.stop();
                Ok(Value::Bool(true))
            }
            other => Err(RuntimeError::new(format!(
                "stop() expects cluster_node, got {}",
                other.type_name()
            ))
            .at(span)),
        }
    }
}

// -------------------------------------------------------------
// std:async
// -------------------------------------------------------------
pub fn build_async_module() -> Value {
    make_map(vec![
        ("spawn", async_spawn),
        ("all", async_all),
        ("join_all", async_all),
        ("race", async_race),
        ("race_all", async_race),
        ("sleep", async_sleep),
        ("timeout", async_timeout),
        ("is_done", async_is_done),
        ("is_finished", async_is_done),
        ("status", async_status),
        ("result", async_result),
    ])
}

fn async_spawn(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("async.spawn() expects at least 1 callable argument".into()).at(span),
        );
    }
    let func = args[0].clone();
    let call_args: Vec<Value> = args[1..].to_vec();
    let env = ev.global_env.clone();
    let handle = std::thread::spawn(move || {
        let mut spawn_eval = Evaluator::with_env(env);
        spawn_eval.call_value(&func, call_args, span)
    });
    Ok(Value::Task(Arc::new(std::sync::Mutex::new(
        crate::value::TaskInner::new(handle),
    ))))
}

fn async_sleep(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("async.sleep() expects milliseconds duration".into()).at(span),
        );
    }
    let ms = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Float(f) if *f >= 0.0 => *f as u64,
        Value::Number(n) if *n >= 0.0 => *n as u64,
        other => {
            return Err(RuntimeError::new(format!(
                "async.sleep() expects non-negative number, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        Ok(Value::Int(ms as i64))
    });
    Ok(Value::Task(Arc::new(std::sync::Mutex::new(
        crate::value::TaskInner::new(handle),
    ))))
}

fn async_all(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Ok(Value::Array(Arc::new(RwLock::new(Vec::new()))));
    }
    let tasks_val = &args[0];
    let task_list: Vec<Value> = match tasks_val {
        Value::Array(arr) => arr.read().unwrap().clone(),
        _ => {
            return Err(RuntimeError::new("async.all() expects an array of tasks".into()).at(span));
        }
    };
    let mut results = Vec::with_capacity(task_list.len());
    for item in task_list {
        match item {
            Value::Task(t) => {
                let mut guard = t.lock().unwrap();
                let res = guard.join().map_err(|e| e.or_at(span))?;
                results.push(res);
            }
            other => {
                results.push(other);
            }
        }
    }
    Ok(Value::Array(Arc::new(RwLock::new(results))))
}

fn async_race(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("async.race() expects an array of tasks".into()).at(span));
    }
    let task_list: Vec<Value> = match &args[0] {
        Value::Array(arr) => arr.read().unwrap().clone(),
        _ => {
            return Err(
                RuntimeError::new("async.race() expects an array of tasks".into()).at(span),
            );
        }
    };
    if task_list.is_empty() {
        return Ok(Value::Null);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    for item in task_list {
        match item {
            Value::Task(t) => {
                let tx_clone = tx.clone();
                std::thread::spawn(move || {
                    let mut guard = t.lock().unwrap();
                    let res = guard.join();
                    let _ = tx_clone.send(res);
                });
            }
            other => {
                return Ok(other);
            }
        }
    }
    drop(tx);
    match rx.recv() {
        Ok(Ok(val)) => Ok(val),
        Ok(Err(e)) => Err(e.or_at(span)),
        Err(_) => Err(RuntimeError::new("All raced tasks failed or disconnected".into()).at(span)),
    }
}

fn async_timeout(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(
            RuntimeError::new("async.timeout(ms, task) expects 2 arguments".into()).at(span),
        );
    }
    let ms = match &args[0] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Float(f) if *f >= 0.0 => *f as u64,
        Value::Number(n) if *n >= 0.0 => *n as u64,
        other => {
            return Err(RuntimeError::new(format!(
                "async.timeout() ms must be a number, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let target = args[1].clone();
    match target {
        Value::Task(t) => {
            let (tx, rx) = std::sync::mpsc::channel();
            let t_clone = Arc::clone(&t);
            std::thread::spawn(move || {
                let mut guard = t_clone.lock().unwrap();
                let res = guard.join();
                let _ = tx.send(res);
            });
            match rx.recv_timeout(Duration::from_millis(ms)) {
                Ok(Ok(val)) => Ok(val),
                Ok(Err(e)) => Err(e.or_at(span)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err(RuntimeError::new(format!("Operation timed out after {}ms", ms)).at(span))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err(RuntimeError::new("Task panicked or disconnected".into()).at(span))
                }
            }
        }
        other => Ok(other),
    }
}

fn async_is_done(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("async.is_done(task) expects 1 argument".into()).at(span));
    }
    match &args[0] {
        Value::Task(t) => {
            let guard = t.lock().unwrap();
            Ok(Value::Bool(guard.is_finished()))
        }
        _ => Ok(Value::Bool(true)),
    }
}

fn async_status(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("async.status(task) expects 1 argument".into()).at(span));
    }
    match &args[0] {
        Value::Task(t) => {
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
        _ => Ok(Value::String("completed".into())),
    }
}

fn async_result(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("async.result(task) expects 1 argument".into()).at(span));
    }
    match &args[0] {
        Value::Task(t) => {
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
        other => Ok(other.clone()),
    }
}

// =============================================================
// std:test
// -------------------------------------------------------------
pub fn build_test_module() -> Value {
    make_map(vec![
        ("test", test_test),
        ("assert", crate::builtins::builtin_assert),
        ("assert_eq", crate::builtins::builtin_assert_eq),
        ("assert_ne", crate::builtins::builtin_assert_ne),
        ("assert_throws", crate::builtins::builtin_assert_throws),
        ("panic", crate::builtins::builtin_panic),
    ])
}

fn test_test(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(
            RuntimeError::new("'test' expects 2 arguments (description, callable)".into()).at(span),
        );
    }
    let desc = match &args[0] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let callable = &args[1];
    if !callable.is_callable() {
        return Err(RuntimeError::new(format!(
            "'test' expects a callable function as second argument, got {}",
            callable.type_name()
        ))
        .at(span));
    }

    crate::test_runner::execute_test(&desc, callable, ev, span)
}
