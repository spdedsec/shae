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

### **Chunk 19: Phase 7 — Distributed Actor Mesh (`std:actor:cluster`)** *(Completed)*
- [x] **Network Clustering Engine & TCP Framing (`src/actor_cluster.rs`)**
  - [x] Length-delimited binary TCP wire framing with 64MB protection threshold.
  - [x] Bidirectional node peer connections (`PeerConnection`) with connection-specific reader threads.
  - [x] Secure handshake protocol with optional authentication cookie verification and rejection.
  - [x] Non-blocking message dispatch (`Send`) and correlated request-response (`Ask` / `Reply`) with timeouts.
  - [x] Multi-node publish-subscribe and event broadcast mesh (`Broadcast` / `on_broadcast`).
- [x] **Remote Actor Handles & Universal Addressing (`RemoteActorRef`, `ClusterNode`)**
  - [x] First-class `Value::RemoteActor` and `Value::ClusterNode` language values.
  - [x] Erlang-style `actor@node` polymorphic addressing in prelude `send(target, msg)`.
  - [x] Dynamic discovery and cross-cluster actor resolution (`node.whereis()`).
  - [x] Full first-class bound method dispatch in both Tree-Walking AST Evaluator (`eval.rs`) and Bytecode VM (`vm.rs`).
- [x] **Pure-Shae Cluster Coordination Library (`lib/actor/cluster.shae`)**
  - [x] Distributed round-robin worker pool (`make_worker_pool`) distributing tasks across local and remote nodes.
  - [x] Distributed leader election (`make_leader_election`) using bully algorithm ranking.
- [x] **Showcase Demo & Dual-Engine Verification (`examples/actor_cluster_demo.shae`, `tests/actor_cluster_test.rs`)**
  - [x] End-to-end multi-node analytics showcase demonstrating clustering, remote ask, worker pools, and leader election.
  - [x] 100% test pass on both Bytecode VM and AST Evaluator.







---

### **Chunk 20: Self-Hosting Bootstrap Pipeline (`lib/compiler/main.shae`)** *(Completed)*
- [x] **Pure-Shae Compiler Driver Suite (`lib/compiler/main.shae`)**
  - [x] Unified compiler driver coordinating parsing, bytecode generation, serialization, disassembly, native linking, and WebAssembly emission.
  - [x] Direct dispatch API: `compile_to_bytecode`, `disassemble_bytecode`, `link_to_native`, `compile_to_wasm`, and `driver_dispatch`.
- [x] **Bootstrap Integration & Verification Suite (`tests/shae/self_bootstrap_test.shae`, `tests/self_bootstrap_test.rs`)**
  - [x] End-to-end self-hosting pipeline test verifying ahead-of-time `.shaec` bytecode compilation with magic header check.
  - [x] Bytecode disassembly verification.
  - [x] Standalone 64-bit ELF native binary linkage with magic header check.
  - [x] WebAssembly (`.wasm`) binary module generation with magic header check.
  - [x] 100% dual-engine test pass across both Bytecode VM and Tree-Walking AST Evaluator.

---

### **Chunk 21: Cluster Transport Security — TLS & Mutual-TLS (`mTLS`)** *(Completed)*
- [x] **Encrypted Wire Transport & Zero-Contention Framing (`src/actor_cluster.rs`)**
  - [x] Unified `ClusterTransport` abstraction handling raw TCP, TLS Server, and TLS Client with Rustls 0.23.
  - [x] Adaptive non-blocking frame polling (`read_frame_adaptive`) and asynchronous channel-driven frame flushing eliminating mutex lock contention on network I/O.
  - [x] Support for file-path and in-memory PEM string certificate/key loading (`load_certs`, `load_key`).
- [x] **Mutual-TLS (mTLS) Authentication & Strict Peer Verification**
  - [x] Root CA certificate validation via `RootCertStore` and `WebPkiClientVerifier`.
  - [x] Client certificate presentation during TLS handshake (`with_client_auth_cert`).
  - [x] Automatic rejection of untrusted or invalid client/peer certificates.
  - [x] Configurable certificate verification modes (`verify_peer: true|false`, SNI `server_name`).
- [x] **First-Class Language & Standard Library Integration (`std:actor:cluster`)**
  - [x] Extended `cluster.node(...)` options map accepting `{ "tls": { "cert": ..., "key": ..., "ca": ..., "verify_peer": ... } }`.
  - [x] `node.is_tls()` / `is_tls(node)` reflection method available across AST Evaluator and Bytecode VM.
- [x] **Dual-Engine Test Verification Suite (`tests/shae/actor_cluster_tls_test.shae`, `tests/actor_cluster_tls_test.rs`)**
  - [x] TLS encrypted messaging verification.
  - [x] mTLS mutual certificate authentication and synchronous ask verification.
  - [x] Untrusted rogue peer handshake rejection verification.
  - [x] 100% dual-engine test pass across both Bytecode VM and Tree-Walking AST Evaluator.

---

### **Chunk 22: First-Class `async` / `await` Coroutines (`std:async`)** *(Completed)*
- [x] **First-Class Task Lifecycle & Idempotent Awaiting (`src/value.rs`, `TaskInner`)**
  - [x] Thread-safe `TaskInner` container supporting repeated idempotent awaiting without "already awaited" panics.
  - [x] First-class Task instance methods: `task.join()`, `task.wait()`, `task.is_done()`, `task.status()`, and `task.result()`.
  - [x] State inspection transitions: `"pending"`, `"completed"`, and `"failed"`.
- [x] **Standard Asynchronous Runtime Library (`std:async`)**
  - [x] `async.spawn(fn, ...args)`: Spawns arbitrary callables or closures as background coroutine tasks.
  - [x] `async.all(tasks)`: Concurrently aggregates multiple tasks and resolves to an array of results.
  - [x] `async.race(tasks)`: Selects the earliest completing coroutine task.
  - [x] `async.sleep(ms)`: Non-blocking coroutine sleep timer.
  - [x] `async.timeout(ms, task)`: Deadline enforcement and timeout handling for async tasks.
  - [x] Non-blocking completion predicates: `async.is_done(task)` and `async.status(task)`.
- [x] **Dual-Engine Test Verification Suite (`tests/shae/async_coroutine_test.shae`, `tests/async_coroutine_test.rs`)**
  - [x] 100% test pass on both Tree-Walking AST Evaluator and Bytecode Stack VM.

---

### **Chunk 23: Standard Documentation Engine (`shae doc`)** *(Completed)*
- [x] **Documentation Comment & Symbol Parser (`src/doc.rs`)**
  - [x] Item doc comments (`///`) and inner module/file doc comments (`//!`).
  - [x] Extraction of function signatures (`fn`, `async fn`), parameters, structs, enums with variants, and constants.
  - [x] Line number and symbol location tracking.
- [x] **Static HTML Site Generator & Markdown Renderer**
  - [x] Self-contained responsive HTML site generation with dark theme, sidebar navigation, badges, and client-side instant search filtering.
  - [x] Clean GitHub Flavored Markdown reference generation (`README.md`).
  - [x] Structured JSON schema model export (`doc.json`).
- [x] **Recursive Project Scanner & CLI Tooling (`shae doc`)**
  - [x] Multi-file recursive project scanner reading `shae.toml` package metadata.
  - [x] CLI subcommand `shae doc [path] [-o <DIR>] [--format html|md|json|all] [--open]`.
- [x] **Integration Test Verification Suite (`tests/doc_gen_test.rs`)**
  - [x] Single-module extraction test for all symbol kinds.
  - [x] HTML, Markdown, and JSON serialization verification.
  - [x] End-to-end multi-module project documentation build verification.

---

### **Chunk 24: Package Registry & Publishing (`shae pkg pack` / `publish`)** *(Completed)*
- [x] **Package Archive Engine (`.shaepkg` / gzip-compressed tarballs in `src/pkg.rs`)**
  - [x] Deterministic sorted file archiving omitting build artifacts, git repositories, and dependencies (`target/`, `.git/`, `.packages/`, `node_modules/`, `doc/`, `*.shaepkg`).
  - [x] SHA-256 cryptographic integrity checksum computation and validation.
  - [x] Safe extraction routine (`unpack_package`) with strict path traversal prevention.
- [x] **Registry Publishing & Verification Pipeline (`shae pkg publish`, `shae pkg verify`)**
  - [x] Dual-mode publishing engine supporting local filesystem registries (`file://`) and remote HTTP(S) API registries.
  - [x] Bearer token authentication support via CLI flags (`--token`) or environment variable (`SHAE_REGISTRY_TOKEN`).
  - [x] Dry-run verification mode (`--dry-run`) validating manifest entries and packing integrity.
  - [x] Atomic registry index updating (`index.json`) for local and distributed registries.
- [x] **CLI Tooling Integration (`src/main.rs`)**
  - [x] Subcommands `shae pkg pack [dir] [-o <file>]`, `shae pkg publish [dir] [--registry <url>] [--token <tok>] [--dry-run]`, and `shae pkg verify <pkg> <sha256>`.
  - [x] Updated CLI help and manifest definitions.
- [x] **Integration Test Verification Suite (`tests/pkg_publish_test.rs`)**
  - [x] Archive packing and file count verification.
  - [x] Exact round-trip byte-for-byte extraction and restoration verification.
  - [x] SHA-256 integrity verification and tamper rejection verification.
  - [x] Dry-run and local filesystem registry publishing verification.

---

### **Chunk 25: Pure-Shae Test Runner (`shae test`) & Live Documentation Server (`shae doc --serve`)** *(Completed)*
- [x] **Native Standard Assertion Framework (`std:test`, `src/stdlib.rs`)**
  - [x] `test(description, callable)`: Isolated test case registration and execution with sub-millisecond execution timing.
  - [x] `assert(condition, [message])`: Descriptive condition assertion error.
  - [x] `assert_eq(actual, expected, [message])`: Formatted expected vs actual equality diffing.
  - [x] `assert_ne(actual, expected, [message])`: Inequality assertion with clear error reporting.
  - [x] `assert_throws(callable, [message])`: Exception raising verification supporting both sync and async callables.
- [x] **CLI Test Runner Engine & Terminal Reporter (`src/test_runner.rs`, `shae test`)**
  - [x] Recursive test discovery across `tests/shae/` and workspace `*_test.shae` files.
  - [x] Name pattern filtering via `-f` / `--filter <pattern>`.
  - [x] Dual-engine execution across Bytecode VM (default) and AST Evaluator (`--ast`).
  - [x] Standard output capture buffering with `--nocapture` real-time passthrough.
  - [x] Colored ANSI reporter showing test durations in milliseconds, failure summaries with captured output, and proper exit codes (0 for pass, 1 for failure).
- [x] **Live Documentation Server (`shae doc --serve`, `src/doc.rs`)**
  - [x] Live static HTTP development server integration via `--serve` (supporting `--port <PORT>` and `--host <HOST>`).
  - [x] Instant local HTTP browsing of generated interactive documentation sites with non-blocking graceful shutdown support.
- [x] **Dual-Engine Integration Tests & Pure-Shae Suite**
  - [x] Pure-Shae sample test suite `tests/shae/sample_suite_test.shae` exercising the full `std:test` API.
  - [x] Comprehensive Rust integration test suite `tests/shae_test_runner_test.rs` validating test discovery, dual-engine runs, filtering, failure diffs, and live doc server.

---

### **Chunk 26: Authenticated Symmetric Cryptography & Key Derivation (`std:crypto`)** *(Completed)*
- [x] **Authenticated Encryption with Associated Data (AEAD)**
  - [x] AES-256-GCM (`encrypt_aes_gcm`, `decrypt_aes_gcm`): 128-bit authentication tags, optional AAD, flexible string/[u8] inputs, auto-generated cryptographically secure 12-byte random nonce, and structured map outputs (`ciphertext`, `nonce`, `tag`, `combined`).
  - [x] ChaCha20-Poly1305 (`encrypt_chacha20`, `decrypt_chacha20`): 256-bit stream cipher with Poly1305 authenticator, flexible input types, auto-generated nonces, and structured map outputs.
  - [x] Authentication tag verification and rejection of tampered ciphertext, tags, or mismatched AAD.
- [x] **Key Derivation Functions (KDF)**
  - [x] PBKDF2 (`pbkdf2_sha256`, `pbkdf2_sha512`): Password-based key derivation with configurable salt, iteration counts, and output length.
  - [x] HKDF (`hkdf_sha256`, `hkdf_sha512`): HMAC-based Extract-and-Expand Key Derivation with optional salt, input key material (IKM), optional context info, and configurable output length.
- [x] **Constant-Time Verification**
  - [x] Constant-time equality checking (`timing_safe_equal`): Leak-free verification against timing-channel attacks.
- [x] **Dual-Engine & Integration Test Verification**
  - [x] Pure-Shae test suite `tests/shae/crypto_suite_test.shae` exercising full crypto suite across VM and AST Evaluator.
  - [x] Rust integration test suite `tests/crypto_advanced_test.rs` validating 8 comprehensive test vectors across both engines.

---

### **Chunk 27: Stage 2 Self-Hosting Two-Stage Bootstrap & Bit-for-Bit Parity** *(Completed)*
- [x] **Self-Hosting Compiler Fixes & Parity Hardening**
  - [x] Added missing `<` (`TOKEN_LT`) and `>` (`TOKEN_GT`) lexical token generation to `lib/compiler/lexer.shae`.
  - [x] Fixed expression-statement assignment parser precedence conflict in `lib/compiler/parser.shae` allowing assignments (`=`, `+=`, `-=`) to properly emit `AssignStmt`.
  - [x] Added statement-level `OP_POP` cleanup for `VariableExpr` assignments in `lib/compiler/codegen.shae` ensuring stack consistency.
  - [x] Hardened chunk constant pool deduplication (`add_constant`) to prevent constant pool overflow.
  - [x] Added robust path resolution fallback for compiler submodules when running standalone `.shaec` bytecode files.
  - [x] Implemented CLI driver entrypoint `run_cli_driver()` in `lib/compiler/main.shae` for standalone compilation, disassembly, and linking.
- [x] **Two-Stage Bootstrap Pipeline (`Stage 0 -> Stage 1 -> Stage 2`)**
  - [x] Stage 0 compiler links self-hosting compiler binary: `shae link lib/compiler/main.shae -o target/shaec`.
  - [x] Stage 0 compiler compiles `lib/compiler/main.shae` to produce Stage 1 bytecode (`target/shaec_stage2.shaec`).
  - [x] Stage 1 compiler executes via Shae VM to compile `lib/compiler/main.shae` to produce Stage 2 bytecode (`target/shaec_stage3.shaec`).
  - [x] Fixed-point convergence: Byte-for-byte identical verification between Stage 1 and Stage 2 bytecode (`cmp target/shaec_stage2.shaec target/shaec_stage3.shaec`).
- [x] **Bit-for-Bit User Target Parity**
  - [x] Verified compiling user programs with Stage 0 and Stage 1 produces identical byte sequences.
- [x] **Dual-Engine & Integration Test Verification**
  - [x] Pure-Shae test suite `tests/shae/self_stage2_bootstrap_test.shae` exercising two-stage compilation, deterministic self-compilation, driver dispatch, and ELF linking.
  - [x] Rust integration test suite `tests/stage2_bootstrap_test.rs` proving two-stage bootstrap pipeline, bit-for-bit parity, and dual-engine VM/AST parity.

---

### **Chunk 28: Direct LLVM SSA Code Generation & Native Compilation (`shae llvm`)** *(Completed)*
- [x] **Direct LLVM 18 SSA IR Generator (`src/llvm.rs`)**
  - [x] Standard `alloca`/`load`/`store` SSA lowering pattern in `entry:` block (promoted to hardware registers via LLVM `mem2reg`).
  - [x] Complete arithmetic, bitwise, comparison, logical, and floating-point SSA instruction emission (`add`, `sub`, `mul`, `sdiv`, `srem`, `and`, `or`, `xor`, `shl`, `ashr`, `fadd`, `fsub`, `fmul`, `fdiv`, `icmp`, `fcmp`, `select`).
  - [x] Structured control flow lowering: `while` loops, `if`/`else` branches, merge blocks, phi-free alloca state storage, and unconditional branches.
  - [x] Function declarations, recursive definitions, multi-parameter calling conventions, and `@__shae_user_main` wrapping.
  - [x] Global variables emission with typed storage (`@var = internal global i64 0, align 8`).
  - [x] String pool constants deduplication with proper hex escape sequences (`\0A`, `\00`, etc.).
  - [x] External C standard library declarations (`printf`, `puts`, `malloc`, `free`, `exit`, `strlen`, `strcpy`, `strcat`, `strcmp`) and variadic printf lowering for `print`/`println`.
  - [x] Native compilation helper `LlvmCompiler::compile_and_link` targeting `x86_64-pc-linux-gnu` with Clang `-O2`.
- [x] **CLI Subcommand Integration (`src/main.rs`)**
  - [x] `shae llvm <file.shae>`: Direct LLVM IR generator with `-o <out.ll>` emission.
  - [x] `-c` / `--compile`: Full native compilation to standalone executable via system `clang -O2`.
  - [x] `-r` / `--run`: Compile and immediately execute the native binary with argument and exit code propagation.
- [x] **Self-Hosting Compiler Suite LLVM Backend (`lib/compiler/llvm.shae`, `lib/compiler/main.shae`)**
  - [x] Pure-Shae LLVM SSA code generator emitting valid LLVM 18 IR with module headers, format strings, string pooling, functions, and `@main` wrapper.
  - [x] Driver dispatch support in `lib/compiler/main.shae` for `"llvm"` action.
- [x] **Dual-Engine & Integration Test Verification**
  - [x] Pure-Shae test suites `tests/shae/llvm_codegen_test.shae` and `tests/shae/self_llvm_test.shae` exercising pure-Shae LLVM IR emission, recursion, loops, floats, strings, booleans, sample target compilation, and driver dispatch across VM and AST Evaluator.
  - [x] Rust integration test suite `tests/llvm_codegen_test.rs` validating arithmetic SSA IR, control flow, native Clang compilation and execution, float/string printing, and CLI subcommand flags.

---

### **Chunk 29: Zero-Copy Typed Memory Buffers & Struct Layouts (`std:buffer`)** *(Completed)*
- [x] **Zero-Copy Typed Memory Buffer (`src/buffer.rs`, `std:buffer`)**
  - [x] `ByteBuffer` container encapsulating `Arc<RwLock<Vec<u8>>>`, `offset: usize`, and `length: usize`.
  - [x] Zero-copy slicing: `buf.slice(start, [end])` sharing underlying storage without memory copies; mutations in slices reflect immediately in parent buffers and vice versa.
  - [x] Constructors & conversions: `alloc(size)`, `from(bytes)`, `from_hex(hex)`, `from_utf8(text)`, `concat(buffers)`, `.copy()`, `.to_hex()`, `.to_utf8()`, `.to_bytes()`.
  - [x] Bounds safety & direct byte indexing: `buf[i]` read/write with bounds checking and `0..=255` byte range validation.
- [x] **Endian-Aware Binary Numeric I/O**
  - [x] 8, 16, 32, and 64-bit signed and unsigned integer reads/writes (`u8`, `i8`, `u16_le`, `u16_be`, `i16_le`, `i16_be`, `u32_le`, `u32_be`, `i32_le`, `i32_be`, `u64_le`, `u64_be`, `i64_le`, `i64_be`).
  - [x] 32-bit and 64-bit IEEE 754 floating-point reads/writes (`f32_le`, `f32_be`, `f64_le`, `f64_be`) with camelCase aliases (`readU32LE`, `writeU32BE`, etc.).
- [x] **High-Performance Struct Packing & Unpacking Engine**
  - [x] Format specifier parser supporting endian prefixes (`<`, `>`, `!`, `=`, `@`), repetition counts, and field types (`x`, `c`, `b`, `B`, `?`, `h`, `H`, `i`, `I`, `l`, `L`, `q`, `Q`, `f`, `d`, `s`, `p`).
  - [x] `calcsize(format)`: Packed struct layout byte length computation (e.g. `<I2h4s` = 12 bytes, `>2d4B` = 20 bytes).
  - [x] `pack(format, ...values)`: Serializing variadic or array arguments into packed byte buffers.
  - [x] `pack_into(format, buffer, offset, ...values)`: Direct serialization into existing buffer at offset.
  - [x] `unpack(format, buffer_or_bytes, [offset])`: Deserializing binary layouts into Shae values.
- [x] **Dual-Engine Parity & Integration Test Verification**
  - [x] Identical execution semantics across Stack-Based Bytecode VM and Tree-Walking AST Evaluator.
  - [x] Pure-Shae test suite `tests/shae/buffer_suite_test.shae` validating allocation, zero-copy mutations, endian I/O, and struct pack/unpack.
  - [x] Rust integration test suite `tests/buffer_test.rs` validating 9 comprehensive test vectors.

---

### **Chunk 30: Hot-Code Reloading for Distributed Cluster Nodes & Long-Running Daemons (`std:actor:cluster` & `std:actor`)** *(Completed)*
- [x] **Local Actor Daemon Hot-Code Reloading (`src/actor.rs`, `std:actor`)**
  - [x] Dynamic handler hot-swapping via `ActorMessage::Reload` without terminating actor worker threads or dropping active daemon state.
  - [x] FIFO mailbox queue continuity: Unconsumed messages are safely retained and processed by the newly loaded handler.
  - [x] State schema migrations: Optional migration function (`migration_fn(&mut state)`) transforms active state before subsequent message handling.
  - [x] Failure resilience: If state migration encounters runtime errors, existing handler and state are safely retained.
- [x] **Distributed Cluster Node Hot-Code Reloading (`src/actor_cluster.rs`, `std:actor:cluster`)**
  - [x] `ClusterWireMsg::Reload` wire protocol transmitting handler code or base64 bytecode closures (`BYTECODE:<arity>:<base64>`) over TCP/TLS mesh connections.
  - [x] Remote reload routing: `remote_actor.reload(new_handler, [migration_fn])` and `node.reload_actor(name, new_handler, [migration_fn])`.
  - [x] Zero connection drops: Running cluster daemons upgrade code remotely while keeping mesh TCP/TLS sessions established.
- [x] **Dual-Engine Parity & Integration Test Verification**
  - [x] Full parity across Tree-Walking Evaluator and Bytecode VM.
  - [x] Pure-Shae test suite `tests/shae/actor_reload_test.shae` covering local actor reload, state migration, mailbox FIFO continuity, and distributed cluster mesh reload.
  - [x] Rust integration test suite `tests/actor_reload_test.rs` covering 6 comprehensive test scenarios.

---

### **Chunk 31: Benchmark Suite, Showcase Demonstrations, & Documentation (`shae bench`)** *(Completed)*
- [x] **CLI Benchmark Suite Engine (`src/bench.rs`, `shae bench`)**
  - [x] `shae bench [filter]`: Subcommand with automatic benchmark discovery across `benchmarks/*.shae`.
  - [x] Options: `-f, --filter <pattern>`, `--ast` / `--engine=ast`, `--vm` / `--engine=vm`, `-i, --iter <N>`.
  - [x] Formatted ASCII table reporting benchmark name, iterations, total execution time, average latency per iteration, and throughput (ops/sec).
  - [x] `std:bench` standard library module exporting `bench(name, [iterations], callable)` with warmup passes and precise latency measurements.
- [x] **Realistic Compute & System-Intensive Benchmark Suites (`benchmarks/`)**
  - [x] `benchmarks/fibonacci.shae`: Recursive and iterative arithmetic benchmark with sequence sum workloads.
  - [x] `benchmarks/buffer_throughput.shae`: Stressing `std:buffer` allocation, zero-copy slicing, endian binary I/O, and struct packing/unpacking throughput.
  - [x] `benchmarks/actor_throughput.shae`: Stressing `std:actor` request-reply (ask), tell/send batching, state mutation, and reload throughput.
  - [x] `benchmarks/collections_bench.shae`: Array insertions, map lookups, functional filter/map closures, and in-place sorting throughput.
- [x] **Comprehensive Language Showcase Demonstrations (`examples/`)**
  - [x] `examples/buffer_demo.shae`: Complete showcase of Chunk 29 `std:buffer` features (ByteBuffer allocation, zero-copy memory slicing reflection, hex/utf8 conversions, endian numeric reads/writes, struct layout pack/unpack `<I2h4s` and `>2d4B`).
  - [x] `examples/actor_hot_reload_demo.shae`: Complete showcase of Chunk 30 hot-reloading features (local stateful actor reload with daemon state preservation, schema migration functions, and distributed cluster actor reload across nodes).
- [x] **Documentation Updates & Live Documentation Server**
  - [x] Specification documentation updated in `LANGUAGE.md` for `std:buffer`, `std:actor` hot-reloading, and `std:bench`.
  - [x] Project roadmap updated in `ROADMAP.md` covering Chunks 29, 30, and 31.
  - [x] Live static HTTP documentation server verified via `shae doc --serve` on loopback.
- [x] **Integration Test Verification**
  - [x] Rust integration test suite `tests/bench_test.rs` validating benchmark discovery, execution timing, filtering, dual-engine parity, documentation generation, and live HTTP doc server serving.
