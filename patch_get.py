with open("src/eval.rs", "r") as f:
    content = f.read()

arr_get_old = """
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
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "Array has no property '{}'",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
"""

arr_get_new = """
                    Value::Array(a) => {
                        if property == "len" {
                            Ok(Some(Value::Number(a.borrow().len() as f64)))
                        } else if property == "first" {
                            let val = a.borrow().first().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'first' of an empty array".into()).at(*span))
                            } else {
                                Ok(Some(val))
                            }
                        } else if property == "last" {
                            let val = a.borrow().last().cloned().unwrap_or(Value::Null);
                            if matches!(val, Value::Null) && !(*safe || lenient) {
                                Err(RuntimeError::new("Cannot get 'last' of an empty array".into()).at(*span))
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
                                Err(RuntimeError::new(format!("Array has no property '{}'", property)).at(*span))
                            }
                        }
                    }
"""

str_get_old = """
                    Value::String(s) => {
                        if property == "len" {
                            Ok(Some(Value::Number(s.len() as f64)))
                        } else {
                            if *safe || lenient {
                                Ok(None)
                            } else {
                                Err(RuntimeError::new(format!(
                                    "String has no property '{}'",
                                    property
                                ))
                                .at(*span))
                            }
                        }
                    }
"""

str_get_new = """
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
                                Err(RuntimeError::new(format!("String has no property '{}'", property)).at(*span))
                            }
                        }
                    }
"""

content = content.replace(arr_get_old.strip(), arr_get_new.strip())
content = content.replace(str_get_old.strip(), str_get_new.strip())

with open("src/eval.rs", "w") as f:
    f.write(content)
