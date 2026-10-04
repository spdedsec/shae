with open("src/eval.rs", "r") as f:
    content = f.read()

target = """        };        self.depth -= 1;
        res"""
replacement = """        };
        self.depth -= 1;
        match res {
            Ok(v) => Ok(v),
            Err(mut e) => {
                let func_name = match callee {
                    Value::Function { name, .. } => name.clone().unwrap_or_else(|| "anonymous".to_string()),
                    Value::Builtin { name, .. } => name.clone(),
                    Value::BoundMethod { method, .. } => method.clone(),
                    Value::EnumConstructor { variant_name, .. } => variant_name.clone(),
                    _ => "unknown".to_string(),
                };
                e.stack.push((func_name, span));
                Err(e)
            }
        }"""
content = content.replace(target, replacement)
with open("src/eval.rs", "w") as f:
    f.write(content)
