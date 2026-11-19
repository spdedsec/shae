use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::{Arc, RwLock};

pub fn build_random_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("int", rand_int),
        ("float", rand_float),
        ("bool", rand_bool),
        ("choice", rand_choice),
        ("shuffle", rand_shuffle),
        ("seed", rand_seed),
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
    Value::Map(Arc::new(RwLock::new(map)))
}

fn rand_int(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    match args.len() {
        0 => Ok(Value::Int(fastrand::i64(0..=i64::MAX))),
        1 => {
            let max = match &args[0] {
                Value::Int(i) => *i,
                Value::Float(n) | Value::Number(n) => *n as i64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.int: expected number, got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            if max <= 0 {
                return Ok(Value::Int(0));
            }
            Ok(Value::Int(fastrand::i64(0..=max)))
        }
        2 => {
            let min = match &args[0] {
                Value::Int(i) => *i,
                Value::Float(n) | Value::Number(n) => *n as i64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.int: expected number for min, got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            let max = match &args[1] {
                Value::Int(i) => *i,
                Value::Float(n) | Value::Number(n) => *n as i64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.int: expected number for max, got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            if min >= max {
                return Ok(Value::Int(min));
            }
            Ok(Value::Int(fastrand::i64(min..=max)))
        }
        _ => Err(RuntimeError::new("random.int: accepts 0, 1, or 2 arguments".into()).at(span)),
    }
}

fn rand_float(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    match args.len() {
        0 => Ok(Value::Float(fastrand::f64())),
        2 => {
            let min = match &args[0] {
                Value::Float(n) | Value::Number(n) => *n,
                Value::Int(i) => *i as f64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.float: expected number for min, got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            let max = match &args[1] {
                Value::Float(n) | Value::Number(n) => *n,
                Value::Int(i) => *i as f64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.float: expected number for max, got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            if min >= max {
                return Ok(Value::Float(min));
            }
            Ok(Value::Float(min + fastrand::f64() * (max - min)))
        }
        _ => Err(RuntimeError::new("random.float: accepts 0 or 2 arguments".into()).at(span)),
    }
}

fn rand_bool(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    match args.len() {
        0 => Ok(Value::Bool(fastrand::bool())),
        1 => {
            let prob = match &args[0] {
                Value::Float(n) | Value::Number(n) => *n,
                Value::Int(i) => *i as f64,
                other => {
                    return Err(RuntimeError::new(format!(
                        "random.bool: expected probability float (0.0 to 1.0), got {}",
                        other.type_name()
                    ))
                    .at(span));
                }
            };
            Ok(Value::Bool(fastrand::f64() < prob))
        }
        _ => Err(RuntimeError::new("random.bool: accepts 0 or 1 arguments".into()).at(span)),
    }
}

fn rand_choice(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("random.choice", 1, &args, span)?;
    match &args[0] {
        Value::Array(arr) => {
            let read_lock = arr.read().unwrap();
            if read_lock.is_empty() {
                return Ok(Value::Null);
            }
            let idx = fastrand::usize(0..read_lock.len());
            Ok(read_lock[idx].clone())
        }
        other => Err(RuntimeError::new(format!(
            "random.choice: expected array, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn rand_shuffle(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("random.shuffle", 1, &args, span)?;
    match &args[0] {
        Value::Array(arr) => {
            let mut list = arr.read().unwrap().clone();
            fastrand::shuffle(&mut list);
            Ok(Value::Array(Arc::new(RwLock::new(list))))
        }
        other => Err(RuntimeError::new(format!(
            "random.shuffle: expected array, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

fn rand_seed(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("random.seed", 1, &args, span)?;
    let seed_val = match &args[0] {
        Value::Int(i) => *i as u64,
        Value::Float(n) | Value::Number(n) => *n as u64,
        other => {
            return Err(RuntimeError::new(format!(
                "random.seed: expected integer seed, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    fastrand::seed(seed_val);
    Ok(Value::Null)
}
