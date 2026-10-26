# Shae v0.4.0 — Official Multi-Platform Release & Systems Ecosystem Overhaul

We are thrilled to announce **Shae v0.4.0**, our largest and most ambitious release to date. This milestone establishes Shae as an industrial-grade systems scripting language, introducing an official multi-platform release pipeline, a single-line universal installer, self-hosting compilation, encrypted distributed cluster actors, first-class coroutines, an automated documentation engine, and a secure package publishing ecosystem.

---

### 🌟 Key Highlights & Major Milestones

#### 1. 🚀 Universal Multi-Platform Distribution & Single-Line Installer
- **Single-Line Universal Installer**: Install Shae instantly across Linux, macOS, and Windows with a single command:
  ```bash
  curl -fsSL https://shae-lang.org/install.sh | bash
  # Or via GitHub:
  curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash
  ```
- **Automated Multi-Platform Release Matrix**:
  - **Linux x86_64**: glibc (`shae-v0.4.0-linux-x86_64.tar.gz`) & musl (`shae-v0.4.0-linux-x86_64-musl.tar.gz`)
  - **Linux aarch64**: (`shae-v0.4.0-linux-aarch64.tar.gz`)
  - **macOS Apple Silicon**: (`shae-v0.4.0-macos-arm64.tar.gz`)
  - **macOS Intel**: (`shae-v0.4.0-macos-x86_64.tar.gz`)
  - **Windows x86_64**: (`shae-v0.4.0-windows-x86_64.zip`)
- **Cryptographic SHA-256 Checksums**: Every release artifact is checksummed in `SHA256SUMS.txt` and verified automatically by `install.sh`.

#### 2. 🛡️ Cluster Transport Security: TLS & Mutual-TLS (mTLS) (`std:actor:cluster`)
- **Enterprise-Grade Encryption**: Fully encrypted actor-to-actor node communication backed by `rustls` v0.23.
- **Mutual-TLS (mTLS)**: Strict bidirectional cryptographic authentication using custom certificate authorities (CAs) and client certificates.
- **Dynamic Configuration**: Configure secure clusters with `cluster.node({ "tls": { "cert": ..., "key": ..., "ca": ..., "verify_peer": true } })`.
- **Introspection API**: Runtime inspection via `node.is_tls()` and `is_tls(node)` across both Bytecode VM and AST Evaluator.
- **Adversarial Resilience**: Automatic rejection and defensive termination of rogue or unverified node connections.

#### 3. 🧵 First-Class Coroutines (`async` / `await`) & `std:async` Runtime
- **Thread-Safe Task Lifecycles**: Non-blocking asynchronous tasks managed via `TaskInner` with states `"pending"`, `"completed"`, and `"failed"`.
- **Idempotent Awaiting**: Safe repeated `.await` / `.join()` without panicking.
- **Rich Task Reflection**: `task.join()`, `task.wait()`, `task.is_done()`, `task.status()`, and `task.result()`.
- **Concurrency Combinators (`std:async`)**:
  - `async.spawn(fn, ...args)`: Spawn background coroutine tasks.
  - `async.all(tasks)`: Aggregate multiple asynchronous operations in parallel.
  - `async.race(tasks)`: Resolve to whichever task completes first.
  - `async.sleep(ms)`: Non-blocking coroutine sleep timer.
  - `async.timeout(ms, task)`: Deadline enforcement and timeout handling.
- **100% Dual-Engine Parity**: Identical asynchronous behavior across both the Tree-Walking AST Evaluator and Bytecode Stack VM.

#### 4. 📚 Standard Documentation Engine (`shae doc`)
- **First-Class Doc Comments**: Item doc comments (`///`) and module doc comments (`//!`).
- **Comprehensive AST Symbol Extractor**: Extracts functions, async functions, structs, enums with variants, and constants with line and signature precision.
- **Responsive HTML Site Generator**: Modern dark-mode static site with client-side real-time search filtering, collapsible sidebar navigation, and type signature badges.
- **Multi-Format Export**: Generate clean GitHub-Flavored Markdown (`doc.md`) and structured JSON schema models (`doc.json`).
- **CLI Subcommand**:
  ```bash
  shae doc . --format all -o ./docs --open
  ```

#### 5. 📦 Package Publishing & Registry Pipeline (`shae pkg pack` / `publish`)
- **Deterministic Package Archives (`.shaepkg`)**: High-efficiency gzip tarballs omitting build artifacts, git repositories, and local caches.
- **Cryptographic Tamper Protection**: SHA-256 integrity hash computation and validation.
- **Path Traversal Security**: Strict directory containment checking in `unpack_package` preventing zip-slip vulnerabilities.
- **Dual-Mode Publishing Engine**: Direct support for local directory/filesystem registries (`file://`) and remote HTTP(S) API registries with Bearer token authentication (`--token` or `SHAE_REGISTRY_TOKEN`).
- **Safe Pre-Flight Validation**: `--dry-run` flag to inspect manifests and archives prior to upload.
- **CLI Commands**: `shae pkg pack`, `shae pkg publish`, `shae pkg verify`.

#### 6. 🔄 Complete Self-Hosting Bootstrap Pipeline (`lib/compiler/main.shae`)
- **Stage 1 (Pure-Shae Lexer & Pratt Parser)**: Self-parsing grammar parser implemented entirely in Shae (`lib/compiler/`).
- **Stage 2 (Bytecode Compiler & Serializer)**: Ahead-of-time bytecode compilation emitting `.shaec` chunks from pure Shae.
- **Stage 3 (Native Linker)**: Direct emission of 64-bit ELF and PE executables without GCC, Clang, or external linkers (`shae link`).
- **Verified 3-Stage Bootstrapping**: Multi-stage self-hosting integration tests passing across all engines.

---

### 📦 Installation

#### Quick Install (macOS / Linux / Windows WSL / MSYS):
```bash
curl -fsSL https://shae-lang.org/install.sh | bash
```

#### Build from Source:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo install --path .
```

Verify your installation:
```bash
shae --version
# Output: shae 0.4.0
```

---

### 🎨 VS Code Extension
Download and install the official VS Code extension from the release assets:
```bash
code --install-extension shae-vscode-0.4.0.vsix
```

---

**Full Commit History & Milestone Tracking**: https://github.com/spdedsec/shae/compare/v0.3.2...v0.4.0
