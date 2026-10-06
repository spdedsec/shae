use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let {
        name: String,
        init: Expr,
    },
    Assign {
        target: Expr,
        value: Expr,
    },
    FnDef {
        name: String,
        params: Rc<Vec<String>>,
        body: Rc<Vec<Stmt>>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    TryCatch {
        try_body: Vec<Stmt>,
        catch_ident: String,
        catch_body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        item: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Expr(Expr),

    StructDef {
        name: String,
        fields: Vec<String>,
    },
    EnumDef {
        name: String,
        variants: Vec<EnumVariant>,
    },

}

#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Variable(String),
    Literal(Literal),
    Enum {
        enum_name: String,
        variant_name: String,
        fields: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Variable {
        name: String,
        span: Span,
    },
    Array(Vec<Expr>),
    Map(Vec<(String, Expr)>),
    Interpolated(Vec<InterpPart>),
    Use {
        path: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Get {
        target: Box<Expr>,
        property: String,
        safe: bool,
        span: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },

    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    Match {
        target: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },

    Lambda {
        params: Rc<Vec<String>>,
        body: Rc<Vec<Stmt>>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Option<Span> {
        match self {
            Expr::Variable { span, .. } => Some(*span),
            Expr::Binary { span, .. } => Some(*span),
            Expr::Unary { span, .. } => Some(*span),
            Expr::Call { span, .. } => Some(*span),
            Expr::Get { span, .. } => Some(*span),
            Expr::Index { span, .. } => Some(*span),
            Expr::Lambda { span, .. } => Some(*span),

            Expr::StructInit { span, .. } => Some(*span),
            Expr::Match { span, .. } => Some(*span),
            Expr::Use { span, .. } => Some(*span),

            // We could extract spans from Array, Map, Interpolated if we wanted,
            // but for runtime errors, typical sources are variables, properties, function calls, and operators.
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Pipe,
    Coalesce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}
