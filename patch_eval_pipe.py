with open("src/eval.rs", "r") as f:
    content = f.read()

target = """                if *op == BinaryOp::Coalesce {
                    let left_val = self.eval_chain(left, env, true)?;
                    if let Some(v) = left_val {
                        if !matches!(v, Value::Null) {
                            return Ok(v);
                        }
                    }
                    return self.eval_expr(right, env);
                }"""

replacement = """                if *op == BinaryOp::Coalesce {
                    let left_val = self.eval_chain(left, env, true)?;
                    if let Some(v) = left_val {
                        if !matches!(v, Value::Null) {
                            return Ok(v);
                        }
                    }
                    return self.eval_expr(right, env);
                }
                
                if *op == BinaryOp::Pipe {
                    let left_val = self.eval_expr(left, env)?;
                    match &**right {
                        Expr::Call { callee, args, span: call_span } => {
                            let callee_val = self.eval_expr(callee, env)?;
                            let mut evaled_args = vec![left_val];
                            for arg in args {
                                evaled_args.push(self.eval_expr(arg, env)?);
                            }
                            return self.call_value(&callee_val, evaled_args, *call_span);
                        }
                        _ => {
                            let func_val = self.eval_expr(right, env)?;
                            return self.call_value(&func_val, vec![left_val], *span);
                        }
                    }
                }"""

content = content.replace(target, replacement)
with open("src/eval.rs", "w") as f:
    f.write(content)
