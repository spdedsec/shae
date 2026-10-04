use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Code {
        src: String,
        line: usize,
        col: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Struct,        // struct
    Enum,          // enum
    Match,         // match
    
    // Symbols
    FatArrow,      // =>
    Underscore,    // _
    Let,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    True,
    False,
    Null,
    Break,
    Continue,

    // Identifiers & Literals
    Ident(String),
    StringLit(String),
    NumberLit(f64),
    Interp(Vec<StrPart>),

    // Symbols & Punctuation
    LParen,         // (
    RParen,         // )
    LBrace,         // {
    RBrace,         // }
    LBracket,       // [
    RBracket,       // ]
    Comma,          // ,
    Colon,          // :
    Semicolon,      // ; (optional, allowed but not required)
    Dot,            // .
    Question,       // ?
    DoubleQuestion, // ??
    Arrow,          // ->

    // Operators
    Plus,        // +
    Minus,       // -
    Star,        // *
    Slash,       // /
    Percent,     // %
    Equals,      // =
    PlusEquals,  // +=
    MinusEquals, // -=
    EqEq,        // ==
    NotEq,       // !=
    Lt,          // <
    LtEq,        // <=
    Gt,          // >
    GtEq,        // >=
    And,         // &&
    Or,          // ||
    Not,        // !

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Struct => write!(f, "'struct'"),
            Token::Enum => write!(f, "'enum'"),
            Token::Match => write!(f, "'match'"),
            Token::FatArrow => write!(f, "'=>'"),
            Token::Underscore => write!(f, "'_'"),
            Token::Let => write!(f, "'let'"),
            Token::Fn => write!(f, "'fn'"),
            Token::Return => write!(f, "'return'"),
            Token::If => write!(f, "'if'"),
            Token::Else => write!(f, "'else'"),
            Token::While => write!(f, "'while'"),
            Token::For => write!(f, "'for'"),
            Token::In => write!(f, "'in'"),
            Token::True => write!(f, "'true'"),
            Token::False => write!(f, "'false'"),
            Token::Null => write!(f, "'null'"),
            Token::Break => write!(f, "'break'"),
            Token::Continue => write!(f, "'continue'"),
            Token::Ident(s) => write!(f, "identifier '{}'", s),
            Token::StringLit(s) => write!(f, "string \"{}\"", s),
            Token::NumberLit(n) => write!(f, "number {}", n),
            Token::Interp(_) => write!(f, "interpolated string"),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
            Token::LBrace => write!(f, "'{{'"),
            Token::RBrace => write!(f, "'}}'"),
            Token::LBracket => write!(f, "'['"),
            Token::RBracket => write!(f, "']'"),
            Token::Comma => write!(f, "','"),
            Token::Colon => write!(f, "':'"),
            Token::Semicolon => write!(f, "';'"),
            Token::Dot => write!(f, "'.'"),
            Token::Question => write!(f, "'?'"),
            Token::DoubleQuestion => write!(f, "'??'"),
            Token::Arrow => write!(f, "'->'"),
            Token::Plus => write!(f, "'+'"),
            Token::Minus => write!(f, "'-'"),
            Token::Star => write!(f, "'*'"),
            Token::Slash => write!(f, "'/'"),
            Token::Percent => write!(f, "'%'"),
            Token::Equals => write!(f, "'='"),
            Token::PlusEquals => write!(f, "'+='"),
            Token::MinusEquals => write!(f, "'-='"),
            Token::EqEq => write!(f, "'=='"),
            Token::NotEq => write!(f, "'!='"),
            Token::Lt => write!(f, "'<'"),
            Token::LtEq => write!(f, "'<='"),
            Token::Gt => write!(f, "'>'"),
            Token::GtEq => write!(f, "'>='"),
            Token::And => write!(f, "'and'"),
            Token::Or => write!(f, "'or'"),
            Token::Not => write!(f, "'not'"),
            Token::Eof => write!(f, "end of file"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}
