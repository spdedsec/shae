# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-10-26

### Added
- **Official Multi-Platform Distribution Pipeline**:
  - Automated GitHub Actions CI/CD release workflow building standalone release binaries for:
    - Linux x86_64 glibc (`shae-v0.4.0-linux-x86_64.tar.gz`)
    - Linux x86_64 musl (`shae-v0.4.0-linux-x86_64-musl.tar.gz`)
    - Linux aarch64 (`shae-v0.4.0-linux-aarch64.tar.gz`)
    - macOS Apple Silicon arm64 (`shae-v0.4.0-macos-arm64.tar.gz`)
    - macOS Intel x86_64 (`shae-v0.4.0-macos-x86_64.tar.gz`)
    - Windows x86_64 (`shae-v0.4.0-windows-x86_64.zip`)
  - Cryptographic SHA-256 checksum generation (`SHA256SUMS.txt`) for all release packages.
  - Official VS Code extension packaging (`shae-vscode-0.4.0.vsix`).
- **Single-Line Universal Installer (`install.sh`)**:
  - Universal one-line installation via `curl -fsSL https://shae-lang.org/install.sh | bash`.
  - Automatic OS detection (Linux glibc/musl, macOS Darwin, Windows MSYS/MINGW) and CPU architecture detection (`x86_64`, `arm64`/`aarch64`).
  - Automatic download and cryptographic checksum verification against `SHA256SUMS.txt`.
  - Safe archive extraction, executable permission configuration, and interactive PATH setup guidance.
  - Comprehensive installer automated test suite (`tests/install_sh_test.sh`).
- **Milestone 1: Complete Self-Hosting Bootstrap Pipeline**:
  - Pure-Shae Lexer, AST nodes, and Pratt Parser (`lib/compiler/lexer.shae`, `ast.shae`, `parser.shae`).
  - Pure-Shae Stack Bytecode Compiler & Disassembler (`lib/compiler/codegen.shae`, `chunk.shae`, `disasm.shae`) producing `.shaec` bytecode chunks.
  - Native Direct Linker for 64-bit ELF and PE binaries (`lib/compiler/elf.shae`, `pe.shae`, `linker.shae`, `shae link`).
  - Self-hosting multi-stage bootstrap integration test suite (`tests/self_bootstrap_test.rs`).
- **Milestone 2: Cluster Transport Security (TLS & Mutual-TLS / mTLS)**:
  - Enterprise encrypted node-to-node cluster communication with `rustls` v0.23 (`src/actor_cluster.rs`).
  - Strict mutual TLS authentication with client certificate verification against CA bundles.
  - Dynamic TLS configuration options in `cluster.node({ "tls": { ... } })`.
  - Introspection method `node.is_tls()` / `is_tls(node)` across both Bytecode VM and AST Evaluator.
  - Defensive rejection and termination of untrusted peer handshakes.
  - Dual-engine test verification suites (`tests/shae/actor_cluster_tls_test.shae`, `tests/actor_cluster_tls_test.rs`).
- **Milestone 3: First-Class `async` / `await` Coroutines and `std:async` Runtime**:
  - Thread-safe `TaskInner` container supporting idempotent repeated awaiting without panic (`src/value.rs`).
  - Lifecycle state machine transitions: `"pending"`, `"completed"`, `"failed"`.
  - First-class task methods: `task.join()`, `task.wait()`, `task.is_done()`, `task.status()`, `task.result()`.
  - Concurrency combinators in `std:async`: `async.spawn`, `async.all`, `async.race`, `async.sleep`, `async.timeout`, `async.is_done`, `async.status`.
  - Dual-engine verification suite (`tests/shae/async_coroutine_test.shae`, `tests/async_coroutine_test.rs`).
- **Milestone 4: Standard Documentation Engine (`shae doc`)**:
  - Doc comments parsing: item-level (`///`) and module-level (`//!`) (`src/doc.rs`).
  - Comprehensive AST symbol extraction: functions (`fn`, `async fn`), parameters, structs, enums, variants, and constants.
  - Responsive static HTML documentation generator with dark theme, instant client-side search, sidebar navigation, and syntax badges.
  - Multi-format export: Clean Markdown (`doc.md`) and machine-readable JSON schema (`doc.json`).
  - CLI command `shae doc [path] [-o <DIR>] [--format html|md|json|all] [--open]`.
  - Integration test suite (`tests/doc_gen_test.rs`).
- **Milestone 5: Package Registry & Publishing (`shae pkg pack` / `publish`)**:
  - Deterministic gzip-compressed tarball packaging (`.shaepkg`) omitting temporary and build artifacts (`src/pkg.rs`).
  - Cryptographic SHA-256 integrity hash generation and tamper verification.
  - Path-traversal safe package unpacking (`unpack_package`).
  - Dual-mode registry publishing engine supporting local filesystem (`file://`) and remote HTTP(S) registries with Bearer token authentication.
  - Safe `--dry-run` pre-flight validation.
  - CLI commands `shae pkg pack`, `shae pkg publish`, `shae pkg verify`.
  - Integration test suite (`tests/pkg_publish_test.rs`).

## [0.3.2] - 2026-10-14

### Added
- 1-Click title bar run button and interactive CodeLens entrypoints in VS Code extension.
- Built-in static development server (`shae serve`).
- Bytecode VM web engine with callable reflection for `serve` and `serve_tls`.
- Resilient parser with panic-mode statement boundary synchronization (`parse_with_recovery`).

## [0.3.1] - 2026-10-12

### Added
- Actor-based concurrency and supervision trees (`std:actor`).
- Actor mailbox message passing, asynchronous spawning, and `ask` request-response pattern.
- Supervision strategies (`OneForOne`, `AllForOne`) with automatic restart policies.

## [0.3.0] - 2026-10-10

### Added
- Cranelift JIT SSA backend for native machine code execution.
- Embedded SQLite database module (`std:sqlite`).
- RFC 4180 CSV engine (`std:csv`).
- UAX #29 Unicode grapheme clustering (`std:unicode`).
- Cryptographic SipHash-2-4 hash-DoS protection.

## [0.1.0] - 2026-10-01

### Added
- Initial release of the Shae programming language.
- Tree-walking AST evaluator and stack bytecode VM.
- CLI driver, package manager foundations, and REPL.
