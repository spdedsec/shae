with open("src/parser.rs", "r") as f:
    content = f.read()

target = """    fn parse_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Result<Expr, ParserError> {"""

replacement = """    fn parse_expression(&mut self) -> Result<Expr, ParserError> {
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

    fn parse_logical_or(&mut self) -> Result<Expr, ParserError> {"""

content = content.replace(target, replacement)
with open("src/parser.rs", "w") as f:
    f.write(content)
