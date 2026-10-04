with open("src/parser.rs", "r") as f:
    content = f.read()

use_expr = """
            Token::Use => {
                let path = self.parse_expression()?;
                Ok(Expr::Use {
                    path: Box::new(path),
                    span,
                })
            }
"""
content = content.replace("            Token::Null => Ok(Expr::Literal(Literal::Null)),", "            Token::Null => Ok(Expr::Literal(Literal::Null)),\n" + use_expr.strip() + "\n")
with open("src/parser.rs", "w") as f:
    f.write(content)
