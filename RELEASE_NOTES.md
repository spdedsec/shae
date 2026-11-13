# Shae v0.5.0 — Native 2D GUI, Creative Engine & Direct Scripting Release

We are proud to announce **Shae v0.5.0**, a landmark release transforming Shae into a high-performance creative and graphical programming language. This release introduces an embedded 60 FPS 2D software framebuffer with native Wayland and X11 windowing, new creative standard library modules (`std:gui`, `std:random`, `std:math`, `std:io`), direct script execution with shebang support, language ergonomics, rich LSP documentation, and complete playable showcase games.

---

### 🌟 Key Highlights & Major Milestones

#### 1. 🎮 Native 2D Game & Windowing Engine (`std:gui` / `std:graphics`)
- **60 FPS Software Rasterizer**: Embedded immediate-mode 2D framebuffer rendering natively on Linux Wayland (`wayland-1`) and X11 (`DISPLAY=:0`).
- **Complete Drawing Primitives**:
  - Solid fills and clears: `gui.clear(win, [color])`.
  - Individual pixels: `gui.pixel(win, x, y, color)`.
  - Rectangles: `gui.rect(win, x, y, w, h, color)` and `gui.rect_outline(win, x, y, w, h, color, [thickness])`.
  - Circles: `gui.circle(win, cx, cy, r, color)` and `gui.circle_outline(win, cx, cy, r, color)`.
  - Lines: Bresenham line rasterizer `gui.line(win, x0, y0, x1, y1, color)`.
  - Text: Scalable retro text `gui.text(win, x, y, text, color, [scale])`.
- **Zero-Dependency 8x8 Bitmap Font**: Embedded font table covering ASCII 32 (`' '`) through 126 (`'~'`) enabling instant in-game typography without external asset dependencies.
- **Non-Blocking Input Polling**: Non-blocking keyboard state querying (`gui.is_key_down`, `gui.get_keys`) and mouse coordinates/buttons (`gui.mouse_pos`, `gui.is_mouse_down`).
- **Frame-Rate Capping**: Microsecond loop pacing with `gui.limit_fps(win, 60)` and `gui.sleep(ms)`.
- **Virtual Headless Canvas**: Automatic fallback to an in-memory virtual canvas when executed in headless CI/SSH environments, ensuring test suites and headless simulations never panic.

#### 2. 🎲 Creative Standard Library Suite (`std:random`, `std:math`, `std:io`)
- **`std:random`**: Fast seedable PRNG supporting `int(min, max)`, `float(min, max)`, `bool(prob)`, `choice(arr)`, `shuffle(arr)`, and deterministic `seed(n)`.
- **`std:math`**: Comprehensive mathematical functions including `sqrt`, `abs`, `min`, `max`, `pow`, `floor`, `ceil`, `round`, `sin`, `cos`, `tan`, `atan2`, `clamp`, and constants `PI` and `E`.
- **`std:io`**: Standard stream utilities `print`, `println`, `input([prompt])`, and `flush`.

#### 3. 📜 Direct Script Execution & Shebang Support
- **Direct Invocation**: Run any script immediately with `shae <file.shae>` without requiring `shae run`.
- **Shebang Integration**: Scripts can start with `#!/usr/bin/env shae` and execute directly in bash (`./game.shae`).
- **Script Comments**: First-class support for `#` single-line comments alongside `//`.
- **Desktop Launcher**: Freedesktop `.desktop` runner (`shae-runner.desktop`) and MIME type configuration (`shae.xml`) for single/double-click script execution in Linux file managers.

#### 4. ⚡ Language Ergonomics & VM Optimizations
- **Boolean Negation**: Added `!` operator alias for `not`.
- **Compound Assignment**: Full support for `+=`, `-=`, `*=`, `/=`, and `%=` across both Evaluator and Bytecode VM.
- **Built-in `input([prompt])`**: Read interactive user input directly from standard input.
- **String Method Aliases**: Added `.to_upper()`, `.to_lower()`, `.to_uppercase()`, and `.to_lowercase()` to string primitives.

#### 5. 🛠️ LSP & VS Code Extension Overhaul
- **LSP Auto-Completion & Hover Docs**: Real-time signatures and interactive documentation for `std:gui`, `std:random`, `std:math`, `std:io`, `std:actor`, `std:actor:cluster`, and `std:async`.
- **VS Code Extension v0.5.0**: Updated TextMate grammar for `#` comments, compound assignment operators, and modern syntax tokens.

#### 6. 🕹️ Playable Games & Showcase Demos
- [**`snake.shae`**](examples/snake_gui.shae): 60 FPS retro arcade Snake game with score counter, apple spawning, collision physics, and instant restart (`[R]`).
- [**`particles.shae`**](examples/particles_gui.shae): 60 FPS generative particle physics and fireworks sandbox with gravity, wall/floor bounces, and interactive mouse bursts.
- [**`dungeon.shae`**](examples/dungeon.shae): Pure-Shae terminal RPG with procedural combat, inventory management, and ASCII rendering.

---

### 📦 Installation & Upgrade

#### Single-Line Universal Installer (macOS, Linux, Windows)
```bash
curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash
```

#### Build and Install from Source
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo install --path .
```

Verify your installation:
```bash
shae --version
# Expected: shae 0.5.0
```
