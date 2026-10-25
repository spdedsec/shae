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
*Zero ceremony, native machine code JIT, embedded SQLite, RFC 4180 CSV, built-in TLS servers, channels, package manager, and standalone binary bundler.*

[![Version](https://img.shields.io/badge/version-0.3.1-blue.svg)](Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-255%2B%20passing-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/built%20with-Rust%202024-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![VS Code](https://img.shields.io/badge/VS%20Code-v0.3.1-purple.svg)](editors/vscode)

</div>

---

## ✨ Why Shae?

Shae is designed to hit the sweet spot between safety, ergonomics, raw utility, and high execution speed:

- 🚫 **Zero Ceremony & No Semicolons**: Clean, expressive syntax without unnecessary punctuation or parentheses around conditions.
- ⚡ **Cranelift JIT SSA Backend**: Native machine code generation (x86_64, aarch64) for hot mathematical loops, recursive functions, `f64` float SSA math, and direct fast-path array indexing with safe deoptimization bailouts.
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
- 📦 **Native Package Manager (`shae pkg`)**: Manifest-driven dependencies (`shae.toml`), deterministic lockfiles (`shae.lock`), and Git dependency resolution.
- 🚀 **Single-Binary App Bundler (`shae bundle`)**: Package your entire Shae codebase and all its dependencies into a self-extracting, standalone executable.
- 🛠️ **First-Class Developer Tooling**:
  - `shae fmt`: Automated AST code formatting.
  - `shae check`: Static analysis and linter.
  - `shae lsp`: Full Language Server Protocol daemon with live diagnostics, autocomplete, and hover documentation.
  - **VS Code Extension**: Official syntax highlighting and LSP integration.

---

## 🚀 Installation & Quick Start

### 1. Build and Install from Source
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

### 2. Hello World in 10 Seconds
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

### 6. Actor Concurrency & Fault-Tolerant Supervision (`std:actor`)
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

### 7. Expanded Standard Library
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
| `shae pkg init [name]` | Initialize a new package with `shae.toml` manifest |
| `shae pkg add <dep>` | Add a dependency (path or Git repository) |
| `shae pkg install` | Resolve and install dependencies into `.shae/packages` |
| `shae bundle <file> [-o <bin>] [--strip] [--no-bytecode]` | Bundle app and its dependencies into a standalone binary with embedded precompiled bytecode |
| `shae lsp` | Run the Language Server Protocol daemon for editor integration |
| `shae test [path]` | Run Shae tests (`*_test.shae`) (defaults to Bytecode VM; use `--engine=ast` for AST) |
| `shae repl` | Start interactive REPL (defaults to Bytecode VM; use `--engine=ast` for AST) |

---

## 🎨 VS Code Extension

Shae provides an official VS Code extension located at [`editors/vscode`](editors/vscode):
- **TextMate Syntax Highlighting**: Full grammar for raw strings, radix numbers, JIT keywords, sets, and standard modules.
- **Language Server Protocol Integration**: Real-time compiler diagnostics, completions, and formatting powered by `shae lsp`.
- **Install**:
  ```bash
  code --install-extension editors/vscode/shae-vscode-0.3.1.vsix
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

Shae is advancing toward an enterprise-scale systems ecosystem. Upcoming major phases:

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
3. **Phase 6: WebAssembly & Edge Runtime Target**:
   - First-class compilation to `wasm32-wasi` and browser-embeddable WebAssembly runtimes.
   - High-throughput serverless function cold starts (<1ms).

---

## 🧪 Running Tests

Shae contains 255+ comprehensive integration and multi-platform test suites:
```bash
cargo test
cargo check --tests
```

---

## 📄 License

Distributed under the [MIT License](LICENSE). Copyright (c) 2026 Satya Prakash (spdedsec).

