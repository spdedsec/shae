# The Shae Programming Language Specification

> **"The programming language that respects your sanity."**

---

## 1. Manifesto: Why Shae?

Modern programming has become exhausting.
* **Over-engineering**: You need 20 configuration files, a bundler, a transpiler, a linter, and 50 packages just to start a web server.
* **Cognitive overload**: Rust forces you into a lifelong philosophical debate with the borrow checker just to mutate an array. Go forces you to type `if err != nil` on 40% of all lines. Python is held back by the GIL and environment chaos. JavaScript turns numbers into `[object Object]` and `NaN === NaN` into `false`.
* **Fragility**: One unexpected `null` in a microservice payload crashes the entire application at 3 AM.

**Shae is designed with a singular mission:**  
Be familiar, fast, crash-proof, and productive from the very first minute. Zero ceremony. Built-in web servers. Zero boilerplate.

---

## 2. Core Language Philosophy

1. **No Cliches, Zero Boilerplate**  
   No semicolons required. No parentheses around `if` or `while` conditions. If a syntax element exists merely because C had it in 1972, we threw it away.

2. **Built-in Batteries (Web Servers Included)**  
   Why should building a simple HTTP service require external frameworks? Shae includes a native web engine (`serve(port, handler)`) directly in the standard library.

3. **Safe by Default (No Null Crashes)**  
   Property lookups and indexing never panic. If a field doesn't exist, it evaluates safely to `null`. The null-coalescing operator `??` provides graceful fallback values.

4. **Functions with Implicit Returns**  
   Functions automatically return the result of their last evaluated expression, or an explicit `return`.

5. **Friendly Compiler & Runtime Errors**  
   Shae's diagnostics speak human. If you write `function` or `def`, Shae gently reminds you: *"In Shae, use 'fn' for functions. Less typing, more doing!"*

---

## 3. Syntax & Features

### Variables & Mutability
```shae
let name = "Satya"
let counter = 0

counter += 1
counter -= 1
```

### Functions & Closures
```shae
fn add(a, b) {
    a + b // implicit return!
}

// Higher-order functions & closures
fn make_multiplier(factor) {
    return fn(val) {
        return val * factor
    }
}

let double = make_multiplier(2)
print("Double 5 is:", double(5))
```

### Data Structures: Arrays & Maps
```shae
// Arrays with negative indexing (Python-style!)
let languages = ["Rust", "Python", "Shae"]
print("Last item:", languages[-1]) // "Shae"

push(languages, "Zig")
let popped = pop(languages)

// Maps (Objects)
let user = {
    name: "Satya",
    role: "Architect",
    online: true
}

let user_role = user.role ?? "Guest"
let theme = user?.settings?.theme ?? "dark" // safe navigation!
```

### Control Flow
```shae
// If / Else (No parentheses needed!)
if score >= 90 {
    print("Grade: A")
} else if score >= 80 {
    print("Grade: B")
} else {
    print("Keep practicing!")
}

// While loops with break & continue
let count = 5
while count > 0 {
    print("Countdown:", count)
    count -= 1
}

// For-in loops
for item in ["apple", "banana", "cherry"] {
    print("Fruit:", item)
}
```

### Built-in Native Web Server
```shae
let visits = 0

fn handle(req) {
    visits += 1

    if req.path == "/api" {
        return { status: "ok", visits: visits }
    }

    return "<h1>Hello from Shae HTTP!</h1><p>Visit count: " + visits + "</p>"
}

serve(8080, handle)
```

---

## 4. Built-in Functions Reference

| Function | Signature | Description |
| :--- | :--- | :--- |
| `print(...)` | `print(...args)` | Prints values separated by space to stdout. |
| `dbg(val)` | `dbg(val) -> val` | Debug prints value with type and representation. |
| `len(val)` | `len(array \| map \| string) -> number` | Returns item count or character length. |
| `type(val)` | `type(val) -> string` | Returns `"number"`, `"string"`, `"bool"`, `"array"`, `"map"`, `"function"`, `"null"`. |
| `range(start, end, [step])` | `range(end) \| range(start, end, [step])` | Generates array of numbers. |
| `push(array, val)` | `push(array, val)` | Appends value to array. |
| `pop(array)` | `pop(array) -> val` | Removes and returns last element. |
| `keys(map)` | `keys(map) -> array` | Returns array of map keys. |
| `values(map)` | `values(map) -> array` | Returns array of map values. |
| `json_parse(str)` | `json_parse(str) -> val` | Parses JSON string into Shae value. |
| `json_stringify(val)` | `json_stringify(val) -> string` | Converts value into formatted JSON string. |
| `serve(port, handler)` | `serve(port, handler_fn)` | Starts native HTTP web server. |

---

## 5. Developer CLI

```bash
# Run a Shae script
shae run script.shae
# (Or just shorthand):
shae script.shae

# Start interactive REPL
shae repl
# (Or just run shae with no args):
shae

# Create a new project
shae new my_app

# Get a programming joke or pro-tip
shae --joke
shae --tip
```
