# Getting Started with the Shae Programming Language 🚀

Welcome to **Shae**! Whether you are writing your very first program or building distributed, high-performance actor clusters, this guide will walk you through running Shae in your favorite editor, IDE, or terminal.

---

## 📋 Table of Contents

1. [Quickstart in 60 Seconds](#-quickstart-in-60-seconds)
2. [Visual Studio Code & Cursor Setup](#-visual-studio-code--cursor-setup-recommended)
3. [JetBrains IDEs Setup (IntelliJ, WebStorm, CLion)](#-jetbrains-ides-setup)
4. [Neovim & Vim Setup](#-neovim--vim-setup)
5. [Terminal & CLI Workflow](#-terminal--cli-workflow)
6. [Interactive REPL](#-interactive-repl)
7. [Compiling Standalone Native Binaries (`shae llvm`)](#-compiling-standalone-native-binaries-shae-llvm)
8. [First Real-World Examples](#-first-real-world-examples)
9. [Troubleshooting & FAQ](#-troubleshooting--faq)

---

## ⚡ Quickstart in 60 Seconds

### 1. Install Shae

#### Option A: Single-Line Universal Installer (Linux & macOS)
```bash
curl -fsSL https://raw.githubusercontent.com/spdedsec/shae/main/install.sh | bash
```

#### Option B: From Source with Cargo (Any Platform)
If you have the Rust toolchain installed:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo build --release
sudo cp target/release/shae /usr/local/bin/
```

### 2. Verify Installation
```bash
shae --version
```
*Output:*
```
Shae Programming Language v0.4.0 (x86_64-unknown-linux-gnu)
```

### 3. Your First Program
Create a file named `hello.shae`:
```shae
let name = "Developer";
println("Hello, " + name + "! Welcome to Shae!");
```

Run it instantly:
```bash
shae run hello.shae
```
*Output:*
```
Hello, Developer! Welcome to Shae!
```

---

## 💻 Visual Studio Code & Cursor Setup (Recommended)

Shae comes with an official Visual Studio Code extension located in [`editors/vscode/`](editors/vscode/) providing full language intelligence.

### Features
* 🎨 **Rich Syntax Highlighting**: Keywords, string interpolations, hexadecimal/binary numbers, pattern matching, closures, pipelines (`|>`), and operators.
* ⚡ **Language Server Protocol (LSP)**: Powered by `shae lsp`:
  * **Real-time Diagnostics**: Red squiggly underlines for syntax errors, unused variables, dead code, and variable shadowing.
  * **IntelliSense & Autocompletion**: Auto-completes keywords, standard library modules (`std:buffer`, `std:actor`, `std:async`, `std:sqlite`, `std:http`, etc.), and local variables.
  * **Hover Information**: Instant type hints and documentation on hover.
  * **Document Formatting**: Canonical formatting on save using `shae fmt`.
* 🚀 **One-Click Run**: Execute the currently open `.shae` file with a single keystroke.

---

### Step 1: Install the Extension

#### Method A: Install the Packaged Extension (`.vsix`)
From your terminal:
```bash
code --install-extension editors/vscode/shae-vscode-0.4.0.vsix
```
*Or in VS Code:*
1. Open the **Extensions** sidebar (`Ctrl+Shift+X` or `Cmd+Shift+X`).
2. Click the `...` menu (Views and More Actions) at the top right of the Extensions panel.
3. Select **Install from VSIX...**.
4. Browse and select `editors/vscode/shae-vscode-0.4.0.vsix`.

#### Method B: Build from Source
```bash
cd editors/vscode
npm install
npm run compile
npx vsce package
code --install-extension shae-vscode-*.vsix
```

---

### Step 2: Configure Settings (Optional)

By default, the extension expects `shae` to be in your system `$PATH`. If `shae` is installed in a custom location, add this to your VS Code `settings.json`:

```json
{
  "shae.lsp.path": "/home/youruser/.shae/bin/shae",
  "[shae]": {
    "editor.formatOnSave": true,
    "editor.tabSize": 4,
    "editor.insertSpaces": true
  }
}
```

---

### Step 3: Run Files Directly Inside VS Code

#### Option A: Command Palette
1. Open any `.shae` file (e.g., `examples/buffer_demo.shae`).
2. Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS).
3. Type and select: **`Shae: Run Active File`**.
4. The integrated terminal will open and run `shae run <file.shae>`.

#### Option B: Configure F5 / Ctrl+F5 One-Click Runner
To run files by pressing `F5` or `Ctrl+F5`, create a `.vscode/tasks.json` in your workspace:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "Run Active Shae File",
      "type": "shell",
      "command": "shae run ${file}",
      "group": {
        "kind": "build",
        "isDefault": true
      },
      "presentation": {
        "reveal": "always",
        "panel": "shared",
        "clear": true
      },
      "problemMatcher": []
    }
  ]
}
```

Now press `Ctrl+Shift+B` (or `Cmd+Shift+B`) to run the active Shae file at any time!

---

## 🛠️ JetBrains IDEs Setup (IntelliJ, WebStorm, CLion, RustRover)

You can run and edit Shae smoothly in any JetBrains IDE using File Type Association and External Tools:

### 1. File Type Recognition
1. Go to **Settings / Preferences** (`Ctrl+Alt+S` or `Cmd+,`).
2. Navigate to **Editor** ➔ **File Types**.
3. Under **Recognized File Types**, click `+` and choose **Language / Text**.
4. Set name to `Shae` and add pattern `*.shae`.
5. Under Keywords, add:
   ```
   let const fn if else while for in return break continue match try catch throw actor import export struct enum pub true false null
   ```

### 2. Configure Run Shortcut (External Tool)
1. Go to **Settings** ➔ **Tools** ➔ **External Tools**.
2. Click `+` to add a new tool:
   * **Name**: `Run with Shae`
   * **Program**: `shae` (or full path, e.g., `/usr/local/bin/shae`)
   * **Arguments**: `run "$FilePath$"`
   * **Working directory**: `$ProjectFileDir$`
3. Go to **Keymap**, search for `Run with Shae`, and assign a shortcut (e.g., `Shift+F10` or `Ctrl+R`).

Now pressing `Shift+F10` executes the open `.shae` file with output displayed in the bottom console!

---

## ⚡ Neovim & Vim Setup

### 1. Filetype Detection
Add the following to your Neovim configuration (`~/.config/nvim/ftdetect/shae.lua`):
```lua
vim.filetype.add({
  extension = {
    shae = 'shae',
  },
})
```

### 2. Configure Built-in LSP (`shae lsp`)
Add Shae to your Neovim LSP setup (`~/.config/nvim/lua/lsp_shae.lua`):
```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = "shae",
  callback = function(ev)
    vim.lsp.start({
      name = "shae-lsp",
      cmd = { "shae", "lsp" },
      root_dir = vim.fs.root(ev.buf, { "shae.toml", ".git" }) or vim.fn.getcwd(),
    })
  end,
})
```

### 3. Quick Run Keymap
Add a keybinding to execute the current file in a floating terminal or buffer:
```lua
vim.keymap.set('n', '<leader>r', ':w<CR>:!shae run %<CR>', { desc = 'Run active Shae file' })
```

---

## 🖥️ Terminal & CLI Workflow

The `shae` CLI binary is a multi-tool designed for fast development workflows:

```bash
# Execute a Shae script (Bytecode VM by default)
shae run script.shae

# Explicitly choose execution engine
shae run --engine=vm script.shae   # Stack Bytecode Virtual Machine (High performance)
shae run --engine=ast script.shae  # Tree-walking Evaluator (Interactive debugging)

# Pretty-print Abstract Syntax Tree (AST)
shae ast script.shae

# Disassemble to human-readable bytecode instructions
shae disasm script.shae

# Format code automatically according to canonical style
shae fmt script.shae
shae fmt --check script.shae       # CI verification

# Run static analysis and linter
shae lint script.shae

# Execute unit and integration tests
shae test
shae test -f "buffer"              # Filter by test name

# Run microbenchmark suites
shae bench
shae bench actor                   # Filter benchmarks

# Generate documentation and launch instant local HTTP preview server
shae doc --serve                   # Starts live server on http://127.0.0.1:8080
```

---

## 🐚 Interactive REPL

Shae includes a full interactive Read-Eval-Print Loop (REPL).

Launch the REPL by running `shae` without arguments:
```bash
shae
```
*Or explicitly:*
```bash
shae repl
```

*Interactive Session Example:*
```shae
Shae REPL v0.4.0
Type ':quit' or press Ctrl+D to exit. Type ':help' for assistance.

shae> let numbers = [1, 2, 3, 4, 5];
shae> let squares = numbers.map(fn(x) { x * x });
shae> println(squares);
[1, 4, 9, 16, 25]

shae> let [head, ..tail] = squares;
shae> head
1
shae> tail
[4, 9, 16, 25]

shae> :quit
Goodbye!
```

---

## ⚙️ Compiling Standalone Native Binaries (`shae llvm`)

Shae features a direct LLVM 18 SSA code generation backend. You can compile Shae source files directly into standalone native machine-code executables without needing the Shae runtime installed on target machines:

```bash
# Compile to native binary using LLVM and Clang -O2 optimizations
shae llvm program.shae -o my_app

# Run the native executable
./my_app
```

The resulting binary:
* Is a standalone 64-bit ELF (Linux), Mach-O (macOS), or PE (Windows) executable.
* Requires zero runtime dependencies.
* Executes with native hardware speed.

---

## 🎯 First Real-World Examples

### 1. Data Pipeline (`examples/pipeline.shae`)
```shae
let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

let result = data
    |> filter(fn(x) { x % 2 == 0 })
    |> map(fn(x) { x * 10 })
    |> reduce(0, fn(acc, x) { acc + x });

println("Pipeline Result: " + result); // 300
```

### 2. State Machine Actor (`examples/actor_demo.shae`)
```shae
import "std:actor";

let initial_state = 0;

let counter = actor.spawn(initial_state, fn(count, msg) {
    match msg {
        "increment" => Result.Ok(count + 1),
        "decrement" => Result.Ok(count - 1),
        "get"       => Result.Ok(count),
        _           => Result.Err("Unknown command"),
    }
});

counter.tell("increment");
counter.tell("increment");
let value = counter.ask("get");
println("Counter state: " + value); // 2
```

### 3. Zero-Copy Typed Memory Buffer (`examples/buffer_demo.shae`)
```shae
import "std:buffer";

// Allocate 32 bytes of raw heap memory
let buf = ByteBuffer.allocate(32);

// Pack binary protocol header: Little-Endian unsigned int, two signed shorts, 4-char magic
let packet = ByteBuffer.pack("<I2h4s", [42, -100, 300, "SHAE"]);

println("Packet byte length: " + packet.len());
let unpacked = ByteBuffer.unpack("<I2h4s", packet);
println("Unpacked values: " + unpacked);
```

---

## 🔍 Troubleshooting & FAQ

### 1. `shae: command not found`
If your terminal cannot find `shae` after installation, ensure the binary directory is in your `$PATH`:
* **Linux/macOS**:
  ```bash
  echo 'export PATH="$HOME/.shae/bin:$PATH"' >> ~/.bashrc
  # Or for zsh:
  echo 'export PATH="$HOME/.shae/bin:$PATH"' >> ~/.zshrc
  source ~/.bashrc
  ```
* **Windows**:
  Add `C:\Users\<YourUser>\.shae\bin` to your User Environment Variables under `Path`.

### 2. VS Code says "Cannot find language server"
Open your VS Code settings (`Ctrl+,` or `Cmd+,`), search for `shae.lsp.path`, and enter the full absolute path to your `shae` binary (e.g., `/usr/local/bin/shae` or `/home/user/.shae/bin/shae`).

### 3. How to check which execution engine is running?
By default, `shae run` uses the fast Stack Bytecode Virtual Machine (`--engine=vm`). To test or compare with the Tree-Walking Evaluator:
```bash
shae run --engine=ast script.shae
```

---

## 📚 Next Steps
* 📕 **[Read the Complete 42-Page Shae Book (PDF)](docs/shae_the_complete_guide.pdf)**: From 5-year-old intuitive metaphors to deep systems architecture.
* 🌐 **Run the Live Documentation Server**: Run `shae doc --serve` to explore the interactive API browser.
* 💡 **Explore the Examples**: Check out the rich set of demonstrations in [`examples/`](examples/).
