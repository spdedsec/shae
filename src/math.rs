use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::{Arc, RwLock};

pub fn build_math_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("sqrt", math_sqrt),
        ("abs", math_abs),
        ("min", math_min),
        ("max", math_max),
        ("pow", math_pow),
        ("floor", math_floor),
        ("ceil", math_ceil),
        ("round", math_round),
        ("sin", math_sin),
        ("cos", math_cos),
        ("tan", math_tan),
        ("atan2", math_atan2),
        ("clamp", math_clamp),
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
    // Constants
    map.insert("PI".to_string(), Value::Float(std::f64::consts::PI));
    map.insert("E".to_string(), Value::Float(std::f64::consts::E));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn to_f64(v: &Value, fn_name: &str, span: Span) -> Result<f64, RuntimeError> {
    match v {
        Value::Float(n) | Value::Number(n) => Ok(*n),
        Value::Int(i) => Ok(*i as f64),
        other => Err(RuntimeError::new(format!(
            "{}: expected number, got {}",
            fn_name,
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn math_sqrt(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.sqrt", 1, &args, span)?;
    let val = to_f64(&args[0], "math.sqrt", span)?;
    if val < 0.0 {
        return Ok(Value::Float(f64::NAN));
    }
    Ok(Value::Float(val.sqrt()))
}

pub fn math_abs(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.abs", 1, &args, span)?;
    match &args[0] {
        Value::Int(i) => Ok(Value::Int(i.abs())),
        Value::Float(n) | Value::Number(n) => Ok(Value::Float(n.abs())),
        other => Err(RuntimeError::new(format!(
            "math.abs: expected number, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn math_min(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.min", 2, &args, span)?;
    let a = to_f64(&args[0], "math.min", span)?;
    let b = to_f64(&args[1], "math.min", span)?;
    if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) {
        Ok(Value::Int((a as i64).min(b as i64)))
    } else {
        Ok(Value::Float(a.min(b)))
    }
}

pub fn math_max(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.max", 2, &args, span)?;
    let a = to_f64(&args[0], "math.max", span)?;
    let b = to_f64(&args[1], "math.max", span)?;
    if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) {
        Ok(Value::Int((a as i64).max(b as i64)))
    } else {
        Ok(Value::Float(a.max(b)))
    }
}

pub fn math_pow(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.pow", 2, &args, span)?;
    let base = to_f64(&args[0], "math.pow", span)?;
    let exp = to_f64(&args[1], "math.pow", span)?;
    if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) && exp >= 0.0 && exp <= 30.0 {
        Ok(Value::Int((base as i64).pow(exp as u32)))
    } else {
        Ok(Value::Float(base.powf(exp)))
    }
}

pub fn math_floor(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.floor", 1, &args, span)?;
    let val = to_f64(&args[0], "math.floor", span)?;
    Ok(Value::Int(val.floor() as i64))
}

pub fn math_ceil(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.ceil", 1, &args, span)?;
    let val = to_f64(&args[0], "math.ceil", span)?;
    Ok(Value::Int(val.ceil() as i64))
}

pub fn math_round(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.round", 1, &args, span)?;
    let val = to_f64(&args[0], "math.round", span)?;
    Ok(Value::Int(val.round() as i64))
}

pub fn math_sin(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.sin", 1, &args, span)?;
    let val = to_f64(&args[0], "math.sin", span)?;
    Ok(Value::Float(val.sin()))
}

pub fn math_cos(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.cos", 1, &args, span)?;
    let val = to_f64(&args[0], "math.cos", span)?;
    Ok(Value::Float(val.cos()))
}

pub fn math_tan(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.tan", 1, &args, span)?;
    let val = to_f64(&args[0], "math.tan", span)?;
    Ok(Value::Float(val.tan()))
}

pub fn math_atan2(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.atan2", 2, &args, span)?;
    let y = to_f64(&args[0], "math.atan2", span)?;
    let x = to_f64(&args[1], "math.atan2", span)?;
    Ok(Value::Float(y.atan2(x)))
}

pub fn math_clamp(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math.clamp", 3, &args, span)?;
    let val = to_f64(&args[0], "math.clamp", span)?;
    let min = to_f64(&args[1], "math.clamp", span)?;
    let max = to_f64(&args[2], "math.clamp", span)?;
    if matches!(&args[0], Value::Int(_)) && matches!(&args[1], Value::Int(_)) && matches!(&args[2], Value::Int(_)) {
        Ok(Value::Int((val as i64).clamp(min as i64, max as i64)))
    } else {
        Ok(Value::Float(val.clamp(min, max)))
    }
}
