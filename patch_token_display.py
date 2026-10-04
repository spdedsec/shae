with open("src/token.rs", "r") as f:
    content = f.read()

content = content.replace("            Token::Break => write!(f, \"'break'\"),", "            Token::Break => write!(f, \"'break'\"),\n            Token::Use => write!(f, \"'use'\"),")
with open("src/token.rs", "w") as f:
    f.write(content)
