with open("src/value.rs", "r") as f:
    content = f.read()

target = """Builtin {
        name: String,
        func: BuiltinFn,
    },"""

bm_variant = """Builtin {
        name: String,
        func: BuiltinFn,
    },
    BoundMethod {
        object: Box<Value>,
        method: String,
    },"""
content = content.replace(target, bm_variant)

with open("src/value.rs", "w") as f:
    f.write(content)
