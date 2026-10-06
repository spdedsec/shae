with open("src/eval.rs", "r") as f:
    content = f.read()

target = """            StmtKind::TryCatch { try_body, catch_ident, catch_body } => {
                let try_env = Environment::new(Some(env.clone()));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = Environment::new(Some(env.clone()));
                        catch_env.define(catch_ident.clone(), Value::String(e.message));
                        self.eval_block(catch_body, &catch_env)
                    }
                }
            }"""

replacement = """            StmtKind::TryCatch { try_body, catch_ident, catch_body } => {
                let try_env = Rc::new(RefCell::new(Environment::new_with_parent(env.clone())));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = Rc::new(RefCell::new(Environment::new_with_parent(env.clone())));
                        catch_env.borrow_mut().define(catch_ident.clone(), Value::String(e.message));
                        self.eval_block(catch_body, &catch_env)
                    }
                }
            }"""

content = content.replace(target, replacement)
with open("src/eval.rs", "w") as f:
    f.write(content)
