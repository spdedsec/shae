use crate::ast::{BinaryOp, BindingPattern, Expr, InterpPart, Literal, Program, Span, Stmt, StmtKind, UnaryOp, UseItem};
use crate::lexer::LexerError;
use crate::token::{SpannedToken, StrPart, Token};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("Shae Syntax Error [line {line}:{col}]: {message}")]
    UnexpectedToken {
        message: String,
        line: usize,
        col: usize,
    },
    #[error("Shae Friendly Advice [line {line}:{col}]: {advice}")]
    Advice {
        advice: String,
        line: usize,
        col: usize,
    },
    #[error(transparent)]
    Lexer(#[from] LexerError),
}

pub struct Parser {
    tokens: Vec<SpannedToken>,
    cursor: usize,
    allow_struct: bool,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, cursor: 0, allow_struct: true }
    }

    pub fn parse(&mut self) -> Result<Program, ParserError> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.parse_statement()?);
        }
        Ok(Program { statements })
    }

    fn peek(&self) -> &SpannedToken {
        &self.tokens[self.cursor]
    }

    fn peek_next(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.cursor + 1)
    }

    fn advance(&mut self) -> &SpannedToken {
        if !self.is_at_end() {
            self.cursor += 1;
        }
        &self.tokens[self.cursor - 1]
    }

    fn match_token(&mut self, token: Token) -> bool {
        if self.peek().token == token {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, token: &Token) -> bool {
        &self.peek().token == token
    }

    fn consume(&mut self, token: Token, message: &str) -> Result<&SpannedToken, ParserError> {
        if self.peek().token == token {
            Ok(self.advance())
        } else {
            Err(ParserError::UnexpectedToken {
                message: message.to_string(),
                line: self.peek().line,
                col: self.peek().col,
            })
        }
    }

    fn is_at_end(&self) -> bool {
        self.peek().token == Token::Eof
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParserError> {
        let cur = self.peek().clone();

        // Advice for common mistakes
        if let Token::Ident(name) = &cur.token {
            match name.as_str() {
                "def" | "function" | "func" | "fun" | "sub" => {
                    return Err(ParserError::Advice {
                        advice: format!(
                            "In Shae, we use 'fn' for functions (e.g., 'fn {}() {{ ... }}'). Keep it short and sweet!",
                            self.peek_next()
                                .map(|t| match &t.token {
                                    Token::Ident(n) => n.as_str(),
                                    _ => "name",
                                })
                                .unwrap_or("name")
                        ),
                        line: cur.line,
                        col: cur.col,
                    });
                }
                "var" | "const" | "int" | "float" | "string" | "auto" | "val" => {
                    if let Some(next) = self.peek_next() {
                        if matches!(next.token, Token::Ident(_)) {
                            return Err(ParserError::Advice {
                                advice: "In Shae, use 'let' to declare variables (e.g., 'let count = 0').".to_string(),
                                line: cur.line,
                                col: cur.col,
                            });
                        }
                    }
                }
                "elif" | "elsif" | "elseif" => {
                    return Err(ParserError::Advice {
                        advice: "In Shae, use 'else if' instead of 'elif'.".to_string(),
                        line: cur.line,
                        col: cur.col,
                    });
                }
                _ => {}
            }
        }

        let stmt = if self.match_token(Token::Let) {
            self.parse_let_statement(cur.line, cur.col)?
} else if self.match_token(Token::Struct) {
            self.parse_struct_def(cur.line, cur.col)?
        } else if self.match_token(Token::Enum) {
            self.parse_enum_def(cur.line, cur.col)?
        } else if self.match_token(Token::Fn) {
            self.parse_fn_statement(cur.line, cur.col)?
        } else if self.match_token(Token::If) {
            self.parse_if_statement(cur.line, cur.col)?
        } else if self.match_token(Token::Try) {
            self.parse_try_catch_statement(cur.line, cur.col)?
        } else if self.match_token(Token::While) {
            self.parse_while_statement(cur.line, cur.col)?
        } else if self.match_token(Token::For) {
            self.parse_for_statement(cur.line, cur.col)?
        } else if self.match_token(Token::Return) {
            self.parse_return_statement(cur.line, cur.col)?
        } else if self.match_token(Token::Break) {
            let stmt = Stmt {
                kind: StmtKind::Break,
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            };
            self.end_statement()?;
            stmt
        } else if self.match_token(Token::Continue) {
            let stmt = Stmt {
                kind: StmtKind::Continue,
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            };
            self.end_statement()?;
            stmt
        } else if self.match_token(Token::Use) {
            self.parse_use_statement(cur.line, cur.col)?
        } else {
            self.parse_expression_statement()?
        };
        Ok(stmt)
    }

    fn parse_use_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let span = Span { line, col };
        if self.match_token(Token::LBrace) {
            let mut imports = Vec::new();
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let name = if let Token::Ident(n) = &self.peek().token {
                    let n = n.clone();
                    self.advance();
                    n
                } else if self.match_token(Token::Star) {
                    "*".to_string()
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected identifier or '*' in import list.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                };

                let alias = if self.match_token(Token::As) || self.match_token(Token::Colon) {
                    if let Token::Ident(a) = &self.peek().token {
                        let a = a.clone();
                        self.advance();
                        Some(a)
                    } else {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected identifier after 'as' or ':' in import list.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                } else {
                    None
                };

                imports.push(UseItem { name, alias });

                if !self.match_token(Token::Comma) {
                    break;
                }
            }

            self.consume(Token::RBrace, "Expected '}' after import list.")?;
            self.consume(Token::From, "Expected 'from' after import list.")?;

            let path = if let Token::StringLit(s) = &self.peek().token {
                let s = s.clone();
                self.advance();
                s
            } else {
                return Err(ParserError::UnexpectedToken {
                    message: "Expected module path string after 'from'.".into(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            };

            self.end_statement()?;
            Ok(Stmt {
                kind: StmtKind::Use { imports, path },
                span,
            })
        } else if self.match_token(Token::Star) {
            self.consume(Token::From, "Expected 'from' after '*'.")?;
            let path = if let Token::StringLit(s) = &self.peek().token {
                let s = s.clone();
                self.advance();
                s
            } else {
                return Err(ParserError::UnexpectedToken {
                    message: "Expected module path string after 'from'.".into(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            };
            self.end_statement()?;
            Ok(Stmt {
                kind: StmtKind::Use {
                    imports: vec![UseItem {
                        name: "*".to_string(),
                        alias: None,
                    }],
                    path,
                },
                span,
            })
        } else {
            let expr = self.parse_expression()?;
            self.end_statement()?;
            Ok(Stmt {
                kind: StmtKind::Expr(Expr::Use {
                    path: Box::new(expr),
                    span,
                }),
                span,
            })
        }
    }

fn parse_struct_def(&mut self, _line: usize, _col: usize) -> Result<Stmt, ParserError> {
        let name = if let Token::Ident(n) = &self.advance().token {
            n.clone()
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected struct name.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' before struct fields.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut fields = Vec::new();
        if self.peek().token != Token::RBrace {
            loop {
                if let Token::Ident(f) = &self.advance().token {
                    fields.push(f.clone());
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected field name.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                if !self.match_token(Token::Comma) {
                    break;
                }
            }
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after struct fields.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        Ok(Stmt { kind: StmtKind::StructDef { name, fields }, span: Span { line: _line, col: _col } })
    }

    fn parse_enum_def(&mut self, _line: usize, _col: usize) -> Result<Stmt, ParserError> {
        use crate::ast::EnumVariant;
        
        let name = if let Token::Ident(n) = &self.advance().token {
            n.clone()
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected enum name.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' before enum variants.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut variants = Vec::new();
        if self.peek().token != Token::RBrace {
            loop {
                let variant_name = if let Token::Ident(v) = &self.advance().token {
                    v.clone()
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected variant name.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                };
                
                let mut fields = Vec::new();
                if self.match_token(Token::LParen) {
                    if self.peek().token != Token::RParen {
                        loop {
                            if let Token::Ident(f) = &self.advance().token {
                                fields.push(f.clone());
                            } else {
                                return Err(ParserError::UnexpectedToken {
                                    message: "Expected field name.".into(),
                                    line: self.peek().line,
                                    col: self.peek().col,
                                });
                            }
                            if !self.match_token(Token::Comma) {
                                break;
                            }
                        }
                    }
                    if !self.match_token(Token::RParen) {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected ')' after variant fields.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                }
                
                variants.push(EnumVariant { name: variant_name, fields });
                
                if !self.match_token(Token::Comma) {
                    break;
                }
            }
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after enum variants.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        Ok(Stmt { kind: StmtKind::EnumDef { name, variants }, span: Span { line: _line, col: _col } })
    }

    fn parse_binding_pattern(&mut self) -> Result<BindingPattern, ParserError> {
        if self.match_token(Token::LBracket) {
            let mut elements = Vec::new();
            let mut rest = None;
            if self.peek().token != Token::RBracket {
                loop {
                    if self.match_token(Token::DotDot) {
                        if let Token::Ident(r_name) = &self.advance().token {
                            rest = Some(r_name.clone());
                        } else {
                            return Err(ParserError::UnexpectedToken {
                                message: "Expected identifier after '..' in array destructuring.".into(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                        self.match_token(Token::Comma); // optional trailing comma
                        break;
                    }
                    elements.push(self.parse_binding_pattern()?);
                    if !self.match_token(Token::Comma) {
                        break;
                    }
                    if self.peek().token == Token::RBracket {
                        break;
                    }
                }
            }
            if !self.match_token(Token::RBracket) {
                return Err(ParserError::UnexpectedToken {
                    message: "Expected ']' at end of array destructuring pattern.".into(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            Ok(BindingPattern::Array { elements, rest })
        } else if self.match_token(Token::LBrace) {
            let mut fields = Vec::new();
            let mut rest = None;
            if self.peek().token != Token::RBrace {
                loop {
                    if self.match_token(Token::DotDot) {
                        if let Token::Ident(r_name) = &self.advance().token {
                            rest = Some(r_name.clone());
                        } else {
                            return Err(ParserError::UnexpectedToken {
                                message: "Expected identifier after '..' in object destructuring.".into(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                        self.match_token(Token::Comma); // optional trailing comma
                        break;
                    }
                    let field_name = if let Token::Ident(f) = &self.advance().token {
                        f.clone()
                    } else {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected property name in object destructuring.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    };
                    let opt_sub = if self.match_token(Token::Colon) {
                        Some(self.parse_binding_pattern()?)
                    } else {
                        None
                    };
                    fields.push((field_name, opt_sub));
                    if !self.match_token(Token::Comma) {
                        break;
                    }
                    if self.peek().token == Token::RBrace {
                        break;
                    }
                }
            }
            if !self.match_token(Token::RBrace) {
                return Err(ParserError::UnexpectedToken {
                    message: "Expected '}' at end of object destructuring pattern.".into(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            Ok(BindingPattern::Object { fields, rest })
        } else if let Token::Ident(name) = &self.peek().token {
            let n = name.clone();
            self.advance();
            Ok(BindingPattern::Ident(n))
        } else {
            Err(ParserError::UnexpectedToken {
                message: "Expected variable name, '[...]', or '{...}' after 'let'.".into(),
                line: self.peek().line,
                col: self.peek().col,
            })
        }
    }

    fn parse_let_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let pattern = self.parse_binding_pattern()?;

        if !self.match_token(Token::Equals) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '=' in 'let' statement.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let init = self.parse_expression()?;
        self.end_statement()?;

        Ok(Stmt {
            kind: StmtKind::Let { pattern, init },
            span: Span { line, col },
        })
    }

    fn parse_fn_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let name = if let Token::Ident(n) = &self.advance().token {
            n.clone()
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected function name after 'fn'.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };

        let (params, body) = self.parse_function_body()?;
        Ok(Stmt {
            kind: StmtKind::FnDef {
                name,
                params: Arc::new(params),
                body: Arc::new(body),
            },
            span: Span { line, col },
        })
    }

    fn parse_function_body(&mut self) -> Result<(Vec<String>, Vec<Stmt>), ParserError> {
        if !self.match_token(Token::LParen) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '(' after function name.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut params = Vec::new();
        if self.peek().token != Token::RParen {
            loop {
                if let Token::Ident(p) = &self.advance().token {
                    params.push(p.clone());
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected parameter name.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                if !self.match_token(Token::Comma) {
                    break;
                }
            }
        }

        if !self.match_token(Token::RParen) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected ')' after parameters.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' before function body.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut body = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after function body.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        Ok((params, body))
    }

    fn parse_if_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let condition = self.parse_condition()?;

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' after if condition.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut then_branch = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            then_branch.push(self.parse_statement()?);
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after if block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let else_branch = if self.match_token(Token::Else) {
            if self.match_token(Token::If) {
                let cur = self.tokens[self.cursor - 1].clone();
                Some(vec![self.parse_if_statement(cur.line, cur.col)?])
            } else {
                if !self.match_token(Token::LBrace) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected '{' after else.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                let mut e_branch = Vec::new();
                while self.peek().token != Token::RBrace && !self.is_at_end() {
                    e_branch.push(self.parse_statement()?);
                }
                if !self.match_token(Token::RBrace) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected '}' after else block.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                Some(e_branch)
            }
        } else {
            None
        };

        Ok(Stmt {
            kind: StmtKind::If {
                condition,
                then_branch,
                else_branch,
            },
            span: Span { line, col },
        })
    }

    fn parse_condition(&mut self) -> Result<Expr, ParserError> {
        let prev = self.allow_struct;
        self.allow_struct = false;
        let expr = self.parse_expression();
        self.allow_struct = prev;
        let expr = expr?;
        if let Expr::Binary {
            op: BinaryOp::Eq, ..
        } = expr
        {
        } else if let Expr::Variable { .. } = expr {
            // we could check if it's an assignment, but assignment is a statement not an expression in our AST.
        }
        Ok(expr)
    }


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
        
        let has_paren = self.match_token(Token::LParen);
        
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
        
        if has_paren && !self.match_token(Token::RParen) {
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

    fn parse_while_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let condition = self.parse_condition()?;

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' after while condition.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut body = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after while block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        Ok(Stmt {
            kind: StmtKind::While { condition, body },
            span: Span { line, col },
        })
    }

    fn parse_for_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let item = if let Token::Ident(n) = &self.advance().token {
            n.clone()
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected variable name after 'for'.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };

        if !self.match_token(Token::In) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected 'in' after for variable.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let prev = self.allow_struct;
        self.allow_struct = false;
        let iterable = self.parse_expression();
        self.allow_struct = prev;
        let iterable = iterable?;

        if !self.match_token(Token::LBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '{' after for iterable.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let mut body = Vec::new();
        while self.peek().token != Token::RBrace && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }

        if !self.match_token(Token::RBrace) {
            return Err(ParserError::UnexpectedToken {
                message: "Expected '}' after for block.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        Ok(Stmt {
            kind: StmtKind::For {
                item,
                iterable,
                body,
            },
            span: Span { line, col },
        })
    }

    fn parse_return_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let mut value = None;
        if !self.check_end_of_statement() {
            value = Some(self.parse_expression()?);
        }
        self.end_statement()?;
        Ok(Stmt {
            kind: StmtKind::Return(value),
            span: Span { line, col },
        })
    }

    fn parse_expression_statement(&mut self) -> Result<Stmt, ParserError> {
        let cur = self.peek().clone();
        let expr = self.parse_expression()?;

        if self.match_token(Token::Equals) {
            // Assignment
            let value = self.parse_expression()?;
            self.end_statement()?;
            if !matches!(
                expr,
                Expr::Variable { .. } | Expr::Get { .. } | Expr::Index { .. }
            ) {
                return Err(ParserError::UnexpectedToken {
                    message: "Invalid assignment target.".into(),
                    line: cur.line,
                    col: cur.col,
                });
            }
            Ok(Stmt {
                kind: StmtKind::Assign {
                    target: expr,
                    value,
                },
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            })
        } else if self.match_token(Token::PlusEquals) {
            let value = self.parse_expression()?;
            self.end_statement()?;
            if !matches!(
                expr,
                Expr::Variable { .. } | Expr::Get { .. } | Expr::Index { .. }
            ) {
                return Err(ParserError::UnexpectedToken {
                    message: "Invalid assignment target.".into(),
                    line: cur.line,
                    col: cur.col,
                });
            }
            // desugar to a = a + b
            let add_expr = Expr::Binary {
                left: Box::new(expr.clone()),
                op: BinaryOp::Add,
                right: Box::new(value),
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            };
            Ok(Stmt {
                kind: StmtKind::Assign {
                    target: expr,
                    value: add_expr,
                },
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            })
        } else if self.match_token(Token::MinusEquals) {
            let value = self.parse_expression()?;
            self.end_statement()?;
            if !matches!(
                expr,
                Expr::Variable { .. } | Expr::Get { .. } | Expr::Index { .. }
            ) {
                return Err(ParserError::UnexpectedToken {
                    message: "Invalid assignment target.".into(),
                    line: cur.line,
                    col: cur.col,
                });
            }
            let sub_expr = Expr::Binary {
                left: Box::new(expr.clone()),
                op: BinaryOp::Sub,
                right: Box::new(value),
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            };
            Ok(Stmt {
                kind: StmtKind::Assign {
                    target: expr,
                    value: sub_expr,
                },
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            })
        } else {
            self.end_statement()?;
            Ok(Stmt {
                kind: StmtKind::Expr(expr),
                span: Span {
                    line: cur.line,
                    col: cur.col,
                },
            })
        }
    }

    fn check_end_of_statement(&self) -> bool {
        let t = &self.peek().token;
        *t == Token::Semicolon
            || *t == Token::RBrace
            || *t == Token::Eof
            || self.peek().line > self.tokens[self.cursor.saturating_sub(1)].line
    }

    fn end_statement(&mut self) -> Result<(), ParserError> {
        let prev = self.tokens[self.cursor.saturating_sub(1)].clone();
        if self.match_token(Token::Semicolon) {
            return Ok(());
        }
        if self.peek().token == Token::RBrace || self.peek().token == Token::Eof {
            return Ok(());
        }
        if self.peek().line > prev.line {
            return Ok(());
        }
        Err(ParserError::UnexpectedToken {
            message: "Expected newline or ';' at end of statement.".into(),
            line: self.peek().line,
            col: self.peek().col,
        })
    }

    fn parse_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_logical_or()?;
        while self.match_token(Token::Pipe) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_logical_or()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Pipe,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_logical_or(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_logical_and()?;
        while self.match_token(Token::Or) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_logical_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_bitwise_or()?;
        while self.match_token(Token::And) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_bitwise_or()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_bitwise_xor()?;
        while self.match_token(Token::BitOr) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_bitwise_xor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitOr,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_bitwise_and()?;
        while self.match_token(Token::Caret) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_bitwise_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitXor,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_coalesce()?;
        while self.match_token(Token::Amp) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_coalesce()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitAnd,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_coalesce(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_equality()?;
        while self.match_token(Token::DoubleQuestion) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_equality()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Coalesce,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_comparison()?;
        while self.match_token(Token::EqEq) || self.match_token(Token::NotEq) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::EqEq => BinaryOp::Eq,
                Token::NotEq => BinaryOp::NotEq,
                _ => unreachable!(),
            };
            let right = self.parse_comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_shift()?;
        while self.match_token(Token::Lt)
            || self.match_token(Token::LtEq)
            || self.match_token(Token::Gt)
            || self.match_token(Token::GtEq)
        {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Lt => BinaryOp::Lt,
                Token::LtEq => BinaryOp::LtEq,
                Token::Gt => BinaryOp::Gt,
                Token::GtEq => BinaryOp::GtEq,
                _ => unreachable!(),
            };
            let right = self.parse_shift()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_term()?;
        while self.peek().token == Token::Shl || self.peek().token == Token::Shr {
            let prev_line = self.tokens[self.cursor.saturating_sub(1)].line;
            if self.peek().line > prev_line {
                break;
            }
            self.advance();
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Shl => BinaryOp::Shl,
                Token::Shr => BinaryOp::Shr,
                _ => unreachable!(),
            };
            let right = self.parse_term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_factor()?;
        // Enforce same-line continuation for binary ops to fix newline `-2` bug
        while self.peek().token == Token::Plus || self.peek().token == Token::Minus {
            let prev_line = self.tokens[self.cursor.saturating_sub(1)].line;
            if self.peek().line > prev_line {
                break; // Do not parse binary minus/plus on a new line
            }
            self.advance();
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_unary()?;
        while self.peek().token == Token::Star
            || self.peek().token == Token::Slash
            || self.peek().token == Token::Percent
        {
            let prev_line = self.tokens[self.cursor.saturating_sub(1)].line;
            if self.peek().line > prev_line {
                break; // Do not parse binary on a new line
            }
            self.advance();
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParserError> {
        if self.match_token(Token::Not) || self.match_token(Token::Minus) || self.match_token(Token::Tilde) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Not => UnaryOp::Not,
                Token::Minus => UnaryOp::Neg,
                Token::Tilde => UnaryOp::BitNot,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            return Ok(Expr::Unary {
                op,
                expr: Box::new(right),
                span: Span {
                    line: op_tok.line,
                    col: op_tok.col,
                },
            });
        }
        self.parse_call()
    }

    fn parse_call(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_primary()?;

        loop {
            let prev_line = self.tokens[self.cursor.saturating_sub(1)].line;
            if self.peek().line > prev_line
                && matches!(self.peek().token, Token::LParen | Token::LBracket)
            {
                // Postfix (, [ must be on same line to avoid newline bug
                break;
            }

            if self.match_token(Token::LParen) {
                let span = Span {
                    line: self.tokens[self.cursor - 1].line,
                    col: self.tokens[self.cursor - 1].col,
                };
                let mut args = Vec::new();
                if self.peek().token != Token::RParen {
                    loop {
                        args.push(self.parse_expression()?);
                        if !self.match_token(Token::Comma) {
                            break;
                        }
                    }
                }
                if !self.match_token(Token::RParen) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected ')' after arguments.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    span,
                };
            } else if self.match_token(Token::Dot) || self.match_token(Token::Question) {
                let mut safe = false;
                let line = self.tokens[self.cursor - 1].line;
                let col = self.tokens[self.cursor - 1].col;
                if self.tokens[self.cursor - 1].token == Token::Question {
                    if !self.match_token(Token::Dot) {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected '.' after '?' for safe navigation.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                    safe = true;
                }

                if let Token::Ident(name) = &self.advance().token {
                    expr = Expr::Get {
                        target: Box::new(expr),
                        property: name.clone(),
                        safe,
                        span: Span { line, col },
                    };
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected property name after '.'.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            } else if self.match_token(Token::LBracket) {
                let span = Span {
                    line: self.tokens[self.cursor - 1].line,
                    col: self.tokens[self.cursor - 1].col,
                };
                let index = self.parse_expression()?;
                if !self.match_token(Token::RBracket) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected ']' after index.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                expr = Expr::Index {
                    target: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
} else if self.allow_struct && self.peek().token == Token::LBrace {
                if let Expr::Variable { name, .. } = &expr {
                    let span = Span { line: self.tokens[self.cursor.saturating_sub(1)].line, col: self.tokens[self.cursor.saturating_sub(1)].col };
                    self.advance();
                    let mut fields = Vec::new();
                    if self.peek().token != Token::RBrace {
                        loop {
                            let field_name = if let Token::Ident(n) = &self.advance().token {
                                n.clone()
                            } else {
                                return Err(ParserError::UnexpectedToken {
                                    message: "Expected field name in struct initialization.".into(),
                                    line: self.peek().line,
                                    col: self.peek().col,
                                });
                            };
                            
                            if !self.match_token(Token::Colon) {
                                return Err(ParserError::UnexpectedToken {
                                    message: "Expected ':' after field name.".into(),
                                    line: self.peek().line,
                                    col: self.peek().col,
                                });
                            }
                            
                            let val = self.parse_expression()?;
                            fields.push((field_name, val));
                            
                            if !self.match_token(Token::Comma) {
                                break;
                            }
                        }
                    }
                    if !self.match_token(Token::RBrace) {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected '}' after struct fields.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                    expr = Expr::StructInit { name: name.clone(), fields, span };
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParserError> {
        let cur = self.advance().clone();
        let span = Span {
            line: cur.line,
            col: cur.col,
        };
        match cur.token {
            Token::False => Ok(Expr::Literal(Literal::Bool(false))),
            Token::True => Ok(Expr::Literal(Literal::Bool(true))),
            Token::Null => Ok(Expr::Literal(Literal::Null)),
Token::Use => {
                let path = self.parse_expression()?;
                Ok(Expr::Use {
                    path: Box::new(path),
                    span,
                })
            }

            Token::IntLit(n) => Ok(Expr::Literal(Literal::Int(n))),
            Token::FloatLit(n) => Ok(Expr::Literal(Literal::Float(n))),
            Token::NumberLit(n) => Ok(Expr::Literal(Literal::Float(n))),
            Token::StringLit(s) => Ok(Expr::Literal(Literal::String(s))),
            Token::Ident(name) => Ok(Expr::Variable { name, span }),
            Token::LParen => {
                let expr = self.parse_expression()?;
                if !self.match_token(Token::RParen) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected ')' after expression.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                Ok(expr)
            }
            Token::LBracket => {
                let mut elements = Vec::new();
                if self.peek().token != Token::RBracket {
                    loop {
                        elements.push(self.parse_expression()?);
                        if !self.match_token(Token::Comma) {
                            break;
                        }
                    }
                }
                if !self.match_token(Token::RBracket) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected ']' after array elements.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                Ok(Expr::Array(elements))
            }
            Token::LBrace => {
                let mut elements = Vec::new();
                if self.peek().token != Token::RBrace {
                    loop {
                        let key = match &self.advance().token {
                            Token::Ident(n) => n.clone(),
                            Token::StringLit(s) => s.clone(),
                            _ => {
                                return Err(ParserError::UnexpectedToken {
                                    message: "Expected identifier or string as map key.".into(),
                                    line: self.peek().line,
                                    col: self.peek().col,
                                });
                            }
                        };
                        if !self.match_token(Token::Colon) {
                            return Err(ParserError::UnexpectedToken {
                                message: "Expected ':' after map key.".into(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                        let value = self.parse_expression()?;
                        elements.push((key, value));
                        if !self.match_token(Token::Comma) {
                            break;
                        }
                    }
                }
                if !self.match_token(Token::RBrace) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected '}' after map elements.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                Ok(Expr::Map(elements))
            }
Token::Match => {
                let prev = self.allow_struct;
                self.allow_struct = false;
                let target = self.parse_expression()?;
                self.allow_struct = prev;
                
                if !self.match_token(Token::LBrace) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected '{' after match target.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                
                let mut arms = Vec::new();
                while self.peek().token != Token::RBrace && !self.is_at_end() {
                    let pattern = self.parse_pattern()?;
                    let guard = if self.match_token(Token::If) {
                        Some(Box::new(self.parse_expression()?))
                    } else {
                        None
                    };
                    if !self.match_token(Token::FatArrow) {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected '=>' after match pattern.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                    let body = self.parse_expression()?;
                    arms.push(crate::ast::MatchArm { pattern, guard, body: Box::new(body) });
                    
                    self.match_token(Token::Comma); // Optional trailing comma
                }
                
                if !self.match_token(Token::RBrace) {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected '}' after match arms.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                
                Ok(Expr::Match { target: Box::new(target), arms, span })
            }
            Token::Fn => {
                let (params, body) = self.parse_function_body()?;
                Ok(Expr::Lambda {
                    params: Arc::new(params),
                    body: Arc::new(body),
                    span,
                })
            }
            Token::Interp(parts) => {
                let mut expr_parts = Vec::new();
                for p in parts {
                    match p {
                        StrPart::Text(t) => expr_parts.push(InterpPart::Text(t)),
                        StrPart::Code { src, line, col } => {
                            let mut sub_lexer = crate::lexer::Lexer::at(&src, line, col);
                            let sub_tokens = sub_lexer.tokenize()?;
                            let mut sub_parser = Parser::new(sub_tokens);
                            let expr = sub_parser.parse_expression()?;
                            expr_parts.push(InterpPart::Expr(expr));
                        }
                    }
                }
                Ok(Expr::Interpolated(expr_parts))
            }
            _ => Err(ParserError::UnexpectedToken {
                message: format!("Expected expression, got {}.", cur.token),
                line: cur.line,
                col: cur.col,
            }),
        }
    }

    fn try_parse_literal_for_pattern(&mut self) -> Result<Option<crate::ast::Literal>, ParserError> {
        use crate::ast::Literal;
        let is_neg = if self.peek().token == Token::Minus {
            match self.peek_next().map(|t| &t.token) {
                Some(Token::IntLit(_)) | Some(Token::FloatLit(_)) | Some(Token::NumberLit(_)) => {
                    self.advance();
                    true
                }
                _ => false,
            }
        } else {
            false
        };

        let peek = self.peek().clone();
        let lit = match peek.token {
            Token::False if !is_neg => { self.advance(); Some(Literal::Bool(false)) }
            Token::True if !is_neg => { self.advance(); Some(Literal::Bool(true)) }
            Token::Null if !is_neg => { self.advance(); Some(Literal::Null) }
            Token::IntLit(n) => {
                self.advance();
                Some(Literal::Int(if is_neg { -n } else { n }))
            }
            Token::FloatLit(n) | Token::NumberLit(n) => {
                self.advance();
                Some(Literal::Float(if is_neg { -n } else { n }))
            }
            Token::StringLit(s) if !is_neg => {
                self.advance();
                Some(Literal::String(s))
            }
            _ => None,
        };
        Ok(lit)
    }

    fn parse_pattern(&mut self) -> Result<crate::ast::Pattern, ParserError> {
        use crate::ast::Pattern;
        if self.match_token(Token::Underscore) {
            return Ok(Pattern::Wildcard);
        }

        if let Some(lit) = self.try_parse_literal_for_pattern()? {
            if self.match_token(Token::DotDotEq) {
                if let Some(end_lit) = self.try_parse_literal_for_pattern()? {
                    return Ok(Pattern::Range {
                        start: lit,
                        end: end_lit,
                        inclusive: true,
                    });
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected end literal in inclusive range pattern.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            } else if self.match_token(Token::DotDot) {
                if let Some(end_lit) = self.try_parse_literal_for_pattern()? {
                    return Ok(Pattern::Range {
                        start: lit,
                        end: end_lit,
                        inclusive: false,
                    });
                } else {
                    return Err(ParserError::UnexpectedToken {
                        message: "Expected end literal in range pattern.".into(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            }
            return Ok(Pattern::Literal(lit));
        }

        let peek = self.peek().clone();
        match peek.token {
            Token::Ident(name) => {
                self.advance();
                if self.match_token(Token::Dot) {
                    let variant = if let Token::Ident(v) = &self.advance().token {
                        v.clone()
                    } else {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected variant name after '.' in pattern.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    };

                    let mut fields = Vec::new();
                    if self.match_token(Token::LParen) {
                        if self.peek().token != Token::RParen {
                            loop {
                                fields.push(self.parse_pattern()?);
                                if !self.match_token(Token::Comma) { break; }
                            }
                        }
                        if !self.match_token(Token::RParen) {
                            return Err(ParserError::UnexpectedToken {
                                message: "Expected ')' after pattern fields.".into(),
                                line: self.peek().line,
                                col: self.peek().col,
                            });
                        }
                    }
                    Ok(Pattern::Enum {
                        enum_name: Some(name),
                        variant_name: variant,
                        fields,
                    })
                } else if self.match_token(Token::LParen) {
                    let mut fields = Vec::new();
                    if self.peek().token != Token::RParen {
                        loop {
                            fields.push(self.parse_pattern()?);
                            if !self.match_token(Token::Comma) { break; }
                        }
                    }
                    if !self.match_token(Token::RParen) {
                        return Err(ParserError::UnexpectedToken {
                            message: "Expected ')' after pattern fields.".into(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                    Ok(Pattern::Enum {
                        enum_name: None,
                        variant_name: name,
                        fields,
                    })
                } else {
                    Ok(Pattern::Variable(name))
                }
            }
            _ => Err(ParserError::UnexpectedToken {
                message: "Expected pattern.".into(),
                line: peek.line,
                col: peek.col,
            })
        }
    }
}
pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, ParserError> {
    Parser::new(tokens).parse()
}
