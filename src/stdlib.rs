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
use std::sync::{Arc, RwLock};

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
        _ => Err(RuntimeError::new(format!(
            "Unknown standard library module 'std:{}'. Available: std:fs, std:path, std:sys, std:time, std:crypto, std:codec, std:regex",
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
