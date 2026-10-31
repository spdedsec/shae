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

### `std:buffer` (Zero-Copy Typed Memory Buffers & Struct Layouts)
The `std:buffer` module provides low-level, high-performance byte manipulation, zero-copy slicing, endian-aware binary I/O, and structured memory layout serialization.

* **Constructors & Factory Functions**:
  * `alloc(size: Int) -> ByteBuffer`: Allocates a zero-initialized buffer of `size` bytes.
  * `from(bytes: Array<Int>) -> ByteBuffer`: Creates a buffer from an array of byte integers (0..255).
  * `from_hex(hex_str: String) -> ByteBuffer`: Parses an ASCII hexadecimal string into raw bytes.
  * `from_utf8(text: String) -> ByteBuffer`: Encodes a UTF-8 string into raw bytes.
  * `concat(buffers: Array<ByteBuffer>) -> ByteBuffer`: Merges multiple buffers into a contiguous buffer.
* **Instance Properties & Methods**:
  * `buf.len -> Int`: Buffer byte length.
  * `buf.offset -> Int`: Buffer view offset into underlying storage.
  * `buf.slice(start: Int, [end: Int]) -> ByteBuffer`: Returns a zero-copy sub-buffer sharing underlying memory with the parent buffer. Mutations through the slice are immediately visible to the parent and vice versa.
  * `buf.copy() -> ByteBuffer`: Deep-copies the buffer view into a new, isolated buffer.
  * `buf[index]`: Reads a byte at `index` (bounds checked).
  * `buf[index] = value`: Writes a byte at `index` (validates `0 <= value <= 255`).
  * `buf.to_hex() -> String`: Converts buffer contents to an ASCII hex string.
  * `buf.to_utf8() -> String`: Decodes buffer contents as UTF-8 string.
  * `buf.to_bytes() -> Array<Int>`: Converts buffer to an array of integers.
* **Endian-Aware Numeric I/O**:
  * Reads/writes for signed/unsigned integers: `read_u8`, `read_i8`, `read_u16_le`, `read_u16_be`, `read_i16_le`, `read_i16_be`, `read_u32_le`, `read_u32_be`, `read_i32_le`, `read_i32_be`, `read_u64_le`, `read_u64_be`, `read_i64_le`, `read_i64_be` and write counterparts.
  * Reads/writes for floats: `read_f32_le`, `read_f32_be`, `read_f64_le`, `read_f64_be` and write counterparts (with camelCase aliases).
* **Struct Packing Engine**:
  * `calcsize(format: String) -> Int`: Computes packed byte length for the format string (e.g. `calcsize("<I2h4s") == 12`, `calcsize(">2d4B") == 20`).
  * `pack(format: String, ...values) -> ByteBuffer`: Packs values into a newly allocated buffer.
  * `pack_into(format: String, buffer: ByteBuffer, offset: Int, ...values) -> Int`: Packs values directly into an existing buffer at `offset`.
  * `unpack(format: String, buffer: ByteBuffer | Array | String, [offset: Int]) -> Array<Value>`: Deserializes binary layouts into Shae values.

### `std:actor` & `std:actor:cluster` Hot-Code Reloading
Hot-code reloading enables zero-downtime upgrades for long-running stateful daemons and distributed cluster meshes.

* **Local Actor Reload**:
  * `actor.reload(new_handler: Function, [migration_fn: Function]) -> Bool`: Swaps the active message handler in place while preserving actor state and mailbox queue continuity.
  * If `migration_fn` is supplied, transforms existing state before subsequent message handling (`state = migration_fn(state)`).
* **Cluster Mesh Reload**:
  * `remote_actor.reload(new_handler: Function, [migration_fn: Function]) -> Bool`: Transmits handler logic across the TCP/TLS cluster mesh via `ClusterWireMsg::Reload`, dynamically upgrading the remote actor without closing network connections or dropping cluster topology.
  * `node.reload_actor(actor_name: String, new_handler: Function, [migration_fn: Function]) -> Bool`: Reloads a registered actor on the node.

### `std:bench` & Benchmark Suite (`shae bench`)
Built-in benchmarking runner and reporting framework for performance profiling and regression detection.

* **CLI Command**:
  * `shae bench [filter] [options]`: Discovers and executes benchmark scripts in `benchmarks/*.shae`.
  * Options: `-f, --filter <PATTERN>`, `--ast`, `--vm`, `-i, --iter <N>`.
  * Formatted table output displays benchmark name, iterations, total execution time, average latency per iteration, throughput (ops/sec), and summary metrics.
* **`std:bench` Standard Module**:
  * `bench(name: String, [iterations: Int], callable: Function)`: Registers and runs a benchmark workload with warmup passes and precise latency measurements.

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
