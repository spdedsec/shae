# Shae

<div align="center">

```
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
```

**A modern, strict, and ergonomic systems scripting language with Cranelift JIT compilation.**  
*Zero ceremony, native machine code JIT, embedded SQLite, RFC 4180 CSV, coroutines, TLS/mTLS cluster mesh, pure-Shae test runner, live doc server, package registry, and standalone binary bundler.*

[![Version](https://img.shields.io/badge/version-0.4.0-blue.svg)](Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-280%2B%20passing-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/built%20with-Rust%202024-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![VS Code](https://img.shields.io/badge/VS%20Code-v0.4.0-purple.svg)](editors/vscode)

</div>

---

## ✨ Why Shae?

Shae is designed to hit the sweet spot between safety, ergonomics, raw utility, and high execution speed:

- 🚫 **Zero Ceremony & No Semicolons**: Clean, expressive syntax without unnecessary punctuation or parentheses around conditions.
- ⚡ **Cranelift JIT SSA Backend**: Native machine code generation (x86_64, aarch64) for hot mathematical loops, recursive functions, `f64` float SSA math, and direct fast-path array indexing with safe deoptimization bailouts.
- ⏳ **First-Class Coroutines (`async` / `await` & `std:async`)**: Cooperative asynchronous routines, non-blocking evaluation, and concurrent combinators (`async:all`, `async:race`, `async:sleep`).
- 🔐 **Cluster Transport Security: TLS & Mutual-TLS (`std:actor:cluster`)**: Production-grade encrypted distributed actor mesh with x509 certificate validation, mutual authentication, and cryptographic actor messaging.
- 🧪 **Pure-Shae Test Runner (`shae test` & `std:test`)**: Zero-boilerplate unit and integration testing engine with rich assertions (`assert_eq`, `assert_throws`), pattern filtering (`-f`), output capture, and dual-engine execution.
- 📚 **Standard Documentation & Live Server (`shae doc --serve`)**: Automated doc parser (`///`, `//!`) exporting static HTML, Markdown, and JSON models, with embedded instant-reloading HTTP preview server.
- 📦 **Native Package Registry & Publishing (`shae pkg`)**: Manifest-driven dependencies (`shae.toml`), deterministic lockfiles (`shae.lock`), `.shaepkg` archive packaging, SHA-256 verification, and registry publishing.
- 🔒 **Industrial Hash-DoS Hardening**: Cryptographic SipHash-2-4 keyed hashing with randomized per-process seeds to neutralize algorithmic complexity attacks on maps and first-class sets.
- 🔤 **UAX #29 Unicode & Rune Safety (`std:unicode`)**: True unicode extended grapheme clustering (`.graphemes()`, `.grapheme_len()`, `.slice_graphemes()`, `.runes()`, `.codepoints()`), eliminating multi-byte slice truncation panics on complex emojis and combining characters.
- 🚦 **Cross-Platform OS Signal Handling (`std:sys`)**: Trap OS interrupts (`SIGINT`, `SIGTERM`), register callbacks, and cleanly drain server pools and active connections.
- 🧮 **High-Performance Collections (`std:collections`)**: First-class `Set` collection with O(N) deduplication, union, intersection, and set difference.
- 🗄️ **Embedded SQLite Database (`std:sqlite`)**: Production-ready SQL storage out of the box with zero-dependency runtime dynamic linking across Linux, macOS, and Windows.
- 📊 **RFC 4180 CSV Engine (`std:csv`)**: High-throughput tabular data parsing and serialization with quoted multiline strings and escape handling.
- 🌐 **Modern HTTP Client & Server**:
  - Client (`std:http`): Ergonomic `get`, `post_json`, query parameter encoding, and automatic response decoding.
  - Server (`serve`, `serve_tls`): Multi-threaded HTTP/1.1 and TLS servers with keep-alive connection pooling and route pattern matching.
- 🎭 **Actor-Based Concurrency & Supervision (`std:actor`)**: Isolated actors with asynchronous unbounded FIFO mailboxes, synchronous request-response (`ask`), named registry resolution, and fault-tolerant supervision trees (`OneForOne`, `AllForOne`) with automatic restart thresholds.
- 🧵 **True Concurrency & Channels**: Asynchronous thread spawning (`spawn()`) and message-passing channels (`channel(capacity)`).
- 🛡️ **Predictable Primitives**: Distinct integers (`i64`) and floats (`f64`), radix literals (`0x`, `0b`, `0o`), and full bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`).
- 🧩 **Pattern Matching & Destructuring**: Match arm guards, range patterns (`1..=10`), and array/struct destructuring with rest patterns.
- 🔌 **Dynamic C FFI (`std:ffi`)**: Bind and invoke native shared libraries (`.so`, `.dylib`, `.dll`) with typed signatures directly from Shae code.
- 🚀 **Single-Binary App Bundler (`shae bundle`)**: Package your entire Shae codebase and all its dependencies into a self-extracting, standalone executable.
- 🛠️ **First-Class Developer Tooling**:
  - `shae fmt`: Automated AST code formatting.
  - `shae check`: Static analysis and linter.
  - `shae test`: Built-in pure-Shae test runner with discovery and colored reports.
  - `shae doc`: Documentation generator and live development server.
  - `shae lsp`: Full Language Server Protocol daemon with live diagnostics, autocomplete, and hover documentation.
  - **VS Code Extension**: Official syntax highlighting and LSP integration.

---

## 🚀 Installation & Quick Start

### 1. Single-Line Universal Installer (macOS, Linux, Windows)
Install the official pre-built standalone toolchain binary with a single command:
```bash
curl -fsSL https://shae-lang.org/install.sh | bash
```
*(Or via GitHub: `curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash`)*

### 2. Build and Install from Source
Ensure you have Rust and Cargo installed:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo install --path .
```

Verify your installation:
```bash
shae --version
```

### 3. Hello World in 10 Seconds
Create `hello.shae`:
```shae
let name = "Developer"
println("Hello, ${name}! Welcome to Shae.")
```
Run it:
```bash
shae run hello.shae
```

---

## 🌟 Language Tour

### 1. Cranelift JIT Compilation
Shae features an integrated SSA-based Just-In-Time compiler powered by Cranelift:
```shae
// Explicitly compile a recursive algorithm to native machine code
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

jit("compile", fib)
println("fib(32) =", fib(32)) // Executes at near-C native speed
```
Hot functions executed over the threshold are also automatically tiered and compiled by the runtime.

### 2. Embedded SQLite (`std:sqlite`)
Connect, query, and mutate SQLite databases without external C compilers:
```shae
let sqlite = use "std:sqlite"

let db = sqlite.open("app.db")
sqlite.execute(db, "CREATE TABLE IF NOT EXISTS metrics (id INTEGER PRIMARY KEY, service TEXT, latency REAL)")
sqlite.execute(db, "INSERT INTO metrics (service, latency) VALUES (?, ?)", ["auth-api", 18.4])

let rows = sqlite.query(db, "SELECT service, latency FROM metrics WHERE latency > ?", [10.0])
for row in rows {
    println("Service:", row["service"], "Latency:", row["latency"])
}
sqlite.close(db)
```

### 3. RFC 4180 CSV Processing (`std:csv`)
```shae
let csv = use "std:csv"

let raw = "id,name,role\n1,\"Doe, Jane\",\"Engineering Lead\"\n2,Bob,Developer"
let table = csv.parse(raw)

// Modify and serialize back to compliant CSV
table.push(["3", "Alice", "Security"])
let output_csv = csv.stringify(table)
```

### 4. HTTP Client & Servers
```shae
// HTTP Client
let http = use "std:http"
let res = http.get("https://httpbin.org/get", {
    query: { "env": "prod", "limit": 10 }
})
if res.ok {
    println("Response status:", res.status)
    println("Parsed JSON:", res.json["args"])
}

// Built-in HTTP Server with Route Matching
fn handle(req) {
    let matched = route_match("/api/users/:id", req.path)
    if matched != null {
        let user_id = matched.params["id"]
        return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: json_stringify({ id: user_id, active: true })
        }
    }
    return { status: 404, body: "Not Found" }
}

println("Listening on http://localhost:8080...")
serve(8080, handle)
```

### 5. Thread-Safe Concurrency Channels
```shae
let ch = channel(5)

spawn(fn() {
    for i in 1..=5 {
        ch.send("Message #" + str(i))
    }
    ch.close()
})

let msg = ch.recv()
while msg != null {
    println("Received:", msg)
    msg = ch.recv()
}
```

### 6. First-Class Coroutines (`async` / `await` & `std:async`)
Shae provides native cooperative coroutines and concurrency combinators:
```shae
let async = use "std:async"

async fn fetch_task(id, delay_ms) {
    async:sleep(delay_ms)
    return "Task #" + str(id) + " finished"
}

async fn run_pipeline() {
    let t1 = fetch_task(1, 30)
    let t2 = fetch_task(2, 50)
    
    // Concurrent execution combinator
    let results = await async:all([t1, t2])
    println("Batch complete:", results)
}

await run_pipeline()
```

### 7. Actor Concurrency & Fault-Tolerant Supervision (`std:actor`)
Shae provides Erlang/OTP-inspired isolated actors with zero shared mutable state, deep-cloned message envelopes, unbounded FIFO mailboxes, request-response synchronization, and hierarchical supervision trees:

```shae
let actor = use "std:actor"

// Stateful actor maintaining encapsulated state
let counter = actor.spawn(0, fn(count, msg) {
    if msg == "inc" {
        return { "state": count + 1, "reply": count + 1 }
    } else if msg == "get" {
        return { "state": count, "reply": count }
    }
    return count
})

// Synchronous request-response (ask) pattern
let val = counter.ask("inc") // 1
println("Counter:", counter.ask("get")) // 1

// Global actor registry and polymorphic sending
actor.register("global_counter", counter)
send("global_counter", "inc")

// Fault-tolerant supervision tree
let sup = actor.supervisor({
    "strategy": "one_for_one",
    "max_restarts": 3,
    "within_seconds": 5.0,
    "children": [
        {
            "id": "worker",
            "state": 100,
            "start": fn(st, msg) {
                if msg == "fail" { return 1 / 0 } // Automatic restart
                return { "state": st, "reply": "ok" }
            }
        }
    ]
})

let worker = sup.get_child("worker")
println("Supervised worker status:", worker.ask("ping"))

// Graceful teardown
counter.stop()
sup.stop()
```

#### `std:actor` Module API Reference
| Function / Method | Signature | Description |
|---|---|---|
| `actor.spawn(handler)` | `fn(handler: fn(msg))` | Spawns a stateless actor with unary message handler |
| `actor.spawn(initial_state, handler)` | `fn(state, handler: fn(state, msg))` | Spawns a stateful actor with state-transition handler |
| `actor.spawn_named(name, [state], handler)` | `fn(name: str, ...)` | Spawns an actor and binds it to the global registry |
| `actor.send(target, msg)` / `pid.send(msg)` | `fn(target, msg)` | Asynchronous non-blocking message dispatch |
| `actor.ask(target, msg, [timeout_ms])` / `pid.ask(msg, [timeout_ms])` | `fn(target, msg, [timeout])` | Synchronous request-response with optional timeout |
| `actor.stop(pid)` / `pid.stop()` | `fn(pid)` | Cleanly shuts down actor and terminates thread |
| `actor.is_alive(pid)` / `pid.is_alive()` | `fn(pid) -> bool` | Checks if actor is active and running |
| `actor.mailbox_len(pid)` / `pid.len()` | `fn(pid) -> int` | Returns current count of queued messages |
| `actor.self_pid()` / `actor.self()` | `fn() -> ActorRef` | Retrieves the calling actor's own handle |
| `actor.register(name, pid)` | `fn(name: str, pid)` | Registers global alias for actor |
| `actor.whereis(name)` | `fn(name: str) -> ActorRef` | Resolves global alias to actor handle |
| `actor.unregister(name)` | `fn(name: str)` | Unbinds global alias from registry |
| `actor.registered()` | `fn() -> [str]` | Lists all currently registered global actor names |
| `actor.supervisor(config)` | `fn(config: map) -> SupervisorRef` | Creates supervision tree (`OneForOne` / `AllForOne`) |
| `sup.get_child(id)` / `sup.children()` | `fn(id)` / `fn()` | Retrieves child actor handle or lists child IDs |
| `sup.start_child(spec)` / `sup.stop_child(id)` | `fn(spec)` / `fn(id)` | Dynamically starts or stops supervised children |
| `sup.restart_child(id)` / `sup.stop()` | `fn(id)` / `fn()` | Restarts specific child or stops supervisor tree |

### 8. Distributed Actor Mesh with TLS/mTLS (`std:actor:cluster`)
Securely network multiple nodes into an encrypted actor cluster with mutual TLS verification:
```shae
let cluster = use "std:actor:cluster"

// Spin up a cluster node with TLS / Mutual TLS encryption
let node = cluster.ClusterNode("node-alpha", "127.0.0.1", 9001, "cluster-auth-cookie")
node.set_mtls("certs/ca.crt", "certs/node.crt", "certs/node.key")
node.start()

// Connect to peer and invoke remote actor transparently
node.connect_peer("127.0.0.1", 9002)
let remote_worker = node.resolve_remote("worker", "node-beta")
let response = remote_worker.ask("compute_workload")
println("Cluster response:", response)
```

### 9. Built-in Pure-Shae Test Runner (`std:test` & `shae test`)
Write zero-boilerplate tests directly in Shae using the native assertion framework:
```shae
use { test, assert, assert_eq, assert_ne, assert_throws } from "std:test"

test("basic arithmetic operations", fn() {
    assert_eq(2 + 2, 4, "math must be deterministic")
    assert_ne(10, 20)
    assert(5 > 0, "5 is positive")
})

test("exception assertions", fn() {
    assert_throws(fn() {
        let x = 1 / 0
    }, "division by zero must throw")
})
```
Execute your test suites with rich ANSI terminal reporting:
```bash
# Run all tests in tests/shae/ or workspace
shae test

# Filter tests by pattern and display stdout live
shae test tests/shae/ --filter arithmetic --nocapture

# Run against AST evaluator
shae test --ast
```

### 10. Live Documentation Engine (`shae doc --serve`)
Shae extracts triple-slash (`///`) item doc comments and module comments (`//!`), compiling them into interactive searchable sites:
```shae
//! String transformation and sanitization utilities.

/// Truncates string to length with trailing ellipsis.
fn truncate(text, max_len) {
    if text.len() <= max_len { return text }
    return text.slice(0, max_len) + "..."
}
```
Generate and preview documentation locally:
```bash
# Export static HTML documentation site into doc/
shae doc

# Launch embedded live HTTP documentation server with instant preview
shae doc --serve --port 8080 --open
```

### 11. Package Registry & Publishing (`shae pkg`)
Package and publish libraries with SHA-256 cryptographic integrity:
```bash
# Initialize a new package manifest (shae.toml)
shae pkg init my_library

# Bundle project into a compressed .shaepkg archive
shae pkg pack -o dist/my_library-0.1.0.shaepkg

# Cryptographically verify archive checksum
shae pkg verify dist/my_library-0.1.0.shaepkg <sha256>

# Publish to package registry (supports local filesystem or remote API)
shae pkg publish --registry https://pkg.shae-lang.org/api/v1 --token $SHAE_REGISTRY_TOKEN
```

### 12. Expanded Standard Library
```shae
// Cryptography
use { sha256, hmac_sha256, uuid_v4, random_bytes } from "std:crypto"
let token_id = uuid_v4()
let secret = random_bytes(32)
let signature = hmac_sha256("my_secret_key", "payload_data")

// Encoding & Codecs
use { b64_encode, b64_decode, url_encode } from "std:codec"
let encoded = b64_encode("Hello World")
let url = "https://example.com/search?q=" + url_encode("shae lang")

// Regular Expressions
use { is_match, replace_all } from "std:regex"
let valid_email = is_match(r"^[\w\.-]+@[\w\.-]+\.\w+$", "user@shae.dev")
let cleaned = replace_all(r"\s+", " ", "multiple   spaces")

// Collections & Sets (Hash-DoS Hardened)
let collections = use "std:collections"
let unique = collections.Set(["apple", "banana", "apple", "grape"])
unique.add("mango")
println("Has banana?", unique.has("banana"))
let other = collections.Set(["mango", "kiwi", "orange"])
let combined = unique.union(other)

// Unicode & UAX #29 Grapheme Clustering
let unicode = use "std:unicode"
let text = "Hi 👨‍👩‍👧‍👦, welcome to 🇺🇸!"
println("Graphemes count:", text.grapheme_len()) // Correctly counts 👨‍👩‍👧‍👦 and 🇺🇸 as 1 cluster
println("Flag slice:", text.slice_graphemes(17, 18))

// OS Signals & Graceful Teardown
let sys = use "std:sys"
sys.on_signal("SIGINT", fn(sig) {
    println("Caught interrupt signal:", sig, "- shutting down gracefully")
})

// Environment & Platform
let env = use "std:env"
env.load_dotenv(".env")
println("Running on:", sys.platform(), sys.arch())
```

---

## 📂 Standalone Examples & Showcase

Explore ready-to-run examples in the [`examples/`](examples) directory:

- 🎭 [**Actor Concurrency & Supervision Demo**](examples/actor_demo.shae): Stateful actors, request-reply `ask`, named registry, and `OneForOne` supervisor crash recovery.
- 🌐 [**Distributed Actor Mesh Demo**](examples/actor_cluster_demo.shae): TCP clustering, cross-node discovery, remote actor `ask`, distributed worker pools, and bully leader election.
- 🧪 [**Pure-Shae Test Runner Suite**](tests/shae/sample_suite_test.shae): Pure-Shae unit and integration tests using `std:test` assertion primitives.
- 🚀 [**Production Microservice Showcase**](examples/realworld_service/): Complete real-world HTTP microservice integrating SQLite telemetry persistence and CSV bulk import/export.
- 🔤 [**Unicode & Grapheme Demo**](examples/unicode_demo.shae): Extended grapheme clusters, emoji sequences, and rune slicing.
- 🚦 [**OS Signals Demo**](examples/signals_demo.shae): Cross-platform interrupt handling and graceful server teardown.
- 🧮 [**Collections & Set Demo**](examples/collections_demo.shae): SipHash-2-4 backed Set operations and algebraic set theory.
- 🗄️ [**SQLite Demo**](examples/sqlite_demo.shae): In-memory database initialization, parameter binding, CRUD, and aggregations.
- 📊 [**CSV Demo**](examples/csv_demo.shae): Parsing RFC 4180 multiline records, table mutations, and serialization.
- 🌐 [**HTTP Client Demo**](examples/http_demo.shae): GET with query params, POST with automatic JSON serialization.
- ⚡ [**Cranelift JIT Demo**](examples/jit_demo.shae): Explicit AOT JIT compilation, native float math, and fast array indexing.
- 🔢 [**Fibonacci Demo**](examples/fibonacci.shae): Clean procedural recursion and iteration.
- 🌐 [**Web Server Demo**](examples/web_server.shae): Lightweight REST routing and responses.

---

## 🛠️ Developer Tooling CLI

Shae ships with an all-in-one developer toolchain:

| Command | Description |
|---|---|
| `shae run <file.shae\|file.shaec>` | Run a script or precompiled bytecode (defaults to Bytecode VM; use `--engine=ast` for AST) |
| `shae compile <file.shae>` | Compile source script into ahead-of-time bytecode binary (`.shaec`) |
| `shae disasm <file.shae\|file.shaec>` | Disassemble script or compiled bytecode into human-readable instructions and constant pool |
| `shae fmt <file.shae>` | Format source files canonically (4-space indentation) |
| `shae fmt --check <file>` | Verify formatting without modifying files |
| `shae check <file.shae>` | Static linter checking for unused variables, dead code, and shadowing |
| `shae test [path] [-f <pat>] [--ast] [--nocapture]` | Built-in Pure-Shae test runner with discovery, filtering, and sub-ms timing |
| `shae doc [path] [-o <dir>] [--format <fmt>] [--serve] [--port <p>] [--open]` | Generate HTML/Markdown/JSON API documentation with optional live HTTP server |
| `shae pkg init [name]` | Initialize a new package with `shae.toml` manifest |
| `shae pkg add <dep>` | Add a dependency (path or Git repository) |
| `shae pkg install` | Resolve and install dependencies into `.shae/packages` |
| `shae pkg pack [dir] [-o <file>]` | Package project into a compressed `.shaepkg` archive with SHA-256 integrity verification |
| `shae pkg publish [dir] [--registry <url>] [--token <tok>] [--dry-run]` | Publish package archive to local filesystem or remote package registry |
| `shae pkg verify <pkg> <sha256>` | Cryptographically verify `.shaepkg` archive integrity checksum |
| `shae bundle <file> [-o <bin>] [--strip] [--no-bytecode]` | Bundle app and its dependencies into a standalone binary with embedded precompiled bytecode |
| `shae link <file.shae> [-o <bin>]` | Link script into a standalone 64-bit ELF or PE native executable |
| `shae wasm <file.shae> [-o <out.wasm>] [--run]` | Compile script into WebAssembly binary bytecode (.wasm) and optionally execute via built-in runner |
| `shae lsp` | Run the Language Server Protocol daemon for editor integration |
| `shae repl` | Start interactive REPL (defaults to Bytecode VM; use `--engine=ast` for AST) |

---

## 🎨 VS Code Extension

Shae provides an official VS Code extension located at [`editors/vscode`](editors/vscode):
- **TextMate Syntax Highlighting**: Full grammar for raw strings, radix numbers, JIT keywords, sets, and standard modules.
- **Language Server Protocol Integration**: Real-time compiler diagnostics, completions, and formatting powered by `shae lsp`.
- **Install**:
  ```bash
  code --install-extension editors/vscode/shae-vscode-0.4.0.vsix
  ```

---

## 📦 Single-Binary Bundler

Compile any Shae project into a zero-dependency standalone binary for production:
```bash
shae bundle src/main.shae -o my_service
./my_service
```
The output executable embeds all imported dependencies and executes directly without requiring the Shae runtime or source files on the target machine.

---

## 🔮 Upcoming Features & Horizon Roadmap

Shae is advancing toward an enterprise-scale systems ecosystem. Recent milestones and future horizon phases:

1. **Phase 4: Actor-Based Concurrency & Distributed Mailboxes** *(Completed in v0.3.1)*:
   - Lightweight isolated actors with unbounded mailboxes and message passing (`std:actor`).
   - Request-response `ask` pattern with configurable timeouts.
   - Global name registration (`ActorRegistry`) and polymorphic message dispatch.
   - Fault-tolerant supervision trees (`OneForOne`, `AllForOne`) with automatic process restart policies.
   - Full dual-engine VM and AST execution parity.
2. **Phase 5: Self-Hosting Shae Compiler & Direct Native Linker** *(Completed in v0.3.3)*:
   - Stage 1 (Complete): Pure-Shae Lexer, AST constructors, and Pratt Parser (`lib/compiler/`) with full self-parsing verification.
   - Stage 2 (Complete): Pure-Shae Bytecode Compiler, Serializer, and Disassembler (`codegen.shae`, `chunk.shae`, `disasm.shae`) emitting `.shaec` bytecode chunks.
   - Stage 3 (Complete): Direct ELF 64-bit and PE executable emission (`lib/compiler/elf.shae`, `lib/compiler/pe.shae`, `lib/compiler/linker.shae`, `shae link`) with zero external compiler dependencies.
3. **Phase 6: WebAssembly & Edge Runtime Target** *(Completed in v0.3.4)*:
   - WebAssembly binary emission (`lib/compiler/wasm.shae`, `shae wasm`).
   - Integrated zero-dependency `WasmRunner` interpreter with full nested call support.
   - Browser execution playground with live instantiation (`examples/wasm_demo/`).
4. **Phase 7: Distributed Actor Mesh (`std:actor:cluster`)** *(Completed in v0.3.5)*:
   - Distributed TCP network clustering with binary length-prefixed framing (`src/actor_cluster.rs`).
   - Node discovery, authentication cookie handshakes, and peer registry synchronization.
   - Remote actor handles (`RemoteActorRef`), asynchronous `send()`, and synchronous `ask()` across nodes.
   - Pure-Shae distributed worker pools and leader election (`lib/actor/cluster.shae`).
5. **Phase 8: Enterprise Release & Systems Toolchain Ecosystem** *(Completed in v0.4.0)*:
   - Official multi-platform distribution CI/CD (Linux glibc/musl, Linux aarch64, macOS arm64/x86_64, Windows x86_64).
   - Single-line universal installer (`install.sh`) with automatic OS/arch detection and SHA-256 integrity verification.
   - Cluster Transport Security: TLS & Mutual-TLS (mTLS) with CA peer verification (`std:actor:cluster`).
   - First-class coroutines (`async` / `await`) and asynchronous concurrency combinators (`std:async`).
   - Standard automated documentation engine (`shae doc`) with static HTML, Markdown, and JSON export.
   - Package registry and publishing pipeline (`shae pkg pack` / `publish`) with `.shaepkg` archives.
6. **Phase 9: Built-In Pure-Shae Test Runner & Live Documentation Server** *(Completed in v0.4.0)*:
   - Pure-Shae test runner CLI `shae test` with test discovery, `-f`/`--filter` matching, and sub-ms timing.
   - Standard test assertion library `std:test` (`test`, `assert`, `assert_eq`, `assert_ne`, `assert_throws`).
   - Dual-engine VM and AST test execution parity.
   - Embedded live HTTP documentation server `shae doc --serve` with instant local browsing.
7. **Phase 10: Horizon & Future Systems Roadmap**:
   - Direct LLVM SSA code generation target for native freestanding binary compilation.
   - Zero-copy typed memory buffers and typed struct layouts.
   - Hot-code reloading for distributed cluster nodes and long-running daemons.

---

## 🧪 Running Tests

Shae contains 280+ comprehensive integration and multi-platform test suites:
```bash
cargo test
cargo check --tests
shae test
```

---

## 📄 License

Distributed under the [MIT License](LICENSE). Copyright (c) 2026 Satya Prakash (spdedsec).
