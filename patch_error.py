with open("src/eval.rs", "r") as f:
    content = f.read()

content = content.replace(
    "pub struct RuntimeError {\n    pub message: String,\n    pub hint: Option<String>,\n    pub span: Option<Span>,\n}",
    "pub struct RuntimeError {\n    pub message: String,\n    pub hint: Option<String>,\n    pub span: Option<Span>,\n    pub stack: Vec<(String, crate::ast::Span)>,\n}"
)

content = content.replace(
    "            message,\n            hint: None,\n            span: None,\n        }",
    "            message,\n            hint: None,\n            span: None,\n            stack: Vec::new(),\n        }"
)
with open("src/eval.rs", "w") as f:
    f.write(content)
