with open("src/ast.rs", "r") as f:
    content = f.read()

content = content.replace("    And,\n    Or,", "    And,\n    Or,\n    Pipe,")

with open("src/ast.rs", "w") as f:
    f.write(content)

with open("src/parser.rs", "r") as f:
    content = f.read()

content = content.replace('            Token::Or => Some((BinaryOp::Or, 2)),', '            Token::Or => Some((BinaryOp::Or, 2)),\n            Token::Pipe => Some((BinaryOp::Pipe, 1)),')

with open("src/parser.rs", "w") as f:
    f.write(content)
