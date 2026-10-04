with open("src/ast.rs", "r") as f:
    content = f.read()

content = content.replace("            Expr::Match { span, .. } => Some(*span),", "            Expr::Match { span, .. } => Some(*span),\n            Expr::Use { span, .. } => Some(*span),")
with open("src/ast.rs", "w") as f:
    f.write(content)
