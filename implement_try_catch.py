import re

# 1. token.rs
with open("src/token.rs", "r") as f:
    content = f.read()
if "Try," not in content:
    content = content.replace("Continue,", "Continue,\n    Try,\n    Catch,")
    content = content.replace("Token::Continue => write!(f, \"'continue'\"),", "Token::Continue => write!(f, \"'continue'\"),\n            Token::Try => write!(f, \"'try'\"),\n            Token::Catch => write!(f, \"'catch'\"),")
with open("src/token.rs", "w") as f:
    f.write(content)

# 2. lexer.rs
with open("src/lexer.rs", "r") as f:
    content = f.read()
if '"try" =>' not in content:
    content = content.replace('"continue" => Token::Continue,', '"continue" => Token::Continue,\n            "try" => Token::Try,\n            "catch" => Token::Catch,')
with open("src/lexer.rs", "w") as f:
    f.write(content)

# 3. ast.rs
with open("src/ast.rs", "r") as f:
    content = f.read()
if "TryCatch {" not in content:
    content = content.replace("While {", "TryCatch {\n        try_block: Box<Expr>,\n        catch_ident: String,\n        catch_block: Box<Expr>,\n        span: Span,\n    },\n    While {")
    content = content.replace("Expr::While { span, .. } => Some(*span),", "Expr::While { span, .. } => Some(*span),\n            Expr::TryCatch { span, .. } => Some(*span),")
with open("src/ast.rs", "w") as f:
    f.write(content)

# 4. parser.rs
with open("src/parser.rs", "r") as f:
    content = f.read()
if "Token::Try =>" not in content:
    content = content.replace("Token::While => self.parse_while(),", "Token::While => self.parse_while(),\n            Token::Try => self.parse_try_catch(),")
    
    try_catch_func = """
    fn parse_try_catch(&mut self) -> Result<Expr, ParserError> {
        let span = self.peek().clone();
        self.advance(); // consume 'try'
        
        let try_block = self.parse_block()?;
        
        if !self.match_token(Token::Catch) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected 'catch' after try block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        let catch_ident = if let Token::Ident(name) = &self.peek().token {
            let n = name.clone();
            self.advance();
            n
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected identifier for catch block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };
        
        let catch_block = self.parse_block()?;
        
        Ok(Expr::TryCatch {
            try_block: Box::new(try_block),
            catch_ident,
            catch_block: Box::new(catch_block),
            span: Span { line: span.line, col: span.col },
        })
    }
    
    fn parse_while"""
    content = content.replace("fn parse_while", try_catch_func)
with open("src/parser.rs", "w") as f:
    f.write(content)

# 5. eval.rs
with open("src/eval.rs", "r") as f:
    content = f.read()
if "Expr::TryCatch {" not in content:
    eval_logic = """
            Expr::TryCatch { try_block, catch_ident, catch_block, span: _ } => {
                let try_res = self.eval_expr(try_block, env);
                match try_res {
                    Ok(v) => Ok(v),
                    Err(e) => {
                        let catch_env = Environment::new(Some(env.clone()));
                        catch_env.define(catch_ident.clone(), Value::String(e.message));
                        self.eval_expr(catch_block, &catch_env)
                    }
                }
            }
            Expr::While {"""
    content = content.replace("Expr::While {", eval_logic)
with open("src/eval.rs", "w") as f:
    f.write(content)
