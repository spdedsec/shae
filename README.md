# Shae

<div align="center">

```
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
```

**The programming language that respects your sanity.**  
*Fast, expressive, concurrent, and crash-proof general-purpose language with built-in HTTP servers and zero ceremony.*

[![Tests](https://img.shields.io/badge/tests-passing-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/built%20with-Rust%202024-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

---

## ✨ Why Shae?

Most modern languages force you into an extreme:
* **Python**: Simple and friendly, but slow, burdened by the GIL, and plagued by runtime environment fragmentation.
* **JavaScript**: Ubiquitous, but littered with historical footguns, weird type coercions (`[] + {}`), and endless configuration hell.
* **Rust**: Unmatched performance and safety, but an argumentative borrow checker that turns writing a simple prototype into an intellectual marathon.
* **Go**: Clean and concurrent, but repetitive boilerplate (`if err != nil`) that drains the joy of building.

**Shae is built to be the sweet spot**:
- 🚫 **No Boilerplate Cliches**: No semicolons. No parentheses around `if` or `while`.
- 🌐 **Built-in Web Server**: Spin up a full HTTP server in 5 lines with zero external libraries.
- ⚡ **True Concurrency**: Fire-and-forget OS background threads instantly using the `spawn()` builtin.
- 🛡️ **Graceful Error Handling**: `try/catch` blocks and beautiful stack traces prevent complete server crashes.
- 🧬 **Data Pipelines**: The Elixir-style pipe operator (`|>`) effortlessly chains functions.
- 🧱 **Structs & Enums**: First-class support for strict data shapes and Rust-style `match` pattern matching.
- 💬 **Friendly Human Compiler**: The `shae check` static linter provides clear, witty error messages that guide you rather than lecture you.

---

## 🚀 Quick Start in 60 Seconds

### 1. Installation
Build and install `shae` globally from source using Cargo:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo install --path .
```

### 2. A "Max Level" API Server
```shae
struct User { id, name }
enum Response { Success(data), Error(msg) }

let db = "users.json"

fn background_monitor() {
    print("System OS User: " + env("USER"))
    print("Disk Space: " + exec("df -h /").trim())
}
spawn(background_monitor) // Runs asynchronously on a background thread!

fn api_handler(request) {
    let start = time()
    
    let res = match request.path {
        "/users" => {
            let data = try { read(db) } catch (e) { "[]" }
            Response.Success(json_parse(data) |> map(fn(u) { return u.name }))
        },
        _ => Response.Error("Not Found")
    }
    
    return match res {
        Response.Success(data) => { "status": "ok", "data": data },
        Response.Error(msg) => { "status": "error", "message": msg }
    }
}

print("Listening on http://localhost:8080...")
serve(8080, api_handler)
```

Run it:
```bash
shae run server.shae
```

---

## 🛠️ Language Features & Cheat Sheet

### The Data Pipeline (`|>`)
Pass data cleanly through transformations:
```shae
let users = [
    { name: "Alice", active: true },
    { name: "Bob", active: false }
]

let active_names = users 
    |> filter(fn(u) { return u.active }) 
    |> map(fn(u) { return u.name })

print(active_names) // ["Alice"]
```

### Structs, Enums, and Pattern Matching
```shae
struct Point { x, y }
let p = Point { x: 10, y: 20 }

enum Option { Some(value), None }
let opt = Option.Some(42)

match opt {
    Option.Some(val) => print("Got: " + str(val)),
    Option.None => print("Got nothing!")
}
```

### Concurrency and Standard Library
```shae
// Execute shell commands directly
let files = exec("ls -la")

// Fetch from APIs seamlessly
let github = fetch("https://api.github.com/users/spdedsec")

// File I/O
write("log.txt", "Server started at " + str(time()))

// True Multithreading
fn heavy_task() { /* ... */ }
spawn(heavy_task) 
```

### Try/Catch
```shae
try {
    let raw = read("missing_file.txt")
} catch (err) {
    print("Oops! Failed to load file: " + err)
}
```

---

## 💡 CLI Commands

```bash
# Execute a script
shae run main.shae
shae main.shae

# Static Linter (Check syntax without executing)
shae check main.shae

# Start Interactive REPL
shae

# Programming jokes & tips
shae --joke
```

---

## 🧪 Testing

```bash
cargo test
```
Runs comprehensive unit and integration test suites covering the VM, evaluator, lexer rules, Pratt parser precedence, closures, array mutations, pipelines, stack traces, and native web server handling.

---

## 📜 Specification

Read the full language specification and design constitution in [LANGUAGE.md](LANGUAGE.md).

---

## 📄 License

Distributed under the [MIT License](LICENSE). Copyright (c) 2026 Satya Prakash (spdedsec).
