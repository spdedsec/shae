# Shae: Advanced Types (Structs, Enums, Pattern Matching)

## 1. Syntax & Grammar

### Structs
```rust
struct User {
    name,
    age,
    is_active
}

// Strict Instantiation
let u = User { name: "Satya", age: 19, is_active: true }

// Access
print(u.name)
```
- No implicit missing fields (missing = error).
- No extra fields allowed during initialization.

### Enums
```rust
enum Result {
    Ok(value),
    Err(message, code)
}

// Initialization via Dot Access (constructor function)
let success = Result.Ok("Data loaded")
let failure = Result.Err("Not found", 404)
```
- Defined as tagged unions.
- Variants act as factory functions returning an `EnumInstance`.

### Pattern Matching
```rust
let res = Result.Ok("Data")

let output = match res {
    Result.Ok(val) => "Success: " + val,
    Result.Err(msg, code) => "Error \{code}: \{msg}",
    _ => "Unknown"
}
```
- `match` is an expression.
- Matches are evaluated top-to-bottom.
- Wildcard `_` acts as a catch-all fallback.

## 2. AST Representation
```rust
pub enum StmtKind {
    StructDef { name: String, fields: Vec<String> },
    EnumDef { name: String, variants: Vec<EnumVariant> },
}
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<String>,
}

pub enum Expr {
    StructInit { name: String, fields: Vec<(String, Expr)>, span: Span },
    Match { target: Box<Expr>, arms: Vec<MatchArm>, span: Span },
}
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Box<Expr>,
}
pub enum Pattern {
    Wildcard,
    Variable(String),
    Literal(Literal),
    Enum { enum_name: String, variant_name: String, fields: Vec<String> }
}
```

## 3. Runtime Value (`eval.rs` & `value.rs`)
- `StructDef` and `EnumDef` are stored in the Environment.
- When `Expr::Get` reads `Result.Ok`, it returns `Value::EnumConstructor`.
- When `Value::EnumConstructor` is called, it returns `Value::EnumInstance`.
- `Expr::Match` tests the target `Value` against each `Pattern`. Bindings are injected into a new block-scoped Environment for the arm's body.
