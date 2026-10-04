with open("src/token.rs", "r") as f:
    content = f.read()

content = content.replace("    Null,\n    Break,", "    Null,\n    Break,\n    Use,")
with open("src/token.rs", "w") as f:
    f.write(content)
