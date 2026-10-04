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
*Fast, expressive, and crash-proof general-purpose language with built-in HTTP servers and zero ceremony.*

[![Tests](https://img.shields.io/badge/tests-12%20passed-brightgreen.svg)]()
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
- 🛡️ **Zero Null Crashes**: Missing fields return `null` instead of panicking; fallback with `??`.
- 🔁 **Closures & First-Class Functions**: Lambdas, higher-order functions, and implicit returns.
- 💬 **Friendly Human Compiler**: Clear, witty error messages that guide you rather than lecture you.

---

## 🚀 Quick Start in 60 Seconds

### 1. Installation
Build and install `shae` globally from source using Cargo:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo install --path .
```

### 2. Hello World (`hello.shae`)
```shae
let name = "Satya"
print("Welcome to Shae, " + name + "!")
```
Run it:
```bash
shae hello.shae
```

---

### 3. Spin Up a Real Web Server in 5 Lines! (`server.shae`)
```shae
let visits = 0

fn router(req) {
    visits += 1
    return "<h1>Hello from Shae!</h1><p>Visitor count: " + visits + "</p>"
}

print("Listening on http://localhost:8080...")
serve(8080, router)
```
Run it:
```bash
shae server.shae
```
Open [http://localhost:8080](http://localhost:8080) in your browser!

---

### 4. Interactive REPL
Just type `shae` into your terminal:
```
$ shae

   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
  The programming language that respects your sanity.

Shae v0.1.0 Interactive REPL
Type 'exit' or press Ctrl+D to quit.

shae> let double = fn(x) { x * 2 }
shae> double(21)
=> 42
shae> let user = { name: "Satya" }
shae> user.role ?? "Superuser"
=> "Superuser"
shae> exit
```

---

## 🛠️ Language Features & Cheat Sheet

### Closures & First-Class Functions
```shae
fn make_counter() {
    let count = 0
    return fn() {
        count += 1
        return count
    }
}

let next = make_counter()
print(next()) // 1
print(next()) // 2
```

### Negative Array Indexing & List Operations
```shae
let stack = [10, 20, 30]
print(stack[-1]) // 30 (last item, Python-style!)

push(stack, 40)
let removed = pop(stack) // 40
print("Length:", len(stack))
```

### Safe Navigation & Null Coalescing
```shae
let profile = { name: "Satya" }

// No "Cannot read property of undefined" crashes!
let bio = profile.bio ?? "Default Bio"
let theme = profile?.settings?.theme ?? "dark"
```

---

## 💡 CLI Commands

```bash
# Run a script
shae main.shae
shae run main.shae

# Start REPL
shae
shae repl

# Create a new Shae project
shae new my_awesome_app

# Programming jokes & tips
shae --joke
shae --tip
```

---

## 🧪 Testing

```bash
cargo test
```
Runs the 12 comprehensive unit and integration test suites covering lexer rules, Pratt parser precedence, closures, array mutations, loops, and native web server handling.

---

## 📜 Specification

Read the full language specification and design constitution in [LANGUAGE.md](LANGUAGE.md).

---

## 📄 License

Distributed under the [MIT License](LICENSE). Copyright (c) 2026 Satya Prakash (spdedsec).
