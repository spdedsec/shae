with open("src/parser.rs", "r") as f:
    content = f.read()

target = """        } else if self.match_token(Token::While) {
            self.parse_while_statement(cur.line, cur.col)?
        } else if self.match_token(Token::For) {"""

replacement = """        } else if self.match_token(Token::Try) {
            self.parse_try_catch_statement(cur.line, cur.col)?
        } else if self.match_token(Token::While) {
            self.parse_while_statement(cur.line, cur.col)?
        } else if self.match_token(Token::For) {"""

content = content.replace(target, replacement)

func = """
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

    fn parse_while_statement"""

content = content.replace("    fn parse_while_statement", func)

with open("src/parser.rs", "w") as f:
    f.write(content)
