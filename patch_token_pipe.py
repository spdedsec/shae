with open("src/token.rs", "r") as f:
    content = f.read()

content = content.replace('            Token::FatArrow => write!(f, "\'=>\'"),', '            Token::FatArrow => write!(f, "\'=>\'"),\n            Token::Pipe => write!(f, "\'|>\'"),')

with open("src/token.rs", "w") as f:
    f.write(content)
