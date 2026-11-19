# Shae 0.5.0 Evolution Masterplan: The Creative & Systems Language

> **Vision**: Transform Shae from a purely backend/CLI language into an expressive, zero-friction creative development environment—surpassing Python and Java in simplicity for games, visual applications, and interactive software, while retaining Rust-level memory safety and Cranelift JIT speed.

---

## 1. Executive Summary & Gap Analysis

### Why Developers Love Python & Java for Creative Projects:
1. **Python**: Batteries-included mindset (`turtle`, `tkinter`, `pygame`, `random`). A student can write a visual game in 40 lines of clean code.
2. **Java**: Built-in GUI and graphics pipeline (`java.awt.*`, `javax.swing.*`) shipped directly in the standard runtime. Zero build-tool hassle.
3. **Lua / LÖVE2D**: Pure, minimal game loops (`update()`, `draw()`), instant startup, and zero boilerplate.

### What Was Missing in Shae (Until Now):
- **No Windowing or 2D Canvas**: Impossible to open a desktop GUI window without complex manual C FFI bindings.
- **No Native `std:random`**: Games and generative art need high-quality PRNG (`random.int(min, max)`, `random.choice(list)`, `random.float()`).
- **No Sub-Millisecond Time Control**: Game loops need `time.sleep_ms(ms)`, `time.now_ms()`, and frame rate limiting (`fps_cap`).
- **String Method Gaps**: Developers frequently need `.split()`, `.trim()`, `.replace()`, `.to_upper()`, `.to_lower()`.

---

## 2. The 5 Parallel Engineering Streams

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│                   SHAE NEXT-GEN ARCHITECTURE (5 STREAMS)                         │
├────────────────────────────────┬─────────────────────────────────────────────────┤
│ Stream 1: Native 2D GUI Engine │ • `std:gui` built-in module via minifb          │
│           (src/gui.rs)         │ • Hardware-accelerated Wayland/X11/macOS/Win32  │
│                                │ • 60+ FPS Software Framebuffer & 2D Primitives  │
│                                │ • Built-in 8x8 Bitmap Font (zero external TTF)  │
│                                │ • Non-blocking keyboard & mouse event polling   │
├────────────────────────────────┼─────────────────────────────────────────────────┤
│ Stream 2: Game Standard Lib    │ • `std:random` (PCG/Xorshift fast PRNG)         │
│           (src/stdlib.rs)      │ • `random.int(a, b)`, `random.choice(arr)`      │
│                                │ • `std:time` high-res monotonic clock & sleep   │
├────────────────────────────────┼─────────────────────────────────────────────────┤
│ Stream 3: String & Ergonomics  │ • Native methods: `.split()`, `.trim()`,        │
│           (src/eval.rs, vm.rs) │   `.replace()`, `.to_upper()`, `.to_lower()`    │
├────────────────────────────────┼─────────────────────────────────────────────────┤
│ Stream 4: Playable GUI Games   │ • `snake.shae`: Full GUI Snake Desktop Game     │
│           (examples/ & root)   │ • `particles.shae`: Generative 60 FPS Fireworks │
├────────────────────────────────┼─────────────────────────────────────────────────┤
│ Stream 5: Verification & Gate  │ • 100% `cargo test` pass rate (0 regressions)   │
│                                │ • Automated headless CI test mode for GUI       │
│                                │ • System binary updated in ~/.cargo/bin/shae    │
└────────────────────────────────┴─────────────────────────────────────────────────┘
```

---

## 3. Specification: `std:gui` API Design

The `std:gui` standard library module provides an immediate-mode 2D drawing canvas and window manager:

```shae
use "std:gui" as gui
use "std:random" as rand

// 1. Create native window
let win = gui.window("Shae Canvas", 640, 480)

// 2. Main game loop
while gui.is_open(win) {
    // Clear background
    gui.clear(win, 0x181825)

    // Draw primitives
    gui.rect(win, 100, 100, 50, 50, 0xA6E3A1)      // Filled rectangle
    gui.rect_outline(win, 100, 100, 50, 50, 0xF38BA8, 2)
    gui.circle(win, 300, 240, 40, 0x89B4FA)       // Filled circle
    gui.line(win, 0, 0, 640, 480, 0x585B70)        // Bresenham line
    gui.text(win, 20, 20, "SCORE: 100", 0xF9E2AF, 2) // Bitmap text (scale 2)

    // Input events
    if gui.is_key_down(win, "Up") {
        println("Up arrow pressed")
    }
    let mouse = gui.mouse_pos(win) // [x, y] or null

    // Frame synchronization
    gui.render(win)
    gui.limit_fps(win, 60)
}
```

### Supported Key Strings:
`"Up"`, `"Down"`, `"Left"`, `"Right"`, `"Space"`, `"Enter"`, `"Escape"`, `"W"`, `"A"`, `"S"`, `"D"`, `"R"`, `"P"`, etc.

---

## 4. Specification: `std:random` API Design

Fast, deterministic, seedable PRNG for games and procedural content:
- `rand.int(min, max)`: Uniform integer in `[min, max]`.
- `rand.float()`: Float in `[0.0, 1.0)`.
- `rand.choice(array)`: Random element from an array.
- `rand.shuffle(array)`: In-place or new shuffled array.
- `rand.seed(val)`: Deterministic seed initialization.

---

## 5. Specification: `std:time` High-Resolution Timing

- `time.now_ms()`: Milliseconds since UNIX epoch as integer.
- `time.now_secs()`: Seconds as float with microsecond precision.
- `time.sleep_ms(ms)`: High-precision thread sleep without burning CPU.

---

## 6. Playable Showcase Targets

1. **`snake.shae` (Native Desktop GUI Game)**:
   - Real desktop window with smooth 60 FPS grid rendering.
   - Smooth keyboard controls (Arrow keys or WASD).
   - Random apple generation using `std:random`.
   - Dynamic snake growth, self-collision detection, score tracking, game over, and restart key.
2. **`particles.shae` (Generative Visual Effects)**:
   - 200+ particle explosion and gravity simulation.
   - Velocity, acceleration, fading alpha/color trails.

---

## 7. Execution & Delivery Protocol

1. Add `minifb = "0.29"` to `Cargo.toml`.
2. Implement `src/gui.rs` and register `std:gui` in `src/stdlib.rs`.
3. Implement `src/random.rs` and register `std:random` in `src/stdlib.rs`.
4. Add high-res timing functions to `std:time`.
5. Implement string methods in `src/eval.rs` (`split`, `trim`, `replace`, etc.).
6. Author `snake.shae` and `particles.shae`.
7. Verify all unit and integration tests via `cargo test`.
8. Recompile and install `~/.cargo/bin/shae` and `~/.local/bin/shae`.
