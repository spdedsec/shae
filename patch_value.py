with open("src/value.rs", "r") as f:
    content = f.read()

bm_variant = """
    BoundMethod {
        object: Box<Value>,
        method: String,
    },
"""
content = content.replace("    Builtin {\n        name: String,\n        func: BuiltinFn,\n    },", "    Builtin {\n        name: String,\n        func: BuiltinFn,\n    },\n" + bm_variant)

content = content.replace('Value::Builtin { .. } => "builtin_function",', 'Value::Builtin { .. } => "builtin_function",\n            Value::BoundMethod { .. } => "bound_method",')
content = content.replace('Value::Function { .. } | Value::Builtin { .. } => true,', 'Value::Function { .. } | Value::Builtin { .. } | Value::BoundMethod { .. } => true,')
content = content.replace('Value::Builtin { name, .. } => format!("<builtin {}>", name),', 'Value::Builtin { name, .. } => format!("<builtin {}>", name),\n            Value::BoundMethod { method, .. } => format!("<bound method {}>", method),')
content = content.replace('Value::Builtin { name, .. } => JsonValue::String(format!("<builtin {}>", name)),', 'Value::Builtin { name, .. } => JsonValue::String(format!("<builtin {}>", name)),\n            Value::BoundMethod { method, .. } => JsonValue::String(format!("<bound method {}>", method)),')

with open("src/value.rs", "w") as f:
    f.write(content)
