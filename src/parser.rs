use crate::ast::{BinaryOp, Expr, InterpPart, Literal, Program, Span, Stmt, StmtKind, UnaryOp};
use crate::lexer::LexerError;
use crate::token::{SpannedToken, StrPart, Token};
use std::rc::Rc;
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
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, cursor: 0 }
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
        } else if self.match_token(Token::Fn) {
            self.parse_fn_statement(cur.line, cur.col)?
        } else if self.match_token(Token::If) {
            self.parse_if_statement(cur.line, cur.col)?
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
        } else {
            self.parse_expression_statement()?
        };
        Ok(stmt)
    }

    fn parse_let_statement(&mut self, line: usize, col: usize) -> Result<Stmt, ParserError> {
        let name = if let Token::Ident(n) = &self.advance().token {
            n.clone()
        } else {
            return Err(ParserError::UnexpectedToken {
                message: "Expected variable name after 'let'.".into(),
                line: self.peek().line,
                col: self.peek().col,
            });
        };

        if !self.match_token(Token::Equals) {
            return Err(ParserError::UnexpectedToken {
                message: format!("Expected '=' after variable '{}'.", name),
                line: self.peek().line,
                col: self.peek().col,
            });
        }

        let init = self.parse_expression()?;
        self.end_statement()?;

        Ok(Stmt {
            kind: StmtKind::Let { name, init },
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
                params: Rc::new(params),
                body: Rc::new(body),
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
        let expr = self.parse_expression()?;
        if let Expr::Binary {
            op: BinaryOp::Eq, ..
        } = expr
        {
        } else if let Expr::Variable { .. } = expr {
            // we could check if it's an assignment, but assignment is a statement not an expression in our AST.
        }
        Ok(expr)
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

        let iterable = self.parse_expression()?;

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
        self.parse_logical_or()
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
        let mut expr = self.parse_coalesce()?;
        while self.match_token(Token::And) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let right = self.parse_coalesce()?;
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
        let mut expr = self.parse_term()?;
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
        while (self.peek().token == Token::Plus || self.peek().token == Token::Minus) {
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
        while (self.peek().token == Token::Star
            || self.peek().token == Token::Slash
            || self.peek().token == Token::Percent)
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
        if self.match_token(Token::Bang) || self.match_token(Token::Minus) {
            let op_tok = self.tokens[self.cursor - 1].clone();
            let op = match op_tok.token {
                Token::Bang => UnaryOp::Not,
                Token::Minus => UnaryOp::Neg,
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
                let mut line = self.tokens[self.cursor - 1].line;
                let mut col = self.tokens[self.cursor - 1].col;
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
            Token::NumberLit(n) => Ok(Expr::Literal(Literal::Number(n))),
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
            Token::Fn => {
                let (params, body) = self.parse_function_body()?;
                Ok(Expr::Lambda {
                    params: Rc::new(params),
                    body: Rc::new(body),
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
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, ParserError> {
    Parser::new(tokens).parse()
}
