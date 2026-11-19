use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

pub fn build_io_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("print", io_print),
        ("println", io_println),
        ("input", io_input),
        ("flush", io_flush),
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

fn io_print(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let out: Vec<String> = args.iter().map(|v| v.to_display()).collect();
    print!("{}", out.join(" "));
    let _ = std::io::stdout().flush();
    Ok(Value::Null)
}

fn io_println(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let out: Vec<String> = args.iter().map(|v| v.to_display()).collect();
    println!("{}", out.join(" "));
    Ok(Value::Null)
}

fn io_input(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    crate::builtins::builtin_input(_ev, args, span)
}

fn io_flush(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let _ = std::io::stdout().flush();
    Ok(Value::Null)
}
