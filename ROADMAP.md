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

### **Chunk 16: Phase 5 (Stage 2) — Pure-Shae Bytecode Compiler & Serializer (`lib/compiler/codegen.shae`)** *(Completed)*
- [x] **Opcode Definitions & Bytecode Chunk (`lib/compiler/opcode.shae`, `lib/compiler/chunk.shae`)**
  - [x] Numerical opcode mapping matching `src/opcode.rs`.
  - [x] Binary serialization of bytecode format (`\x7fSHAE`, version 1, code, lines, constant pool).
- [x] **AST-to-Bytecode Code Generator (`lib/compiler/codegen.shae`)**
  - [x] Local variable resolution, lexical scope tracking, and upvalue capture for closures.
  - [x] Expression compilation, arithmetic/bitwise operators, collections, and control flow jumps.
  - [x] Direct file compilation pipeline (`compile_file_to_shaec`).
- [x] **Bytecode Disassembler in Pure Shae (`lib/compiler/disasm.shae`)**
  - [x] Human-readable instruction formatting with constant pool inspection and recursive compiled function disassembly.
- [x] **Self-Codegen Integration Test Suite (`tests/shae/self_codegen_test.shae`, `tests/self_codegen_test.rs`)**
  - [x] Strict dual-engine parity verified on both Bytecode VM (`run_file_vm`) and AST Evaluator (`run_file`).
  - [x] Direct execution of pure-Shae emitted `.shaec` bytecode verified by Shae VM runtime with exact computed results.

---

### **Chunk 17: Phase 5 (Stage 3) — Direct Native Linker (ELF / PE Object Emission)** *(Completed)*
- [x] **Pure-Shae Direct ELF 64-bit Linker (`lib/compiler/elf.shae`)**
  - [x] Full ELF 64-bit header construction (`\x7fELF`, ET_EXEC, EM_X86_64, EM_AARCH64, EV_CURRENT).
  - [x] Program header table (`PT_LOAD` segment definitions with virtual addresses and execute/read/write permissions).
  - [x] Standalone executable binary assembly with bootstrap runtime stub and embedded bytecode trailer.
- [x] **Pure-Shae Direct PE 64-bit Linker (`lib/compiler/pe.shae`)**
  - [x] MS-DOS header (`MZ`), DOS stub, and PE signature offset (`e_lfanew = 0x80`).
  - [x] COFF header (Machine AMD64, section count, optional header size, executable characteristics).
  - [x] PE32+ 64-bit Optional Header (ImageBase, AddressOfEntryPoint, SectionAlignment, FileAlignment, Windows CUI subsystem).
  - [x] Section headers (`.text`, `.data`) and aligned section raw data emission.
- [x] **Universal Linker Orchestration (`lib/compiler/linker.shae`)**
  - [x] Cross-platform linking pipeline (`link_source_to_elf`, `link_source_to_pe`, `link_file`).
- [x] **Rust CLI Integration & Native Linker (`src/linker.rs`, `shae link`)**
  - [x] CLI command: `shae link <file.shae> [-o <out>] [--format elf|pe] [--strip]`.
  - [x] Direct execution of linked native ELF binary verified as standalone OS process with exact exit status and output.
- [x] **Dual-Engine Test Verification (`tests/shae/self_linker_test.shae`, `tests/native_linker_test.rs`)**
  - [x] 100% test pass on both Bytecode VM and AST Evaluator.

---

### **Chunk 18: Phase 6 — WebAssembly & Edge Runtime Target** *(Completed)*
- [x] **WebAssembly Binary Generator (`lib/compiler/wasm.shae`)**
  - [x] WebAssembly binary spec compliance (`\0asm`, version 1, LEB128 unsigned/signed integer encoders).
  - [x] Core Wasm types (`i32`, `i64`, `f32`, `f64`, `funcref`, `externref`).
  - [x] Wasm sections: Type (0x01), Function (0x03), Memory (0x05), Export (0x07), and Code (0x0a).
  - [x] Pure-Shae WebAssembly AST module builder and binary serializer.
- [x] **Rust WebAssembly Compiler & Integrated Runner (`src/wasm.rs`, `shae wasm`)**
  - [x] Compiles Shae function declarations into valid WebAssembly binary bytecode (`.wasm`).
  - [x] Built-in zero-dependency `WasmRunner` interpreter executing WASM functions, parameters, local variables, nested function calls (`OP_CALL`), arithmetic, and bitwise operations.
  - [x] CLI command: `shae wasm <file.shae> [-o <out.wasm>] [--run]`.
- [x] **Browser Runtime & Playground (`examples/wasm_demo/index.html`)**
  - [x] Web browser interface instantiating and executing Shae-compiled WebAssembly binaries via `WebAssembly.instantiate`.
- [x] **Dual-Engine Test Verification (`tests/shae/self_wasm_test.shae`, `tests/wasm_test.rs`)**
  - [x] 100% test pass on both Bytecode VM and AST Evaluator.

---

### **Chunk 19: Phase 7 — Distributed Actor Mesh (`std:actor:cluster`)**
- [ ] Distributed network clustering over TCP/TLS with cross-node actor discovery and messaging.





