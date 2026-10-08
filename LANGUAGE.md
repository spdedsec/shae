# The Shae Programming Language Specification

> **"The programming language that respects your sanity."**

---

## 1. Manifesto: Why Shae?

Modern programming often forces developers to choose between extremes:
* **Over-engineering**: Requiring dozens of configuration files, bundlers, and transpilers just to start a service.
* **Cognitive overload**: Borrow checker marathons for simple prototypes, or endless `if err != nil` boilerplate.
* **Fragility**: Loose type coercion and unexpected runtime null dereferences.

**Shae is designed with a clear mission:**  
Be strict, predictable, expressive, and productive. Zero ceremony. Built-in HTTP/TLS servers. Concurrency channels. Standard cryptographic, regex, and encoding libraries included out of the box.

---

## 2. Type System & Primitives

Shae is dynamically typed with strong runtime safety and structural verification:

### Numbers
Shae supports distinct 64-bit signed integers and 64-bit IEEE 754 floats:
- **Integers (`i64`)**: `42`, `-10`, `1_000_000`
- **Floats (`f64`)**: `3.14159`, `-0.5`, `1.0e10`
- **Radix Literals**:
  - Hexadecimal: `0xFF`, `0x1A_2B`
  - Binary: `0b1010_0101`
  - Octal: `0o755`
- **Bitwise Operations**: `&` (AND), `|` (OR), `^` (XOR), `~` (NOT), `<<` (shift left), `>>` (shift right).

### Strings
- **Standard Strings**: `"Hello, World!\n"`
- **Template Interpolation**: `"Hello, ${user.name}!"`
- **Raw Strings**: `r"C:\Windows\System32\drivers\etc\hosts"` or `r"^\w+@\w+\.\w+$"` without escape character processing.
- **Built-in String Methods**:
  - `.len()`, `.trim()`, `.to_upper()`, `.to_lower()`
  - `.starts_with(prefix)`, `.ends_with(suffix)`, `.contains(sub)`
  - `.pad_start(target_len, pad_char)`
  - `.lines()`, `.chars()`, `.slice(start, end)`

### Collections
- **Arrays**: `let list = [1, 2, 3, "four"]`
  - Negative indexing: `list[-1]` returns the last element.
  - Methods: `.find(pred)`, `.some(pred)`, `.every(pred)`, `.flat()`, `.join(sep)`, `.reverse()`, `.slice(start, end)`.
- **Maps**: `let user = { name: "Satya", role: "Developer" }`
  - Safe navigation: `user?.settings?.theme ?? "dark"`

### Structs and Enums
```shae
struct User { id, name, email }
let u = User { id: 1, name: "Alice", email: "alice@example.com" }

enum Result { Ok(value), Err(message) }
let res = Result.Ok(200)
```

---

## 3. Bindings & Destructuring

```shae
// Variable declarations
let x = 10
let mut counter = 0
counter += 1

// Array Destructuring
let [head, next, ..tail] = [1, 2, 3, 4, 5]

// Struct and Map Destructuring
let { name, email, ..extra } = u
```

---

## 4. Pattern Matching

Shae features advanced Rust-inspired `match` expressions with arm guards and ranges:

```shae
let age = 25

let category = match age {
    0..13 => "Child",
    13..20 => "Teenager",
    20..65 if age < 30 => "Young Adult",
    20..65 => "Adult",
    _ => "Senior"
}

// Matching Enum Variants
match res {
    Result.Ok(val) => println("Success:", val),
    Result.Err(msg) => println("Failed:", msg)
}
```

---

## 5. Functions & Pipelines

Functions support implicit returns, closures, and the Elixir-style pipe operator (`|>`):

```shae
fn double(x) {
    x * 2 // implicit return of last expression
}

// Pipelines
let result = [1, 2, 3, 4, 5]
    |> filter(fn(x) { x % 2 == 0 })
    |> map(fn(x) { x * 10 })

println(result) // [20, 40]
```

---

## 6. Concurrency & Message Channels

Shae provides thread-level asynchronous task execution and message-passing channels:

```shae
let queue = channel(10) // Bounded channel with capacity 10

spawn(fn() {
    for i in 1..=5 {
        queue.send("Job #" + str(i))
    }
    queue.close()
})

let item = queue.recv()
while item != null {
    println("Processing:", item)
    item = queue.recv()
}
```

---

## 7. Web & Networking

Shae includes a high-performance native HTTP/HTTPS server engine with keep-alive pooling:

```shae
// Dynamic Routing and Handler
fn app(req) {
    let match_user = route_match("/users/:id", req.path)
    if match_user != null {
        return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: json_stringify({ user_id: match_user.params["id"] })
        }
    }
    return { status: 404, body: "Not Found" }
}

// HTTP Server
serve(8080, app)

// HTTPS / TLS Server
serve_tls({
    port: 8443,
    cert: "cert.pem",
    key: "key.pem"
}, app)
```

---

## 8. Standard Library Namespaces

Import standard modules using `use { ... } from "std:<namespace>"`:

### `std:crypto`
* `sha256(data) -> string`: Hex-encoded SHA-256 digest.
* `sha512(data) -> string`: Hex-encoded SHA-512 digest.
* `hmac_sha256(key, data) -> string`: Hex-encoded HMAC-SHA256 signature.
* `hmac_sha512(key, data) -> string`: Hex-encoded HMAC-SHA512 signature.
* `uuid_v4() -> string`: RFC 4122 v4 random UUID.
* `random_bytes(len) -> array`: Cryptographically secure random byte array.

### `std:codec`
* `b64_encode(str) -> string` / `b64_decode(str) -> string`
* `b64_url_encode(str) -> string` / `b64_url_decode(str) -> string`
* `url_encode(str) -> string` / `url_decode(str) -> string`
* `hex_encode(str) -> string` / `hex_decode(str) -> string`

### `std:regex`
* `is_match(pattern, text) -> bool`
* `find(pattern, text) -> map | null`
* `find_all(pattern, text) -> array`
* `replace(pattern, text, rep) -> string`
* `replace_all(pattern, text, rep) -> string`
* `split(pattern, text) -> array`
* `captures(pattern, text) -> map | null`

### `std:fs`
* `read_file(path) -> string`
* `write_file(path, contents)`
* `append_file(path, contents)`
* `file_exists(path) -> bool`
* `remove_file(path)`

### `std:path`
* `join(...parts) -> string`
* `basename(path) -> string`
* `dirname(path) -> string`
* `ext(path) -> string`
* `is_absolute(path) -> bool`

### `std:sys`
* `args() -> array`
* `env(var_name) -> string | null`
* `exit(code)`
* `platform() -> string`
* `arch() -> string`

### `std:time`
* `now_secs() -> float`
* `now_ms() -> int`
* `sleep_ms(ms)`

---

## 9. Modules & Project System

### Relative File Imports
```shae
use { calculate, Config } from "./math.shae"
use { * } from "./helpers.shae"
```

### Package Management (`shae.toml`)
Initialize and manage packages with `shae pkg`:
```toml
[package]
name = "my_app"
version = "0.1.0"
entry = "src/main.shae"

[dependencies]
utils = { path = "../utils" }
network = { git = "https://github.com/example/network.git", branch = "main" }
```

Import third-party packages in Shae code:
```shae
use { request } from "network"
```
