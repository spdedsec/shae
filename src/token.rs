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
    Struct, // struct
    Enum,   // enum
    Match,  // match

    // Symbols
    FatArrow,   // =>
    Pipe,       // |>
    Underscore, // _
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
    Use,
    From,
    As,
    Continue,
    Try,
    Catch,
    Async,
    Await,

    // Identifiers & Literals
    Ident(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
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
    DotDot,         // ..
    DotDotEq,       // ..=
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
    Not,         // !

    // Bitwise Operators
    Amp,   // &
    BitOr, // |
    Caret, // ^
    Tilde, // ~
    Shl,   // <<
    Shr,   // >>

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Struct => write!(f, "'struct'"),
            Token::Enum => write!(f, "'enum'"),
            Token::Match => write!(f, "'match'"),
            Token::FatArrow => write!(f, "'=>'"),
            Token::Pipe => write!(f, "'|>'"),
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
            Token::Use => write!(f, "'use'"),
            Token::From => write!(f, "'from'"),
            Token::As => write!(f, "'as'"),
            Token::Continue => write!(f, "'continue'"),
            Token::Try => write!(f, "'try'"),
            Token::Catch => write!(f, "'catch'"),
            Token::Async => write!(f, "'async'"),
            Token::Await => write!(f, "'await'"),
            Token::Ident(s) => write!(f, "identifier '{}'", s),
            Token::StringLit(s) => write!(f, "string \"{}\"", s),
            Token::IntLit(n) => write!(f, "integer {}", n),
            Token::FloatLit(n) => write!(f, "number {}", n),
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
            Token::DotDot => write!(f, "'..'"),
            Token::DotDotEq => write!(f, "'..='"),
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
            Token::Amp => write!(f, "'&'"),
            Token::BitOr => write!(f, "'|'"),
            Token::Caret => write!(f, "'^'"),
            Token::Tilde => write!(f, "'~'"),
            Token::Shl => write!(f, "'<<'"),
            Token::Shr => write!(f, "'>>'"),
            Token::Eof => write!(f, "end of file"),
        }
    }
}

impl Token {
    pub fn property_name(&self) -> Option<String> {
        match self {
            Token::Ident(s) => Some(s.clone()),
            Token::Fn => Some("fn".to_string()),
            Token::Let => Some("let".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::While => Some("while".to_string()),
            Token::For => Some("for".to_string()),
            Token::In => Some("in".to_string()),
            Token::Return => Some("return".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Match => Some("match".to_string()),
            Token::Struct => Some("struct".to_string()),
            Token::Enum => Some("enum".to_string()),
            Token::Use => Some("use".to_string()),
            Token::From => Some("from".to_string()),
            Token::As => Some("as".to_string()),
            Token::Async => Some("async".to_string()),
            Token::Await => Some("await".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}
