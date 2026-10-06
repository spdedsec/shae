# Shae Language Roadmap

Shae is evolving into a strict, predictable, yet extremely ergonomic general-purpose programming language.

## Phase 1: Core Semantics (Completed)
- Lexer, Parser, AST, and Tree-Walking Evaluator in Rust.
- Friendly, error-by-default property access and arrays (`?.` and `??` for safety).
- Strict types on operators (e.g. `+` does not coerce strings and numbers implicitly).
- String interpolation (`"Hello {name}"`, escape with `\{`).
- Exact float equality for `==`.
- Builtins: `print`, `dbg`, `len`, `type`, `str`, `num`, `range`, `push`, `pop`, `keys`, `values`, `get`, `json_parse`, `json_stringify`, `serve`.
- Arity checks on function calls.
- Helpful hints in error messages with exact source code snippets and carets.

## Phase 2: Leverage
- [x] String - String & array methods (`.trim()`, `.map()`, `.filter()`, `.reduce()`, `.sort()`, `.sum()`). array methods (`.trim()`, `.map()`, `.filter()`, `.reduce()`, `.sort()`, `.sum()`).
- File I/O (`read()`, `write()`) and network requests (`fetch(url)`).
- Error handling mechanisms (e.g., `try` block or `Result` types).
- [x] Module system (`use "file.shae"`).
- [x] Interactive REPL.
- Built-in `approx(a, b)` for approximate float comparisons.
- [x] `shae check` (static AST linter) and `shae fmt` (formatter).

## Phase 3: Seriousness
- [x] Pragmatic truthiness in conditions (formalized in LANGUAGE.md; empty collections, 0, null, false are falsy).
- [x] Aliases `and`, `or`, `not` for `&&`, `||`, `!`.
- Integer type support (currently all numbers are `f64`).
- [x] Structs, Enums, and pattern matching.
- Pipe operator `|>` for ergonomic data transformations.
- Stack traces for runtime errors.
- [x] Concurrent and robust HTTP `serve` (request headers, bodies, status codes, routing).
- [x] Bytecode VM scaffolding and AST compiler core.
- Type annotations and optional static typing.
- [x] CLI Toolchain (check, fmt)
- [x] OS Standard Library (env, exec, time)
- [x] Try/Catch Error Handling
- [x] Concurrency (`spawn` with `Task` handles and `join`/error propagation)
- [x] Test Runner (`shae test` with `assert` & `assert_eq`), LSP, and Package manager.
