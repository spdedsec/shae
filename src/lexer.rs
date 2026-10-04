use crate::token::{SpannedToken, Token};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum LexerError {
    #[error("Shae Syntax Error [line {line}:{col}]: Unexpected character '{ch}'. Did a rogue typo slip in?")]
    UnexpectedChar { ch: char, line: usize, col: usize },

    #[error("Shae Syntax Error [line {line}:{col}]: Unterminated string literal. Did you forget the closing '\"'?")]
    UnterminatedString { line: usize, col: usize },

    #[error("Shae Syntax Error [line {line}:{col}]: Invalid number '{raw}': {msg}")]
    InvalidNumber { raw: String, msg: String, line: usize, col: usize },

    #[error("Shae Syntax Error [line {line}:{col}]: Unknown escape sequence '\\{ch}'. Supported: \\n, \\t, \\r, \\\", \\\\")]
    InvalidEscape { ch: char, line: usize, col: usize },

    #[error("Shae Friendly Advice [line {line}:{col}]: {advice}")]
    FriendlyAdvice { advice: String, line: usize, col: usize },
}

pub struct Lexer {
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            chars: input.char_indices().collect(),
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>, LexerError> {
        let mut tokens = Vec::new();

        while let Some(&(_, ch)) = self.current() {
            match ch {
                // Whitespace
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                '\n' => {
                    self.cursor += 1;
                    self.line += 1;
                    self.col = 1;
                }

                // Single-line and Multi-line Comments
                '/' if self.peek(1) == Some('/') => {
                    self.advance(); // consume '/'
                    self.advance(); // consume '/'
                    while let Some(&(_, next_ch)) = self.current() {
                        if next_ch == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                '/' if self.peek(1) == Some('*') => {
                    self.advance(); // consume '/'
                    self.advance(); // consume '*'
                    while let Some(&(_, next_ch)) = self.current() {
                        if next_ch == '*' && self.peek(1) == Some('/') {
                            self.advance(); // consume '*'
                            self.advance(); // consume '/'
                            break;
                        }
                        if next_ch == '\n' {
                            self.cursor += 1;
                            self.line += 1;
                            self.col = 1;
                        } else {
                            self.advance();
                        }
                    }
                }

                // Parentheses, Braces, Brackets
                '(' => tokens.push(self.single(Token::LParen)),
                ')' => tokens.push(self.single(Token::RParen)),
                '{' => tokens.push(self.single(Token::LBrace)),
                '}' => tokens.push(self.single(Token::RBrace)),
                '[' => tokens.push(self.single(Token::LBracket)),
                ']' => tokens.push(self.single(Token::RBracket)),

                // Delimiters
                ',' => tokens.push(self.single(Token::Comma)),
                ':' => tokens.push(self.single(Token::Colon)),
                ';' => tokens.push(self.single(Token::Semicolon)),
                '.' => tokens.push(self.single(Token::Dot)),

                // Two-character operators with single-char fallbacks
                '+' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::PlusEquals,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Plus,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '-' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('>') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Arrow,
                            line: self.line,
                            col: start_col,
                        });
                    } else if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::MinusEquals,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Minus,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '*' => tokens.push(self.single(Token::Star)),
                '/' => tokens.push(self.single(Token::Slash)),
                '%' => tokens.push(self.single(Token::Percent)),

                '=' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::EqEq,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Equals,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '!' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::NotEq,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Bang,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '<' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::LtEq,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Lt,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '>' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::GtEq,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Gt,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '&' if self.peek(1) == Some('&') => {
                    let start_col = self.col;
                    self.advance();
                    self.advance();
                    tokens.push(SpannedToken {
                        token: Token::And,
                        line: self.line,
                        col: start_col,
                    });
                }
                '|' if self.peek(1) == Some('|') => {
                    let start_col = self.col;
                    self.advance();
                    self.advance();
                    tokens.push(SpannedToken {
                        token: Token::Or,
                        line: self.line,
                        col: start_col,
                    });
                }
                '?' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('?') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::DoubleQuestion,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Question,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }

                // Strings
                '"' => {
                    let token = self.scan_string()?;
                    tokens.push(token);
                }

                // Numbers
                '0'..='9' => {
                    let token = self.scan_number()?;
                    tokens.push(token);
                }

                // Identifiers or Keywords
                'a'..='z' | 'A'..='Z' | '_' => {
                    let token = self.scan_identifier()?;
                    tokens.push(token);
                }

                _ => {
                    return Err(LexerError::UnexpectedChar {
                        ch,
                        line: self.line,
                        col: self.col,
                    });
                }
            }
        }

        tokens.push(SpannedToken {
            token: Token::Eof,
            line: self.line,
            col: self.col,
        });

        Ok(tokens)
    }

    fn single(&mut self, token: Token) -> SpannedToken {
        let start_col = self.col;
        self.advance();
        SpannedToken {
            token,
            line: self.line,
            col: start_col,
        }
    }

    fn current(&self) -> Option<&(usize, char)> {
        self.chars.get(self.cursor)
    }

    fn current_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|&(_, ch)| ch)
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.cursor + offset).map(|&(_, ch)| ch)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(&(_, ch)) = self.current() {
            self.cursor += 1;
            self.col += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn scan_string(&mut self) -> Result<SpannedToken, LexerError> {
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // consume opening quote

        let mut val = String::new();

        while let Some(&(_, ch)) = self.current() {
            match ch {
                '"' => {
                    self.advance(); // consume closing quote
                    return Ok(SpannedToken {
                        token: Token::StringLit(val),
                        line: start_line,
                        col: start_col,
                    });
                }
                '\\' => {
                    self.advance();
                    if let Some(&(_, esc)) = self.current() {
                        self.advance();
                        match esc {
                            'n' => val.push('\n'),
                            't' => val.push('\t'),
                            'r' => val.push('\r'),
                            '"' => val.push('"'),
                            '\\' => val.push('\\'),
                            other => {
                                return Err(LexerError::InvalidEscape {
                                    ch: other,
                                    line: self.line,
                                    col: self.col,
                                });
                            }
                        }
                    } else {
                        return Err(LexerError::UnterminatedString {
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
                '\n' => {
                    return Err(LexerError::UnterminatedString {
                        line: start_line,
                        col: start_col,
                    });
                }
                other => {
                    val.push(other);
                    self.advance();
                }
            }
        }

        Err(LexerError::UnterminatedString {
            line: start_line,
            col: start_col,
        })
    }

    fn scan_number(&mut self) -> Result<SpannedToken, LexerError> {
        let start_line = self.line;
        let start_col = self.col;
        let mut s = String::new();
        let mut seen_dot = false;

        while let Some(&(_, ch)) = self.current() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !seen_dot {
                if let Some(next_ch) = self.peek(1) {
                    if next_ch.is_ascii_digit() {
                        seen_dot = true;
                        s.push('.');
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let num = s.parse::<f64>().map_err(|e| LexerError::InvalidNumber {
            raw: s.clone(),
            msg: e.to_string(),
            line: start_line,
            col: start_col,
        })?;

        Ok(SpannedToken {
            token: Token::NumberLit(num),
            line: start_line,
            col: start_col,
        })
    }

    fn scan_identifier(&mut self) -> Result<SpannedToken, LexerError> {
        let start_line = self.line;
        let start_col = self.col;
        let mut s = String::new();

        while let Some(&(_, ch)) = self.current() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Friendly advice for common cross-language habits
        match s.as_str() {
            "function" | "def" => {
                return Err(LexerError::FriendlyAdvice {
                    advice: format!("In Shae, we use 'fn' for functions (e.g., 'fn add(a, b) {{ ... }}'). Keep it short and sweet!"),
                    line: start_line,
                    col: start_col,
                });
            }
            "var" => {
                return Err(LexerError::FriendlyAdvice {
                    advice: "In Shae, use 'let' to declare variables (e.g., 'let count = 0').".to_string(),
                    line: start_line,
                    col: start_col,
                });
            }
            "nil" | "None" | "undefined" => {
                return Err(LexerError::FriendlyAdvice {
                    advice: "In Shae, null values are simply 'null'.".to_string(),
                    line: start_line,
                    col: start_col,
                });
            }
            _ => {}
        }

        let token = match s.as_str() {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "true" => Token::True,
            "false" => Token::False,
            "null" => Token::Null,
            "break" => Token::Break,
            "continue" => Token::Continue,
            _ => Token::Ident(s),
        };

        Ok(SpannedToken {
            token,
            line: start_line,
            col: start_col,
        })
    }
}

pub fn tokenize(input: &str) -> Result<Vec<SpannedToken>, LexerError> {
    Lexer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_tokens() {
        let code = r#"
            let name = "Satya"
            fn greet(person) {
                return "Hello, " + person
            }
            if name == "Satya" {
                let status = true ?? false
            }
        "#;
        let tokens = tokenize(code).expect("Tokenization should succeed");
        let kinds: Vec<Token> = tokens.into_iter().map(|st| st.token).collect();

        assert!(kinds.contains(&Token::Let));
        assert!(kinds.contains(&Token::Fn));
        assert!(kinds.contains(&Token::Return));
        assert!(kinds.contains(&Token::If));
        assert!(kinds.contains(&Token::DoubleQuestion));
    }

    #[test]
    fn test_friendly_advice_def() {
        let code = "def my_func() {}";
        let err = tokenize(code).unwrap_err();
        match err {
            LexerError::FriendlyAdvice { advice, .. } => {
                assert!(advice.contains("use 'fn' for functions"));
            }
            _ => panic!("Expected friendly advice error"),
        }
    }
}
