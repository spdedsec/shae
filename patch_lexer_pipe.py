with open("src/lexer.rs", "r") as f:
    content = f.read()

target = """                '=' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {"""

replacement = """                '|' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('>') {
                        self.advance();
                        tokens.push(SpannedToken {
                            token: Token::Pipe,
                            line: self.line,
                            col: start_col,
                        });
                    } else {
                        return Err(LexerError::UnexpectedChar {
                            ch: '|',
                            line: self.line,
                            col: start_col,
                            hint: "Did you mean `|>`?".into(),
                        });
                    }
                }
                '=' => {
                    let start_col = self.col;
                    self.advance();
                    if self.current_char() == Some('=') {"""

content = content.replace(target, replacement)
with open("src/lexer.rs", "w") as f:
    f.write(content)
