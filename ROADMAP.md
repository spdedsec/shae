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

### **Chunk 3: Bytecode VM — Control Flow & Local Variables** *(Next)*
- [ ] Local variable resolution at compile time (`OpGetLocal`, `OpSetLocal`).
- [ ] Conditional and unconditional jumps (`OpJump`, `OpJumpIfFalse`).
- [ ] Control flow compilation: `if/else`, `while`, `for in`.
- [ ] Comparison & boolean bytecode operations with short-circuiting.

---

### **Chunk 4: Bytecode VM — Call Frames, Functions & Closures**
- [ ] Explicit call frames (`CallFrame`) with virtual stack allocation.
- [ ] Closures and upvalue capture (`OpGetUpvalue`, `OpSetUpvalue`, `OpCloseUpvalue`).
- [ ] Composite types and indexing in bytecode (`OpBuildArray`, `OpBuildMap`, `OpGetIndex`, `OpSetIndex`).

---

### **Chunk 5: Memory Efficiency & Garbage Collection Foundation**
- [ ] Transition from large `Arc<RwLock<...>>` wrappers to managed heap allocations.
- [ ] Mark-and-sweep garbage collection engine with stack and global root tracing.

---

### **Chunk 6: Structured Module System & Standard Namespaces**
- [ ] Relative file imports and selective exports (`use { a, b } from "./mod.shae"`).
- [ ] Standard library namespaces: `std:fs`, `std:path`, `std:sys`, `std:time`.

---

### **Chunk 7: Concurrency Modernization & Asynchronous I/O**
- [ ] Message-passing channels (`channel(capacity)`, `send()`, `recv()`).
- [ ] Non-blocking event-driven web server engine with keep-alive and routing parameters.
- [ ] TLS / HTTPS built-in server support via `rustls`.

---

### **Chunk 8: Expanded Standard Library (Networking, Crypto, Regex)**
- [ ] `std:crypto`: SHA-256, SHA-512, HMAC, secure random bytes.
- [ ] `std:regex`: PCRE-compatible regex matching, search, and substitution.
- [ ] `std:codec`: Base64 and URL encoding/decoding.

---

### **Chunk 9: Package Manager & Project System (`shae pkg`)**
- [ ] Project manifests (`shae.toml`) and lockfiles (`shae.lock`).
- [ ] `shae add` and `shae install` with Git dependency resolution.

---

### **Chunk 10: Language Server Protocol (LSP) & Standalone Bundler**
- [ ] Language Server Protocol daemon (`shae lsp`) with diagnostics and auto-completion.
- [ ] Single-binary standalone application bundler (`shae bundle`).
