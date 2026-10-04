with open("src/eval.rs", "r") as f:
    content = f.read()

bound_method = """
            Value::BoundMethod { object, method } => {
                match (*object.clone(), method.as_str()) {
                    // Array methods
                    (Value::Array(a), "push") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("push() expects 1 argument".into()).at(span));
                        }
                        a.borrow_mut().push(args[0].clone());
                        Ok(Value::Null)
                    }
                    (Value::Array(a), "pop") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("pop() expects 0 arguments".into()).at(span));
                        }
                        if let Some(val) = a.borrow_mut().pop() {
                            Ok(val)
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    (Value::Array(a), "map") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("map() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("map() argument must be a function".into()).at(span));
                        }
                        let mut new_arr = Vec::new();
                        for item in a.borrow().iter() {
                            let mapped = self.call_value(func, vec![item.clone()], span)?;
                            new_arr.push(mapped);
                        }
                        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(new_arr))))
                    }
                    (Value::Array(a), "filter") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("filter() expects 1 argument (a function)".into()).at(span));
                        }
                        let func = &args[0];
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("filter() argument must be a function".into()).at(span));
                        }
                        let mut new_arr = Vec::new();
                        for item in a.borrow().iter() {
                            let keep = self.call_value(func, vec![item.clone()], span)?;
                            if keep.is_truthy() {
                                new_arr.push(item.clone());
                            }
                        }
                        Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(new_arr))))
                    }
                    (Value::Array(a), "reduce") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("reduce() expects 2 arguments (function, initial_value)".into()).at(span));
                        }
                        let func = &args[0];
                        let mut acc = args[1].clone();
                        if !matches!(func, Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. }) {
                            self.depth -= 1;
                            return Err(RuntimeError::new("reduce() first argument must be a function".into()).at(span));
                        }
                        for item in a.borrow().iter() {
                            acc = self.call_value(func, vec![acc, item.clone()], span)?;
                        }
                        Ok(acc)
                    }
                    (Value::Array(a), "sum") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sum() expects 0 arguments".into()).at(span));
                        }
                        let mut sum = 0.0;
                        for item in a.borrow().iter() {
                            if let Value::Number(n) = item {
                                sum += n;
                            } else {
                                self.depth -= 1;
                                return Err(RuntimeError::new(format!("Cannot sum non-number: {}", item.type_name())).at(span));
                            }
                        }
                        Ok(Value::Number(sum))
                    }
                    (Value::Array(a), "sort") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("sort() expects 0 arguments".into()).at(span));
                        }
                        let mut arr = a.borrow_mut();
                        arr.sort_by(|x, y| {
                            match (x, y) {
                                (Value::Number(nx), Value::Number(ny)) => nx.partial_cmp(ny).unwrap_or(std::cmp::Ordering::Equal),
                                (Value::String(sx), Value::String(sy)) => sx.cmp(sy),
                                _ => std::cmp::Ordering::Equal,
                            }
                        });
                        Ok(Value::Array(a.clone()))
                    }
                    // String methods
                    (Value::String(s), "trim") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("trim() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.trim().to_string()))
                    }
                    (Value::String(s), "upper") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("upper() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.to_uppercase()))
                    }
                    (Value::String(s), "lower") => {
                        if !args.is_empty() {
                            self.depth -= 1;
                            return Err(RuntimeError::new("lower() expects 0 arguments".into()).at(span));
                        }
                        Ok(Value::String(s.to_lowercase()))
                    }
                    (Value::String(s), "split") => {
                        if args.len() != 1 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("split() expects 1 argument (delimiter)".into()).at(span));
                        }
                        if let Value::String(delim) = &args[0] {
                            let parts: Vec<Value> = s.split(delim).map(|p| Value::String(p.to_string())).collect();
                            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(parts))))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("split() delimiter must be a string".into()).at(span))
                        }
                    }
                    (Value::String(s), "replace") => {
                        if args.len() != 2 {
                            self.depth -= 1;
                            return Err(RuntimeError::new("replace() expects 2 arguments (old, new)".into()).at(span));
                        }
                        if let (Value::String(old), Value::String(new)) = (&args[0], &args[1]) {
                            Ok(Value::String(s.replace(old, new)))
                        } else {
                            self.depth -= 1;
                            Err(RuntimeError::new("replace() arguments must be strings".into()).at(span))
                        }
                    }
                    _ => {
                        self.depth -= 1;
                        Err(RuntimeError::new(format!("Method {} not found", method)).at(span))
                    }
                }
            }
"""

content = content.replace("            Value::Builtin { func, .. } => func(self, args, span),", "            Value::Builtin { func, .. } => func(self, args, span),\n" + bound_method)
with open("src/eval.rs", "w") as f:
    f.write(content)
