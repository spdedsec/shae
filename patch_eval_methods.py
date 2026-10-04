with open("src/eval.rs", "r") as f:
    content = f.read()

arr_get = """
                    Value::Array(a) => {
                        if property == "len" {
                            Ok(Some(Value::Number(a.borrow().len() as f64)))
                        } else if property == "first" {
                            let val = a.borrow().first().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new(
                                    "Cannot get 'first' of an empty array".into(),
                                )
                                .at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "last" {
                            let val = a.borrow().last().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(
                                    RuntimeError::new("Cannot get 'last' of an empty array".into())
                                        .at(*span),
                                )
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "push" || property == "pop" || property == "map" || property == "filter" || property == "reduce" || property == "sum" || property == "sort" {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::Array(a.clone())),
                                method: property.clone()
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "Property '{}' not found on array",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
"""

str_get = """
                    Value::String(s) => {
                        if property == "len" {
                            Ok(Some(Value::Number(s.chars().count() as f64)))
                        } else if property == "trim" || property == "upper" || property == "lower" || property == "split" || property == "replace" {
                            Ok(Some(Value::BoundMethod {
                                object: Box::new(Value::String(s.clone())),
                                method: property.clone()
                            }))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "Property '{}' not found on string",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
"""

# We need to replace the old Array and String getter blocks in `eval_chain`.
import re
# Regex to match Value::Array(a) => { ... } up to Value::String(s) => { ... }
# It is better to just find where `Value::Array(a) => {` starts and replace up to `Value::Map` or whatever follows.
