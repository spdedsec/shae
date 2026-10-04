with open("src/builtins.rs", "r") as f:
    content = f.read()

target = '        ("serve", builtin_serve),'
replacement = '        ("serve", builtin_serve),\n        ("read", builtin_read),\n        ("write", builtin_write),\n        ("fetch", builtin_fetch),'

content = content.replace(target, replacement)
with open("src/builtins.rs", "w") as f:
    f.write(content)
