with open("src/eval.rs", "r") as f:
    content = f.read()

use_expr = """
            Expr::Use { path, span } => {
                let path_val = self.eval_expr(path, env)?;
                let path_str = match path_val {
                    Value::String(s) => s,
                    _ => return Err(RuntimeError::new("Module path must be a string".into()).at(*span)),
                };
                
                let source = std::fs::read_to_string(&path_str).map_err(|e| {
                    RuntimeError::new(format!("Failed to read module '{}': {}", path_str, e)).at(*span)
                })?;
                
                let mut builtin_env = crate::env::Environment::new();
                crate::builtins::register(&mut builtin_env);
                let builtin_env_rc = std::rc::Rc::new(std::cell::RefCell::new(builtin_env));
                let module_env = std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new_with_parent(builtin_env_rc)));
                
                let mut ev = crate::eval::Evaluator::with_env(module_env.clone());
                
                let tokens = crate::lexer::tokenize(&source).map_err(|e| RuntimeError::new(format!("Lexer error in module '{}': {}", path_str, e)).at(*span))?;
                let program = crate::parser::parse(tokens).map_err(|e| RuntimeError::new(format!("Parser error in module '{}': {}", path_str, e)).at(*span))?;
                ev.eval_program(&program).map_err(|e| RuntimeError::new(format!("Runtime error in module '{}': {}", path_str, e.message)).at(*span))?;
                
                let map = module_env.borrow().export_map();
                Ok(Value::Map(std::rc::Rc::new(std::cell::RefCell::new(map))))
            }
"""

content = content.replace("            Expr::Interpolated(parts) => {", use_expr.strip() + "\n            Expr::Interpolated(parts) => {")

with open("src/eval.rs", "w") as f:
    f.write(content)
