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
    content = content.replace("While {", "TryCatch {\n        try_body: Vec<Stmt>,\n        catch_ident: String,\n        catch_body: Vec<Stmt>,\n    },\n    While {")
with open("src/ast.rs", "w") as f:
    f.write(content)

# 4. parser.rs
with open("src/parser.rs", "r") as f:
    content = f.read()
if "Token::Try =>" not in content:
    content = content.replace("Token::While => {", "Token::Try => {\n                let line = self.peek().line;\n                let col = self.peek().col;\n                self.advance();\n                self.parse_try_catch_statement(line, col)\n            }\n            Token::While => {")
    
    try_catch_func = """
    fn parse_try_catch_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' after try block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        let mut try_body = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            try_body.push(self.parse_statement()?);
        }
        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after try block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        if !self.match_token(Token::Catch) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected 'catch' after try block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        if !self.match_token(Token::LParen) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '(' after catch.".into(),
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
                message: "Expected identifier for catch variable.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };
        
        if !self.match_token(Token::RParen) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected ')' after catch variable.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' after catch clause.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        let mut catch_body = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            catch_body.push(self.parse_statement()?);
        }
        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after catch block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        
        Ok(Stmt {
            kind: crate::ast::StmtKind::TryCatch { try_body, catch_ident, catch_body },
            span: crate::ast::Span { line, col },
        })
    }
    
    fn parse_while"""
    content = content.replace("fn parse_while", try_catch_func)
with open("src/parser.rs", "w") as f:
    f.write(content)

# 5. eval.rs
with open("src/eval.rs", "r") as f:
    content = f.read()
if "StmtKind::TryCatch {" not in content:
    eval_logic = """
            StmtKind::TryCatch { try_body, catch_ident, catch_body } => {
                let try_env = Environment::new(Some(env.clone()));
                match self.eval_block(try_body, &try_env) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        let catch_env = Environment::new(Some(env.clone()));
                        catch_env.define(catch_ident.clone(), Value::String(e.message));
                        self.eval_block(catch_body, &catch_env)
                    }
                }
            }
            StmtKind::While {"""
    content = content.replace("StmtKind::While {", eval_logic)
with open("src/eval.rs", "w") as f:
    f.write(content)
