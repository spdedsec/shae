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

---

### **Chunk 12: VM-Backed Interactive REPL (`shae repl`)** *(Completed)*
- [x] Persistent VM runtime session maintaining globals, function definitions, and closures across input lines.
- [x] Multi-line input accumulation with balance checking for unclosed delimiters (`{`, `(`, `[`).
- [x] REPL meta-commands: `:help`, `:disasm <code>`, `:globals`, `:reset`, and live engine toggling `:engine [vm|ast]`.
- [x] Dual-engine compatibility with seamless fallback to AST interpreter via `--engine=ast`.

---

### **Chunk 13: Standalone Bundler Bytecode Integration (`shae bundle`)** *(Completed)*
- [x] Precompiled bytecode packaging: `shae bundle` compiles entrypoint and all dependency modules into ahead-of-time `.shaec` bytecode chunks.
- [x] Zero-overhead execution: Standalone executables run on the Bytecode VM by default with zero lexing or parsing overhead.
- [x] Source protection (`--strip`): Omit plaintext source code from bundled binaries to distribute pure bytecode executables.
- [x] AST-only mode (`--no-bytecode`): Flag to bundle raw source code without AOT compilation.
- [x] Dual-engine compatibility: Bundled executables support `--engine=ast` fallback when source is retained, and disassembling embedded chunks via `--disasm`.
- [x] 100% backward compatibility with legacy bundle archive JSON schemas.

---

### **Chunk 14: Phase 4 — Actor-Based Concurrency & Distributed Mailboxes** *(Completed)*
- [x] **Lightweight Isolated Actors (`src/actor.rs`, `src/value.rs`)**
  - [x] Dedicated OS thread per actor with zero-idle CPU waiting (`Condvar`).
  - [x] Full memory isolation with recursive deep cloning (`Value::deep_clone`) preventing shared mutable memory and data races.
  - [x] Stateless actors (`actor.spawn(fn(msg))`) and stateful actors (`actor.spawn(state, fn(state, msg))`).
  - [x] Unbounded FIFO message mailboxes (`VecDeque<ActorMessage>`).
  - [x] Non-blocking asynchronous message sending (`actor.send(pid, msg)` and `pid.send(msg)`).
  - [x] Synchronous request-response pattern (`actor.ask(pid, msg, [timeout_ms])` and `pid.ask(msg, [timeout_ms])`).
  - [x] Actor lifecycle controls: `stop()`, `is_alive()`, `len()`, `id()`, and `status()`.
  - [x] Calling actor self-identity inspection (`actor.self_pid()` / `actor.self()`).
  - [x] Self-ask deadlock detection and prevention.
- [x] **Global Actor Registry (`ActorRegistry`)**
  - [x] Named actor registration (`actor.register(name, pid)`).
  - [x] Name resolution to actor handles (`actor.whereis(name)`).
  - [x] Unregistration and duplicate name collision protection (`actor.unregister(name)`, `actor.registered()`).
  - [x] Polymorphic `send(target, msg)` in standard prelude routing to channels, actor handles, and registered names.
- [x] **Hierarchical Supervision Trees & Process Restart Policies**
  - [x] Declarative supervisor specifications (`actor.supervisor(config)`).
  - [x] `OneForOne` restart strategy: isolated restart of crashed child actors with clean initial state.
  - [x] `AllForOne` restart strategy: atomic restart of all sibling child actors upon child failure.
  - [x] Restart frequency rate limiting via sliding time window (`max_restarts` within `within_seconds`).
  - [x] Clean cascading termination of all supervised children on supervisor shutdown.
  - [x] Dynamic runtime child management: `start_child()`, `stop_child()`, `restart_child()`, `get_child()`, `children()`.
- [x] **Dual-Engine Runtime Integration & Standard Library (`std:actor`)**
  - [x] First-class language values: `Value::Actor(Arc<ActorRef>)` and `Value::Supervisor(Arc<SupervisorRef>)`.
  - [x] Standard library module `std:actor` registered in `stdlib.rs` with 13 core exports.
  - [x] Full first-class bound method dispatch in both Tree-Walking AST Evaluator (`eval.rs`) and Bytecode VM (`vm.rs`).
  - [x] Exception-safe depth tracking preserving stack balance in AST try-catch blocks.

---

### **Chunk 15: Phase 5 (Stage 1) — Pure-Shae Compiler Frontend & Self-Parsing Verification** *(Completed)*
- [x] **Lexical Tokens & Keywords (`lib/compiler/token.shae`)**
  - [x] Complete token specifications with kinds, symbols, keywords, line/column tracking, and constructors.
- [x] **Abstract Syntax Tree Definitions (`lib/compiler/ast.shae`)**
  - [x] Full AST node constructors for statements (`let`, `assign`, `fn`, `if`, `while`, `for`, `return`, `try/catch`, `struct`, `enum`, `use`) and expressions (`literal`, `binary`, `unary`, `call`, `get`, `index`, `array`, `map`, `lambda`, `match`, `struct_init`).
- [x] **Pure-Shae Tokenizer & Lexer (`lib/compiler/lexer.shae`)**
  - [x] Radix literals (hex `0x`, binary `0b`, octal `0o`), floating-point numbers, multi-character operators, raw string literals, string escape sequences, and comment stripping.
- [x] **Pure-Shae Pratt Parser (`lib/compiler/parser.shae`)**
  - [x] Top-down operator precedence Pratt parsing with configurable infix/prefix binding powers.
  - [x] Compound statement parsing, struct and enum declarations, anonymous closures, destructuring patterns, and error synchronization.
- [x] **Self-Hosting Verification (`tests/shae/self_parser_test.shae`)**
  - [x] Self-parsing of all compiler source files (`token.shae`, `ast.shae`, `lexer.shae`, `parser.shae`) totaling 7,799+ tokens and 140 statements.
  - [x] Strict dual-engine parity verified on both Bytecode VM (`run`) and AST Evaluator (`run --engine=ast`).

---

### **Chunk 16: Phase 5 (Stage 2) — Pure-Shae Bytecode Compiler & Serializer (`lib/compiler/codegen.shae`)** *(In Progress)*
- [ ] **Opcode Definitions & Bytecode Chunk (`lib/compiler/opcode.shae`, `lib/compiler/chunk.shae`)**
  - [ ] Numerical opcode mapping matching `src/opcode.rs`.
  - [ ] Binary serialization of bytecode format (`\x7fSHAE`, version 1, code, lines, constant pool).
- [ ] **AST-to-Bytecode Code Generator (`lib/compiler/codegen.shae`)**
  - [ ] Local variable resolution, lexical scope tracking, and upvalue capture for closures.
  - [ ] Expression compilation, arithmetic/bitwise operators, collections, and control flow jumps.
- [ ] **Bytecode Disassembler in Pure Shae (`lib/compiler/disasm.shae`)**
- [ ] **Self-Codegen Integration Test Suite (`tests/shae/self_codegen_test.shae`)**

---

### **Chunk 17: Phase 5 (Stage 3) — Direct Native Linker (ELF / PE Object Emission)**
- [ ] Direct ELF 64-bit and PE executable emission with zero external linker dependencies.

---

### **Chunk 18: Phase 6 — WebAssembly & Edge Runtime Target**
- [ ] `wasm32-wasi` and browser WebAssembly codegen and embed runner.

---

### **Chunk 19: Phase 7 — Distributed Actor Mesh (`std:actor:cluster`)**
- [ ] Distributed network clustering over TCP/TLS with cross-node actor discovery and messaging.





