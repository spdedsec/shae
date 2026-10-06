with open("src/compiler.rs", "r") as f:
    content = f.read()

content = content.replace("BinaryOp::Subtract", "BinaryOp::Sub")
content = content.replace("BinaryOp::Multiply", "BinaryOp::Mul")
content = content.replace("BinaryOp::Divide", "BinaryOp::Div")
content = content.replace("UnaryOp::Negate", "UnaryOp::Neg")
content = content.replace("Expr::Unary { op, right, span }", "Expr::Unary { op, expr: right, span }")

with open("src/compiler.rs", "w") as f:
    f.write(content)
