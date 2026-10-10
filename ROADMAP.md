# Shae Language Roadmap

Shae is evolving into a strict, predictable, yet extremely ergonomic general-purpose programming language.

---

### **Chunk 1: Language Ergonomics & Core Primitives** *(Completed)*
- [x] **Integer & Number System**
  - [x] Distinct integer and float types (`Value::Int(i64)` and `Value::Float(f64)`).
  - [x] Radix literals: hexadecimal (`0xFF`), binary (`0b1010`), octal (`0o77`), underscore separators (`1_000_000`).
  - [x] Bitwise operators: `&` (AND), `|` (OR), `^` (XOR), `~` (NOT), `<<` (SHL), `>>` (SHR).
  - [x] Integer array indexing with whole-number verification.
- [x] **Pattern Matching Improvements**
  - [x] Match arm guards: `Pattern if condition => expr`.
  - [x] Range patterns: half-open `start..end` and inclusive `start..=end`.
  - [x] Wildcard variant bindings (`Option.Some(_)`).
- [x] **Destructuring Bindings**
  - [x] Array destructuring in `let`: `let [a, b, ..rest] = list`.
  - [x] Struct/Map destructuring in `let`: `let { name, role, ..rest } = user`.
  - [x] Static AST linter validation for destructuring patterns and scope collision detection.
- [x] **Standard Array/String Methods**
  - [x] Array methods: `.find()`, `.some()`, `.every()`, `.flat()`, `.join()`, `.reverse()`, `.slice(start, end)`.
  - [x] String methods: `.starts_with()`, `.ends_with()`, `.contains()`, `.pad_start()`, `.lines()`, `.chars()`.

---

### **Chunk 2: Code Formatter (`shae fmt`) & AST Linter Polish** *(Completed)*
- [x] **AST Pretty-Printer (`src/fmt.rs`)**
  - [x] Canonical indentation (4 spaces), brace placement, and operator formatting.
  - [x] CLI integration: `shae fmt <file.shae>` and `shae fmt --check <file.shae>`.
- [x] **Linter Expansion (`src/linter.rs`)**
  - [x] Unused variable and unused parameter tracking with `_` prefix suppression.
  - [x] Dead/unreachable code warnings after `return`, `break`, `continue`.
  - [x] Variable shadowing warnings (built-in shadowing and nested outer scope shadowing).

---

### **Chunk 3: Bytecode VM — Control Flow & Local Variables** *(Completed)*
- [x] Local variable resolution at compile time (`OpGetLocal`, `OpSetLocal`).
- [x] Conditional and unconditional jumps (`OpJump`, `OpJumpIfFalse`, `OpLoop`).
- [x] Control flow compilation: `if/else`, `while`, `for in` (with loop contexts and `break`/`continue` scope cleanup).
- [x] Comparison & boolean bytecode operations with short-circuiting (`and`, `or`).

---

### **Chunk 4: Bytecode VM — Call Frames, Functions & Closures** *(Completed)*
- [x] Explicit call frames (`CallFrame`) with virtual stack allocation and function calls.
- [x] Closures and upvalue capture (`OpGetUpvalue`, `OpSetUpvalue`, `OpCloseUpvalue`).
- [x] Composite types and indexing in bytecode (`OpBuildArray`, `OpBuildMap`, `OpGetIndex`, `OpSetIndex`).

---

### **Chunk 5: Memory Efficiency & Garbage Collection Foundation** *(Completed)*
- [x] Transition from large `Arc<RwLock<...>>` wrappers to managed heap allocations.
- [x] Mark-and-sweep garbage collection engine with stack and global root tracing.

---

### **Chunk 6: Structured Module System & Standard Namespaces** *(Completed)*
- [x] Relative file imports and selective exports (`use { a, b } from "./mod.shae"`).
- [x] Standard library namespaces: `std:fs`, `std:path`, `std:sys`, `std:time`.

---

### **Chunk 7: Concurrency Modernization & Asynchronous I/O** *(Completed)*
- [x] Message-passing channels (`channel(capacity)`, `send()`, `recv()`).
- [x] Non-blocking event-driven web server engine with keep-alive and routing parameters.
- [x] TLS / HTTPS built-in server support via `rustls`.

---

### **Chunk 8: Expanded Standard Library (Networking, Crypto, Regex)** *(Completed)*
- [x] `std:crypto`: SHA-256, SHA-512, HMAC, secure random bytes.
- [x] `std:regex`: PCRE-compatible regex matching, search, and substitution.
- [x] `std:codec`: Base64 and URL encoding/decoding.

---

### **Chunk 9: Package Manager & Project System (`shae pkg`)** *(Completed)*
- [x] Project manifests (`shae.toml`) and lockfiles (`shae.lock`).
- [x] `shae add` and `shae install` with Git dependency resolution.

---

### **Chunk 10: Language Server Protocol (LSP) & Standalone Bundler** *(Completed)*
- [x] Language Server Protocol daemon (`shae lsp`) with diagnostics and auto-completion.
- [x] Single-binary standalone application bundler (`shae bundle`).

---

### **Chunk 11: Bytecode Compilation & Serialization (`shae compile` / `.shaec`)** *(Completed)*
- [x] Compact binary bytecode serialization format with magic header (`\x7fSHAE`) and versioning.
- [x] Constant pool serialization supporting primitive types, strings, structs, enums, arrays, and compiled functions.
- [x] CLI compiler: `shae compile <file.shae> [-o <out.shaec>] [--disasm]`.
- [x] Direct bytecode execution (`shae run file.shaec` and `shae file.shaec`).
- [x] Bytecode module resolution and imports (`use { func } from "./module.shaec"`).
- [x] Precompiled bytecode disassembler (`shae disasm file.shaec`).

