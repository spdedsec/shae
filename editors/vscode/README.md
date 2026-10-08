# Shae Language Support for VS Code

Official Visual Studio Code extension for the **Shae** programming language (`.shae`).

## Features

- 🎨 **Syntax Highlighting**: Full TextMate grammar supporting keywords, raw strings (`r"..."`), radix literals (`0x`, `0b`, `0o`), pattern matching, control flow, and operators.
- ⚡ **Language Server Protocol (LSP)**: Powered directly by the built-in `shae lsp` daemon.
  - **Live Diagnostics**: Real-time syntax errors and static analysis linter warnings (unused variables, unreachable code, variable shadowing).
  - **IntelliSense & Autocompletion**: Auto-complete keywords, standard library modules (`std:crypto`, `std:regex`, `std:codec`, `std:fs`, etc.), builtins, and in-file symbols.
  - **Hover Documentation**: Detailed documentation for keywords and built-in functions.
  - **Document Formatting**: Canonical AST formatting integrated with `shae fmt`.
- 🚀 **Run Active File**: Command to execute the current `.shae` file in the integrated terminal.

## Requirements

The extension communicates with the `shae` compiler via `shae lsp`. Ensure `shae` is installed and available in your `PATH`, or set `shae.lsp.path` in your VS Code settings.

## Configuration

| Setting | Type | Default | Description |
|---|---|---|---|
| `shae.lsp.path` | `string` | `"shae"` | Path to the Shae binary. |
| `shae.lsp.trace.server` | `string` | `"off"` | Traces communication between VS Code and the LSP server (`off`, `messages`, `verbose`). |

## Commands

- `Shae: Restart Language Server` (`shae.restartServer`)
- `Shae: Run Active File` (`shae.runFile`)

## Building and Packaging

```bash
cd editors/vscode
npm install
npm run compile
npx vsce package
```
