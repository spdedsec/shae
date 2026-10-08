# Shae

<div align="center">

```
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
```

**A modern, strict, and ergonomic programming language built in Rust.**  
*Zero ceremony, built-in servers with TLS, channels, package manager, and standalone binary bundler.*

[![Tests](https://img.shields.io/badge/tests-82%20passing-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/built%20with-Rust%202024-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![VS Code](https://img.shields.io/badge/VS%20Code-Extension-blue.svg)](editors/vscode)

</div>

---

## ✨ Why Shae?

Shae is designed to hit the sweet spot between safety, ergonomics, and raw utility:

- 🚫 **Zero Ceremony & No Semicolons**: Clean, expressive syntax without unnecessary punctuation or parentheses around conditions.
- ⚡ **True Concurrency & Channels**: Asynchronous thread spawning (`spawn()`) and message-passing channels (`channel(capacity)`).
- 🔒 **Built-in HTTP & TLS Engine**: Spin up production HTTP or HTTPS servers in seconds with `serve()` and `serve_tls()`, featuring keep-alive connection pooling and route matching.
- 🛡️ **Predictable Primitives**: Distinct integers (`i64`) and floats (`f64`), radix literals (`0x`, `0b`, `0o`), and full bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`).
- 🧩 **Pattern Matching & Destructuring**: Match arm guards, range patterns (`1..=10`), and array/struct destructuring with rest patterns.
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

### 1. HTTP & HTTPS Servers with Route Matching
```shae
use { sha256 } from "std:crypto"

fn handle(req) {
    let matched = route_match("/api/users/:id", req.path)
    if matched != null {
        let user_id = matched.params["id"]
        return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: json_stringify({ id: user_id, hash: sha256(user_id) })
        }
    }
    return { status: 404, body: "Not Found" }
}

print("Listening on http://localhost:8080...")
serve(8080, handle)
```

For TLS / HTTPS with automatic certificate loading:
```shae
serve_tls({
    port: 8443,
    cert: "cert.pem",
    key: "key.pem"
}, handle)
```

### 2. Thread-Safe Concurrency Channels
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

### 3. Expanded Standard Library
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
use { is_match, captures, replace_all } from "std:regex"
let valid_email = is_match(r"^[\w\.-]+@[\w\.-]+\.\w+$", "user@shae.dev")
let cleaned = replace_all(r"\s+", "multiple   spaces", " ")
```

### 4. Pattern Matching & Destructuring
```shae
// Destructuring
let [first, second, ..rest] = [10, 20, 30, 40, 50]
let { name, role, ..extra } = { name: "Alice", role: "Admin", active: true }

// Guards and Ranges in Match
let score = 88
let grade = match score {
    90..=100 => "A",
    80..90 if score >= 85 => "B+",
    80..90 => "B",
    _ => "Keep practicing!"
}
```

---

## 🛠️ Developer Tooling CLI

Shae ships with an all-in-one developer toolchain:

| Command | Description |
|---|---|
| `shae run <file.shae>` | Run a script with the Shae interpreter / VM |
| `shae fmt <file.shae>` | Format source files canonically (4-space indentation) |
| `shae fmt --check <file>` | Verify formatting without modifying files |
| `shae check <file.shae>` | Static linter checking for unused variables, dead code, and shadowing |
| `shae pkg init [name]` | Initialize a new package with `shae.toml` manifest |
| `shae pkg add <dep>` | Add a dependency (path or Git repository) |
| `shae pkg install` | Resolve and install dependencies into `.shae/packages` |
| `shae bundle <file> -o <bin>` | Bundle an entire app and its dependencies into a standalone executable |
| `shae lsp` | Run the Language Server Protocol daemon for editor integration |
| `shae test` | Run internal Shae test suites |
| `shae repl` | Start interactive REPL |

---

## 🎨 VS Code Extension

Shae provides an official VS Code extension located at [`editors/vscode`](editors/vscode):
- **TextMate Syntax Highlighting**: Full grammar for raw strings, radix numbers, and keywords.
- **Language Server Protocol Integration**: Real-time compiler diagnostics, completions, and formatting powered by `shae lsp`.
- **Install**:
  ```bash
  code --install-extension editors/vscode/shae-vscode-0.1.0.vsix
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

## 🧪 Running Tests

Shae contains 82 comprehensive integration tests:
```bash
cargo test
cargo check --tests
```

---

## 📄 License

Distributed under the [MIT License](LICENSE). Copyright (c) 2026 Satya Prakash (spdedsec).
