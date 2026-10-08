use crate::token::{SpannedToken, StrPart, Token};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Clone)]
pub enum LexerError {
    #[error("Shae Syntax Error [line {line}:{col}]: Unexpected character '{ch}'.{hint}")]
    UnexpectedChar {
        ch: char,
        hint: String,
        line: usize,
        col: usize,
    },

    #[error(
        "Shae Syntax Error [line {line}:{col}]: Unterminated string literal. Did you forget the closing quote?"
    )]
    UnterminatedString { line: usize, col: usize },

    #[error("Shae Syntax Error [line {line}:{col}]: Unterminated block comment.")]
    UnterminatedComment { line: usize, col: usize },

    #[error("Shae Syntax Error [line {line}:{col}]: Invalid number '{raw}': {msg}")]
    InvalidNumber {
        raw: String,
        msg: String,
        line: usize,
        col: usize,
    },

    #[error(
        "Shae Syntax Error [line {line}:{col}]: Unknown escape sequence '\\\\{ch}'. Supported: \\\\n, \\\\t, \\\\r, \\\\\", \\\\\\\\, \\\\{{, \\\\}}"
    )]
    InvalidEscape { ch: char, line: usize, col: usize },
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

    pub fn at(input: &str, line: usize, col: usize) -> Self {
        Self {
            chars: input.char_indices().collect(),
            cursor: 0,
            line,
            col,
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
                    let start_line = self.line;
                    let start_col = self.col;
                    self.advance(); // consume '/'
                    self.advance(); // consume '*'
                    let mut terminated = false;
                    while let Some(&(_, next_ch)) = self.current() {
                        if next_ch == '*' && self.peek(1) == Some('/') {
                            self.advance(); // consume '*'
                            self.advance(); // consume '/'
                            terminated = true;
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
                    if !terminated {
                        return Err(LexerError::UnterminatedComment {
                            line: start_line,
                            col: start_col,
                        });
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
                '.' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('.') {
                        self.advance();
                        if self.current_char() == Some('=') {
                            self.advance();
                            tokens.push(SpannedToken {
                                token: Token::DotDotEq,
                                line: self.line,
                                col: start_col,
                            });
                        } else {
                            tokens.push(SpannedToken {
                                token: Token::DotDot,
                                line: self.line,
                                col: start_col,
                            });
                        }
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Dot,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }

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

                '|' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('>') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Pipe,
                            line: self.line,
                            col: start_col,
                        });
                    } else if self.current_char() == Some('|') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Or,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::BitOr,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '&' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('&') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::And,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        tokens.push(SpannedToken {
                            token: Token::Amp,
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '^' => tokens.push(self.single(Token::Caret)),
                '~' => tokens.push(self.single(Token::Tilde)),
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
                    } else if self.current_char() == Some('>') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::FatArrow,
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
                        return Err(LexerError::UnexpectedChar {
                            ch: '!',
                            hint: " Did you mean 'not' or '!='?".to_string(),
                            line: self.line,
                            col: start_col,
                        });
                    }
                }
                '<' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('<') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Shl,
                            line: self.line,
                            col: start_col,
                        });
                    } else if self.current_char() == Some('=') {
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
                    if self.current_char() == Some('>') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Shr,
                            line: self.line,
                            col: start_col,
                        });
                    } else if self.current_char() == Some('=') {
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
                    let hint = match ch {
                        '#' => " Did you mean '//' for a comment?",
                        '\'' => " Did you mean to use double quotes for a string?",
                        '&' => " Did you mean '&&'?",
                        '|' => " Did you mean '||'?",
                        _ => "",
                    };
                    return Err(LexerError::UnexpectedChar {
                        ch,
                        hint: hint.to_string(),
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

        let mut parts = Vec::new();
        let mut current_text = String::new();
        let mut has_interp = false;

        while let Some(&(_, ch)) = self.current() {
            match ch {
                '"' => {
                    self.advance(); // consume closing quote
                    if !has_interp {
                        return Ok(SpannedToken {
                            token: Token::StringLit(current_text),
                            line: start_line,
                            col: start_col,
                        });
                    } else {
                        if !current_text.is_empty() {
                            parts.push(StrPart::Text(current_text));
                        }
                        return Ok(SpannedToken {
                            token: Token::Interp(parts),
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
                '\\' => {
                    self.advance();
                    if let Some(&(_, esc)) = self.current() {
                        self.advance();
                        match esc {
                            'n' => current_text.push('\n'),
                            't' => current_text.push('\t'),
                            'r' => current_text.push('\r'),
                            '"' => current_text.push('"'),
                            '\\' => current_text.push('\\'),
                            '{' => current_text.push('{'),
                            '}' => current_text.push('}'),
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
                '{' => {
                    has_interp = true;
                    if !current_text.is_empty() {
                        parts.push(StrPart::Text(current_text.clone()));
                        current_text.clear();
                    }
                    let code_line = self.line;
                    let code_col = self.col;
                    self.advance();

                    let mut brace_count = 1;
                    let mut code_src = String::new();
                    let mut in_string = false;
                    let mut string_esc = false;

                    while let Some(&(_, ich)) = self.current() {
                        if in_string {
                            code_src.push(ich);
                            if string_esc {
                                string_esc = false;
                            } else if ich == '\\' {
                                string_esc = true;
                            } else if ich == '"' {
                                in_string = false;
                            }
                            self.advance();
                        } else {
                            if ich == '"' {
                                in_string = true;
                            } else if ich == '{' {
                                brace_count += 1;
                            } else if ich == '}' {
                                brace_count -= 1;
                                if brace_count == 0 {
                                    self.advance(); // consume }
                                    break;
                                }
                            }
                            if brace_count > 0 {
                                code_src.push(ich);
                                self.advance();
                            }
                        }
                    }
                    if brace_count > 0 {
                        return Err(LexerError::UnexpectedChar {
                            ch: '{',
                            hint: " Unterminated string interpolation block.".to_string(),
                            line: start_line,
                            col: start_col,
                        });
                    }
                    parts.push(StrPart::Code {
                        src: code_src,
                        line: code_line,
                        col: code_col,
                    });
                }
                other => {
                    current_text.push(other);
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

        // Check for 0x (hex), 0b (bin), 0o (oct)
        if self.current_char() == Some('0') {
            if let Some(prefix) = self.peek(1) {
                match prefix {
                    'x' | 'X' => {
                        self.advance(); // consume '0'
                        self.advance(); // consume 'x'/'X'
                        let mut hex_str = String::new();
                        while let Some(&(_, ch)) = self.current() {
                            if ch.is_ascii_hexdigit() {
                                hex_str.push(ch);
                                self.advance();
                            } else if ch == '_' {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        if hex_str.is_empty() {
                            return Err(LexerError::InvalidNumber {
                                raw: format!("0{}", prefix),
                                msg: "Expected hex digits after '0x'".into(),
                                line: start_line,
                                col: start_col,
                            });
                        }
                        let num = i64::from_str_radix(&hex_str, 16).map_err(|e| LexerError::InvalidNumber {
                            raw: format!("0{}{}", prefix, hex_str),
                            msg: e.to_string(),
                            line: start_line,
                            col: start_col,
                        })?;
                        return Ok(SpannedToken {
                            token: Token::IntLit(num),
                            line: start_line,
                            col: start_col,
                        });
                    }
                    'b' | 'B' => {
                        self.advance(); // consume '0'
                        self.advance(); // consume 'b'/'B'
                        let mut bin_str = String::new();
                        while let Some(&(_, ch)) = self.current() {
                            if ch == '0' || ch == '1' {
                                bin_str.push(ch);
                                self.advance();
                            } else if ch == '_' {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        if bin_str.is_empty() {
                            return Err(LexerError::InvalidNumber {
                                raw: format!("0{}", prefix),
                                msg: "Expected binary digits after '0b'".into(),
                                line: start_line,
                                col: start_col,
                            });
                        }
                        let num = i64::from_str_radix(&bin_str, 2).map_err(|e| LexerError::InvalidNumber {
                            raw: format!("0{}{}", prefix, bin_str),
                            msg: e.to_string(),
                            line: start_line,
                            col: start_col,
                        })?;
                        return Ok(SpannedToken {
                            token: Token::IntLit(num),
                            line: start_line,
                            col: start_col,
                        });
                    }
                    'o' | 'O' => {
                        self.advance(); // consume '0'
                        self.advance(); // consume 'o'/'O'
                        let mut oct_str = String::new();
                        while let Some(&(_, ch)) = self.current() {
                            if ('0'..='7').contains(&ch) {
                                oct_str.push(ch);
                                self.advance();
                            } else if ch == '_' {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        if oct_str.is_empty() {
                            return Err(LexerError::InvalidNumber {
                                raw: format!("0{}", prefix),
                                msg: "Expected octal digits after '0o'".into(),
                                line: start_line,
                                col: start_col,
                            });
                        }
                        let num = i64::from_str_radix(&oct_str, 8).map_err(|e| LexerError::InvalidNumber {
                            raw: format!("0{}{}", prefix, oct_str),
                            msg: e.to_string(),
                            line: start_line,
                            col: start_col,
                        })?;
                        return Ok(SpannedToken {
                            token: Token::IntLit(num),
                            line: start_line,
                            col: start_col,
                        });
                    }
                    _ => {}
                }
            }
        }

        let mut s = String::new();
        let mut seen_dot = false;
        let mut seen_exp = false;

        while let Some(&(_, ch)) = self.current() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '_' {
                self.advance();
            } else if ch == '.' && !seen_dot && !seen_exp {
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
            } else if (ch == 'e' || ch == 'E') && !seen_exp {
                if let Some(next_ch) = self.peek(1) {
                    if next_ch.is_ascii_digit() || ((next_ch == '+' || next_ch == '-') && self.peek(2).map(|c| c.is_ascii_digit()).unwrap_or(false)) {
                        seen_exp = true;
                        s.push(ch);
                        self.advance();
                        if self.current_char() == Some('+') || self.current_char() == Some('-') {
                            s.push(self.current_char().unwrap());
                            self.advance();
                        }
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

        if seen_dot || seen_exp {
            let num = s.parse::<f64>().map_err(|e| LexerError::InvalidNumber {
                raw: s.clone(),
                msg: e.to_string(),
                line: start_line,
                col: start_col,
            })?;
            Ok(SpannedToken {
                token: Token::FloatLit(num),
                line: start_line,
                col: start_col,
            })
        } else {
            if let Ok(num) = s.parse::<i64>() {
                Ok(SpannedToken {
                    token: Token::IntLit(num),
                    line: start_line,
                    col: start_col,
                })
            } else if let Ok(num) = s.parse::<f64>() {
                Ok(SpannedToken {
                    token: Token::FloatLit(num),
                    line: start_line,
                    col: start_col,
                })
            } else {
                Err(LexerError::InvalidNumber {
                    raw: s.clone(),
                    msg: "Invalid integer literal".to_string(),
                    line: start_line,
                    col: start_col,
                })
            }
        }
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

        let token = match s.as_str() {
            "struct" => Token::Struct,
            "enum" => Token::Enum,
            "match" => Token::Match,
            "_" => Token::Underscore,
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
            "try" => Token::Try,
            "catch" => Token::Catch,
            "use" => Token::Use,
            "from" => Token::From,
            "as" => Token::As,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
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
