use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
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
        _ => Err(RuntimeError::new(format!(
            "Unknown standard library module 'std:{}'. Available: std:fs, std:path, std:sys, std:time",
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
            Err(e) => Err(RuntimeError::new(format!("fs.read: Failed to read '{}': {}", path, e)).at(span)),
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
            Err(e) => Err(RuntimeError::new(format!("fs.write: Failed to write '{}': {}", path, e)).at(span)),
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
            .map_err(|e| RuntimeError::new(format!("fs.append: Failed to open '{}': {}", path, e)).at(span))?;
        file.write_all(content.as_bytes())
            .map_err(|e| RuntimeError::new(format!("fs.append: Failed to write '{}': {}", path, e)).at(span))?;
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
            Err(e) => Err(RuntimeError::new(format!("fs.remove: Failed to delete '{}': {}", path, e)).at(span)),
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
            Err(e) => Err(RuntimeError::new(format!("fs.mkdir: Failed to create '{}': {}", path, e)).at(span)),
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
            Err(e) => Err(RuntimeError::new(format!("fs.readDir: Failed to read directory '{}': {}", path, e)).at(span)),
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

fn sys_platform(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    Ok(Value::String(std::env::consts::OS.to_string()))
}

fn sys_arch(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    Ok(Value::String(std::env::consts::ARCH.to_string()))
}

fn sys_cwd(_ev: &mut Evaluator, _args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    match std::env::current_dir() {
        Ok(p) => Ok(Value::String(p.to_string_lossy().to_string())),
        Err(e) => Err(RuntimeError::new(format!("sys.cwd: Failed to get current dir: {}", e)).at(span)),
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
        Value::Int(i) => *i as u64,
        Value::Float(f) | Value::Number(f) => *f as u64,
        _ => return Err(RuntimeError::new("time.sleep expects milliseconds as number".into()).at(span)),
    };
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Ok(Value::Null)
}
