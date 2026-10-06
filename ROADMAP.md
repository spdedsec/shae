# Shae Language Roadmap

Shae is evolving into a strict, predictable, yet extremely ergonomic general-purpose programming language.

## Phase 1: Core Semantics (Completed)
- Lexer, Parser, AST, and Tree-Walking Evaluator in Rust.
- Friendly, error-by-default property access and arrays (`?.` and `??` for safety).
- Strict types on operators (e.g. `+` does not coerce strings and numbers implicitly).
- String interpolation (`"Hello \{name}"`).
- Exact float equality for `==`.
- Builtins: `print`, `dbg`, `len`, `type`, `str`, `num`, `range`, `push`, `pop`, `keys`, `values`, `get`, `json_parse`, `json_stringify`, `serve`.
- Arity checks on function calls.
- Helpful hints in error messages with exact source code snippets and carets.

## Phase 2: Leverage
- [x] String - String & array methods (`.trim()`, `.map()`, `.filter()`, `.reduce()`, `.sort()`, `.sum()`). array methods (`.trim()`, `.map()`, `.filter()`, `.reduce()`, `.sort()`, `.sum()`).
- File I/O (`read()`, `write()`) and network requests (`fetch(url)`).
- Error handling mechanisms (e.g., `try` block or `Result` types).
- [x] Module system (`use "file.shae"`).
- Multi-line REPL.
- Built-in `approx(a, b)` for approximate float comparisons.
- `shae check` (linter) and `shae fmt` (formatter).

## Phase 3: Seriousness
- Strict booleans in conditions (resolving the open design question of truthiness vs explicit booleans).
- [x] Aliases `and`, `or`, `not` for `&&`, `||`, `!`.
- Integer type support (currently all numbers are `f64`).
- [x] Structs, Enums, and pattern matching.
- Pipe operator `|>` for ergonomic data transformations.
- Stack traces for runtime errors.
- Concurrent and robust HTTP `serve` (request headers, bodies, status codes, routing).
- [x] Bytecode VM scaffolding and AST compiler core.
- Type annotations and optional static typing.
- Package manager, LSP, and Test Runner.
