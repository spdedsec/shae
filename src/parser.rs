use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, UnaryOp};
use crate::token::{SpannedToken, Token};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ParserError {
    #[error("Shae Syntax Error [line {line}:{col}]: Unexpected {found}. {hint}")]
    UnexpectedToken {
        found: String,
        hint: String,
        line: usize,
        col: usize,
    },

    #[error("Shae Syntax Error: Unexpected end of file while parsing {context}. Did you forget a closing '}}' or ')'?")]
    UnexpectedEof { context: String },
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
            // Skip any lone semicolons
            if self.match_token(&Token::Semicolon) {
                continue;
            }
            statements.push(self.parse_statement()?);
        }

        Ok(Program { statements })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParserError> {
        let current = self.peek();
        match &current.token {
            Token::Let => self.parse_let_statement(),
            Token::Fn if self.is_fn_declaration() => self.parse_fn_declaration(),
            Token::If => self.parse_if_statement(),
            Token::While => self.parse_while_statement(),
            Token::For => self.parse_for_statement(),
            Token::Return => self.parse_return_statement(),
            Token::Break => {
                self.advance();
                self.match_token(&Token::Semicolon);
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.match_token(&Token::Semicolon);
                Ok(Stmt::Continue)
            }
            _ => self.parse_expr_or_assign_statement(),
        }
    }

    fn is_fn_declaration(&self) -> bool {
        // fn ident(...) -> declaration
        // fn(...) -> anonymous lambda expression
        if let Some(t) = self.peek_offset(1) {
            matches!(t.token, Token::Ident(_))
        } else {
            false
        }
    }

    fn parse_let_statement(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::Let, "Expected 'let'")?;

        let name = match self.advance().token {
            Token::Ident(name) => name,
            other => {
                let prev = self.previous();
                return Err(ParserError::UnexpectedToken {
                    found: other.to_string(),
                    hint: "Expected variable name after 'let' (e.g., 'let count = 0').".to_string(),
                    line: prev.line,
                    col: prev.col,
                });
            }
        };

        self.consume(&Token::Equals, "Expected '=' in variable declaration.")?;
        let init = self.parse_expression(0)?;
        self.match_token(&Token::Semicolon);

        Ok(Stmt::Let { name, init })
    }

    fn parse_fn_declaration(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::Fn, "Expected 'fn'")?;

        let name = match self.advance().token {
            Token::Ident(name) => name,
            other => {
                let prev = self.previous();
                return Err(ParserError::UnexpectedToken {
                    found: other.to_string(),
                    hint: "Expected function name after 'fn'.".to_string(),
                    line: prev.line,
                    col: prev.col,
                });
            }
        };

        self.consume(&Token::LParen, "Expected '(' after function name.")?;
        let mut params = Vec::new();
        while !self.check(&Token::RParen) && !self.is_at_end() {
            match self.advance().token {
                Token::Ident(param) => params.push(param),
                other => {
                    let prev = self.previous();
                    return Err(ParserError::UnexpectedToken {
                        found: other.to_string(),
                        hint: "Expected parameter name in function definition.".to_string(),
                        line: prev.line,
                        col: prev.col,
                    });
                }
            }
            if self.match_token(&Token::Comma) {
                continue;
            } else if !self.check(&Token::RParen) {
                break;
            }
        }
        self.consume(&Token::RParen, "Expected ')' to close parameter list.")?;

        let body = self.parse_block()?;

        Ok(Stmt::FnDef { name, params, body })
    }

    fn parse_if_statement(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::If, "Expected 'if'")?;
        let condition = self.parse_expression(0)?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.match_token(&Token::Else) {
            if self.check(&Token::If) {
                // else if ...
                Some(vec![self.parse_if_statement()?])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_while_statement(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::While, "Expected 'while'")?;
        let condition = self.parse_expression(0)?;
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_for_statement(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::For, "Expected 'for'")?;

        let item = match self.advance().token {
            Token::Ident(name) => name,
            other => {
                let prev = self.previous();
                return Err(ParserError::UnexpectedToken {
                    found: other.to_string(),
                    hint: "Expected loop variable name after 'for' (e.g. 'for item in items { ... }').".to_string(),
                    line: prev.line,
                    col: prev.col,
                });
            }
        };

        self.consume(&Token::In, "Expected 'in' after loop variable in for loop.")?;
        let iterable = self.parse_expression(0)?;
        let body = self.parse_block()?;

        Ok(Stmt::For {
            item,
            iterable,
            body,
        })
    }

    fn parse_return_statement(&mut self) -> Result<Stmt, ParserError> {
        self.consume(&Token::Return, "Expected 'return'")?;

        let expr = if self.check(&Token::RBrace) || self.check(&Token::Semicolon) || self.is_at_end() {
            None
        } else {
            Some(self.parse_expression(0)?)
        };

        self.match_token(&Token::Semicolon);
        Ok(Stmt::Return(expr))
    }

    fn parse_expr_or_assign_statement(&mut self) -> Result<Stmt, ParserError> {
        let expr = self.parse_expression(0)?;

        // Check for assignments: = , += , -=
        if self.match_token(&Token::Equals) {
            let value = self.parse_expression(0)?;
            self.match_token(&Token::Semicolon);
            return Ok(Stmt::Assign {
                target: expr,
                value,
            });
        } else if self.match_token(&Token::PlusEquals) {
            let right = self.parse_expression(0)?;
            self.match_token(&Token::Semicolon);
            return Ok(Stmt::Assign {
                target: expr.clone(),
                value: Expr::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::Add,
                    right: Box::new(right),
                },
            });
        } else if self.match_token(&Token::MinusEquals) {
            let right = self.parse_expression(0)?;
            self.match_token(&Token::Semicolon);
            return Ok(Stmt::Assign {
                target: expr.clone(),
                value: Expr::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::Sub,
                    right: Box::new(right),
                },
            });
        }

        self.match_token(&Token::Semicolon);
        Ok(Stmt::Expr(expr))
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParserError> {
        self.consume(&Token::LBrace, "Expected '{' to start a block.")?;
        let mut stmts = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            if self.match_token(&Token::Semicolon) {
                continue;
            }
            stmts.push(self.parse_statement()?);
        }

        self.consume(&Token::RBrace, "Expected '}' to close block.")?;
        Ok(stmts)
    }

    pub fn parse_expression(&mut self, min_bp: u8) -> Result<Expr, ParserError> {
        let mut left = self.parse_primary()?;

        loop {
            // Postfix operators: Calls (), Indexing [], Property access . and ?.
            if self.check(&Token::LParen) {
                if self.peek().line == self.previous().line {
                    self.advance();
                    let mut args = Vec::new();
                    while !self.check(&Token::RParen) && !self.is_at_end() {
                        args.push(self.parse_expression(0)?);
                        if self.match_token(&Token::Comma) {
                            continue;
                        } else if !self.check(&Token::RParen) {
                            break;
                        }
                    }
                    self.consume(&Token::RParen, "Expected ')' to close argument list.")?;
                    left = Expr::Call {
                        callee: Box::new(left),
                        args,
                    };
                    continue;
                } else {
                    break;
                }
            }

            if self.check(&Token::LBracket) {
                if self.peek().line == self.previous().line {
                    self.advance();
                    let index = self.parse_expression(0)?;
                    self.consume(&Token::RBracket, "Expected ']' after index expression.")?;
                    left = Expr::Index {
                        target: Box::new(left),
                        index: Box::new(index),
                    };
                    continue;
                } else {
                    break;
                }
            }

            if self.match_token(&Token::Dot) {
                let property = match self.advance().token {
                    Token::Ident(prop) => prop,
                    other => {
                        let prev = self.previous();
                        return Err(ParserError::UnexpectedToken {
                            found: other.to_string(),
                            hint: "Expected property name after '.'".to_string(),
                            line: prev.line,
                            col: prev.col,
                        });
                    }
                };
                left = Expr::Get {
                    target: Box::new(left),
                    property,
                    safe: false,
                };
                continue;
            }

            // Safe navigation: ?.
            if self.check(&Token::Question) && self.peek_offset(1).map_or(false, |t| t.token == Token::Dot) {
                self.advance(); // consume '?'
                self.advance(); // consume '.'
                let property = match self.advance().token {
                    Token::Ident(prop) => prop,
                    other => {
                        let prev = self.previous();
                        return Err(ParserError::UnexpectedToken {
                            found: other.to_string(),
                            hint: "Expected property name after '?.'".to_string(),
                            line: prev.line,
                            col: prev.col,
                        });
                    }
                };
                left = Expr::Get {
                    target: Box::new(left),
                    property,
                    safe: true,
                };
                continue;
            }

            // Infix Binary Operators
            let current = self.peek();
            if let Some((l_bp, r_bp, op)) = self.infix_binding_power(&current.token) {
                if l_bp < min_bp {
                    break;
                }

                self.advance(); // consume operator
                let right = self.parse_expression(r_bp)?;
                left = Expr::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn infix_binding_power(&self, token: &Token) -> Option<(u8, u8, BinaryOp)> {
        match token {
            Token::Or => Some((1, 2, BinaryOp::Or)),
            Token::And => Some((3, 4, BinaryOp::And)),
            Token::DoubleQuestion => Some((5, 4, BinaryOp::Coalesce)),
            Token::EqEq => Some((6, 7, BinaryOp::Eq)),
            Token::NotEq => Some((6, 7, BinaryOp::NotEq)),
            Token::Lt => Some((8, 9, BinaryOp::Lt)),
            Token::LtEq => Some((8, 9, BinaryOp::LtEq)),
            Token::Gt => Some((8, 9, BinaryOp::Gt)),
            Token::GtEq => Some((8, 9, BinaryOp::GtEq)),
            Token::Plus => Some((10, 11, BinaryOp::Add)),
            Token::Minus => Some((10, 11, BinaryOp::Sub)),
            Token::Star => Some((12, 13, BinaryOp::Mul)),
            Token::Slash => Some((12, 13, BinaryOp::Div)),
            Token::Percent => Some((12, 13, BinaryOp::Mod)),
            _ => None,
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParserError> {
        let current = self.peek();
        match &current.token {
            Token::NumberLit(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Literal(Literal::Number(n)))
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::Literal(Literal::String(s)))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Literal(Literal::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Literal(Literal::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Literal::Null))
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Variable(name))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expression(0)?;
                self.consume(&Token::RParen, "Expected ')' to close grouped expression.")?;
                Ok(expr)
            }
            Token::Bang => {
                self.advance();
                let expr = self.parse_expression(14)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            Token::Minus => {
                self.advance();
                let expr = self.parse_expression(14)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Token::LBracket => self.parse_array_literal(),
            Token::LBrace => self.parse_map_literal(),
            Token::Fn => self.parse_anonymous_lambda(),
            other => Err(ParserError::UnexpectedToken {
                found: other.to_string(),
                hint: "Expected an expression (number, string, variable, '(', '[', or '{').".to_string(),
                line: current.line,
                col: current.col,
            }),
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expr, ParserError> {
        self.consume(&Token::LBracket, "Expected '[' to start array.")?;
        let mut items = Vec::new();

        while !self.check(&Token::RBracket) && !self.is_at_end() {
            items.push(self.parse_expression(0)?);
            if self.match_token(&Token::Comma) {
                continue;
            } else if !self.check(&Token::RBracket) {
                break;
            }
        }

        self.consume(&Token::RBracket, "Expected ']' to close array.")?;
        Ok(Expr::Array(items))
    }

    fn parse_map_literal(&mut self) -> Result<Expr, ParserError> {
        self.consume(&Token::LBrace, "Expected '{' to start map/object.")?;
        let mut entries = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let key = match self.advance().token {
                Token::Ident(k) => k,
                Token::StringLit(k) => k,
                other => {
                    let prev = self.previous();
                    return Err(ParserError::UnexpectedToken {
                        found: other.to_string(),
                        hint: "Expected map key (identifier or string).".to_string(),
                        line: prev.line,
                        col: prev.col,
                    });
                }
            };

            self.consume(&Token::Colon, "Expected ':' after map key.")?;
            let val = self.parse_expression(0)?;
            entries.push((key, val));

            if self.match_token(&Token::Comma) {
                continue;
            } else if !self.check(&Token::RBrace) {
                break;
            }
        }

        self.consume(&Token::RBrace, "Expected '}' to close map/object.")?;
        Ok(Expr::Map(entries))
    }

    fn parse_anonymous_lambda(&mut self) -> Result<Expr, ParserError> {
        self.consume(&Token::Fn, "Expected 'fn'")?;
        self.consume(&Token::LParen, "Expected '(' after 'fn' for lambda parameters.")?;
        let mut params = Vec::new();
        while !self.check(&Token::RParen) && !self.is_at_end() {
            match self.advance().token {
                Token::Ident(param) => params.push(param),
                other => {
                    let prev = self.previous();
                    return Err(ParserError::UnexpectedToken {
                        found: other.to_string(),
                        hint: "Expected parameter name in lambda parameter list.".to_string(),
                        line: prev.line,
                        col: prev.col,
                    });
                }
            }
            if self.match_token(&Token::Comma) {
                continue;
            } else if !self.check(&Token::RParen) {
                break;
            }
        }
        self.consume(&Token::RParen, "Expected ')' to close lambda parameter list.")?;
        let body = self.parse_block()?;

        Ok(Expr::Lambda { params, body })
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, token: &Token) -> bool {
        if self.is_at_end() {
            token == &Token::Eof
        } else {
            &self.peek().token == token
        }
    }

    fn advance(&mut self) -> SpannedToken {
        if !self.is_at_end() {
            self.cursor += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        self.peek().token == Token::Eof
    }

    fn peek(&self) -> SpannedToken {
        self.tokens
            .get(self.cursor)
            .cloned()
            .unwrap_or(SpannedToken {
                token: Token::Eof,
                line: 0,
                col: 0,
            })
    }

    fn peek_offset(&self, offset: usize) -> Option<&SpannedToken> {
        self.tokens.get(self.cursor + offset)
    }

    fn previous(&self) -> SpannedToken {
        if self.cursor == 0 {
            self.peek()
        } else {
            self.tokens[self.cursor - 1].clone()
        }
    }

    fn consume(&mut self, expected: &Token, hint: &str) -> Result<SpannedToken, ParserError> {
        if self.check(expected) {
            Ok(self.advance())
        } else {
            let current = self.peek();
            Err(ParserError::UnexpectedToken {
                found: current.token.to_string(),
                hint: hint.to_string(),
                line: current.line,
                col: current.col,
            })
        }
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, ParserError> {
    Parser::new(tokens).parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn test_parse_let_and_fn() {
        let code = r#"
            let message = "Welcome to Shae!"
            fn greet(name) {
                return "Hello, " + name
            }
            let res = greet("Satya")
        "#;
        let tokens = tokenize(code).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.statements.len(), 3);
    }

    #[test]
    fn test_parse_if_while_for() {
        let code = r#"
            if x > 10 {
                print("big")
            } else {
                print("small")
            }
            while x > 0 {
                x -= 1
            }
            for item in list {
                print(item)
            }
        "#;
        let tokens = tokenize(code).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.statements.len(), 3);
    }
}
