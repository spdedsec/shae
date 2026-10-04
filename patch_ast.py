with open("src/ast.rs", "r") as f:
    content = f.read()

content = content.replace("    Interpolated(Vec<InterpPart>),", "    Interpolated(Vec<InterpPart>),\n    Use {\n        path: Box<Expr>,\n        span: Span,\n    },")

content = content.replace("            Expr::Interpolated(_) => todo!(),", "            Expr::Interpolated(_) => todo!(),\n            Expr::Use { span, .. } => *span,")
with open("src/ast.rs", "w") as f:
    f.write(content)
