# Shae v0.6.0 — Gradual Type Checker, Package Registry & Space Invaders 2D Showcase

We are thrilled to announce **Shae v0.6.0**, a massive evolutionary leap for the Shae programming language. This landmark release introduces an opt-in **Gradual Static Type System & Diagnostics Checker**, a complete decentralized **Remote Package Registry Subsystem**, the **Shae Studio Web IDE**, and the **Unified Creative Suite** featuring a full retro **Space Invaders 2D** arcade game with synthesized waveform audio.

---

### 🌟 Key Highlights & Major Milestones

#### 1. 🛡️ Gradual Typing & Static Type Checker (`shae check`)
- **Syntax Type Annotations**: Opt-in static typing on functions, parameters, return bounds, and variable declarations:
  ```shae
  fn calculate_tax(subtotal: float, rate: float | null) -> float {
      let effective_rate: float = rate ?? 0.08
      return subtotal * effective_rate
  }
  ```
- **Type Universe**: Primitive types (`int`, `float`, `string`, `bool`), compound types (`list`, `map`), escape hatch (`any`), and nullable sentinel (`null`).
- **Nullable Union Types**: Expressive union syntax (`int | null`, `string | float | any`) with natural null-safety ergonomics.
- **Deep Static Analysis Engine**: `shae check` verifies return type consistency, parameter arity and types, and variable assignments without executing code.
- **LSP Diagnostics Integration**: Full real-time editor diagnostic squiggles and hover tooltips for typed identifiers.
- **100% Backward Compatibility**: Dynamically typed code continues to execute seamlessly without friction.

#### 2. 📦 Remote Package Registry & Manager Subsystem (`shae pkg`)
- **Comprehensive CLI Suite**:
  - `shae pkg init <name>`: Scaffolds a standardized package repository with `shae.toml`.
  - `shae pkg add <pkg>@<version>`: Downloads dependencies from local or remote registries and records them in `shae.toml`.
  - `shae pkg install`: Deterministically resolves and installs all dependencies from `shae.lock`.
  - `shae pkg pack`: Packages libraries into portable, compressed `.shaepkg` archives.
  - `shae pkg publish`: Securely authenticates and uploads packages to remote HTTP registries.
  - `shae pkg verify`: Validates package integrity, structure, and metadata checksums.
- **Cryptographic Security**: Every package is validated with SHA-256 integrity checksums, and the extractor enforces Zip-Slip path traversal defense.

#### 3. 👾 Space Invaders 2D Showcase & Unified Creative Engine
- **`examples/space_invaders.shae`**: Full retro 2D arcade game running natively in pure Shae:
  - 5x11 animated alien armada with stepped pacing and descent.
  - Player laser cannon with collision detection and bunker degradation.
  - Real-time score tracking, wave escalation, and lives HUD.
- **`std:gui` Multi-Mode Graphics**:
  - 24-bit TrueColor ANSI half-block terminal canvas rendering at 60 FPS.
  - Embedded desktop windowing fallback (`minifb`) with hardware pixel buffers.
  - Sprite blitting, color fills, and non-blocking keyboard input polling (`is_key_down`).
- **`std:audio` Hardware Sound Synthesis**:
  - Pure waveform synthesis engine: sine, square, triangle, sawtooth, and white noise.
  - Retro sound triggers: `shoot`, `invader_killed`, `explosion`, and `step`.
  - Master volume controls, ADSR envelope generation, and headless audio event tracing.
- **`std:ui` Arcade UI Toolkit**:
  - Arcade HUD components (`draw_hud`, `draw_score`, `draw_lives_icons`, `draw_panel`).
  - Immediate-mode UI widgets: buttons, sliders, checkboxes, and progress bars.

#### 4. 💻 Shae Studio Web IDE (`shae ide` / `shae studio`)
- **Zero-Dependency Browser IDE**: Run `shae ide --port 8080` to launch an interactive development environment directly from the Shae binary.
- **Integrated Toolchain**: Code editor with syntax formatting (`/api/fmt`), static type analysis (`/api/check`), sub-millisecond execution runner (`/api/run`), interactive REPL console, and workspace file explorer.

#### 5. 📖 Comprehensive Genesis Guide (`SHAE_FROM_SCRATCH.md`)
- A master architectural manifesto detailing the journey from sand and silicon to the Pratt parser, stack bytecode VM, Cranelift JIT SSA, LLVM 18 IR generation, and distributed actor clusters.

---

### 📦 Installation & Verification

#### Single-Line Universal Installer (Linux, macOS, Windows)
```bash
curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash
# Or via official site:
curl -fsSL https://shae-site.vercel.app/install.sh | bash
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
# Expected: shae 0.6.0
```
