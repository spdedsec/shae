with open("src/lexer.rs", "r") as f:
    content = f.read()

content = content.replace('"continue" => Token::Continue,', '"continue" => Token::Continue,\n            "use" => Token::Use,')
with open("src/lexer.rs", "w") as f:
    f.write(content)
