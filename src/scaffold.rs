//! Project Scaffolding CLI for Shae (`shae new`)
//!
//! Generates batteries-included, idiomatic project skeletons with `shae.toml`,
//! `.gitignore`, `README.md`, and `src/main.shae` across multiple application templates:
//! - `cli`: Modular command-line tool with argument handling and ANSI coloring.
//! - `game`: 60 FPS arcade loop with `std:gui` canvas, input polling, and physics.
//! - `actor-service`: High-concurrency supervised actor system with message passing.
//! - `web-app`: Lightweight HTTP microservice with JSON handling and routes.

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Cli,
    Game,
    ActorService,
    WebApp,
}

impl Template {
    pub fn as_str(&self) -> &'static str {
        match self {
            Template::Cli => "cli",
            Template::Game => "game",
            Template::ActorService => "actor-service",
            Template::WebApp => "web-app",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Template::Cli => "Command-line tool with argument parsing and ANSI terminal output",
            Template::Game => "Interactive 2D 60 FPS arcade game loop with std:gui",
            Template::ActorService => "Supervised concurrent actor cluster with message passing",
            Template::WebApp => "Lightweight HTTP microservice with routing and JSON support",
        }
    }
}

impl FromStr for Template {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "cli" | "console" | "app" | "default" => Ok(Template::Cli),
            "game" | "arcade" | "gui" => Ok(Template::Game),
            "actor" | "actor-service" | "actors" | "cluster" => Ok(Template::ActorService),
            "web" | "web-app" | "http" | "api" | "server" => Ok(Template::WebApp),
            other => Err(format!(
                "Unknown template '{}'. Available templates: cli, game, actor-service, web-app",
                other
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    pub name: String,
    pub template: Template,
    pub target_dir: Option<PathBuf>,
    pub force: bool,
}

impl Default for ScaffoldOptions {
    fn default() -> Self {
        Self {
            name: "shae_app".to_string(),
            template: Template::Cli,
            target_dir: None,
            force: false,
        }
    }
}

/// Scaffolds a new Shae project on disk.
pub fn scaffold_project(opts: &ScaffoldOptions) -> Result<PathBuf, String> {
    crate::pkg::validate_package_name(&opts.name)
        .map_err(|e| format!("Invalid project name '{}': {}", opts.name, e))?;

    let target_dir = opts
        .target_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(&opts.name));

    if target_dir.exists() {
        let is_empty = fs::read_dir(&target_dir)
            .map(|mut i| i.next().is_none())
            .unwrap_or(false);

        if !is_empty && !opts.force {
            return Err(format!(
                "Target directory '{}' already exists and is not empty. Use --force or specify a different directory.",
                target_dir.display()
            ));
        }
    }

    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create directory '{}': {}", target_dir.display(), e))?;

    let src_dir = target_dir.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| format!("Failed to create 'src' in '{}': {}", target_dir.display(), e))?;

    // 1. shae.toml
    let manifest_content = generate_manifest(&opts.name, opts.template);
    fs::write(target_dir.join("shae.toml"), manifest_content)
        .map_err(|e| format!("Failed to write shae.toml: {}", e))?;

    // 2. .gitignore
    let gitignore_content = ".shae/\n.packages/\ntarget/\n*.log\n*.shaepkg\n.DS_Store\n";
    fs::write(target_dir.join(".gitignore"), gitignore_content)
        .map_err(|e| format!("Failed to write .gitignore: {}", e))?;

    // 3. README.md
    let readme_content = generate_readme(&opts.name, opts.template);
    fs::write(target_dir.join("README.md"), readme_content)
        .map_err(|e| format!("Failed to write README.md: {}", e))?;

    // 4. src/main.shae
    let main_content = generate_main_shae(&opts.name, opts.template);
    fs::write(src_dir.join("main.shae"), main_content)
        .map_err(|e| format!("Failed to write src/main.shae: {}", e))?;

    Ok(target_dir)
}

fn generate_manifest(name: &str, template: Template) -> String {
    format!(
        r#"[package]
name = "{}"
version = "0.1.0"
authors = []
entry = "src/main.shae"
description = "{}"

[dependencies]
"#,
        name,
        template.description()
    )
}

fn generate_readme(name: &str, template: Template) -> String {
    let specific_guide = match template {
        Template::Cli => r#"### CLI Options
- Run with `--help` to inspect supported arguments.
- Pass `--name <name>` to test custom greeting logic."#,
        Template::Game => r#"### 2D Game Controls & Architecture
- Uses `std:gui` TrueColor software rasterizer running at 60 FPS.
- Built-in bouncing orb kinematics and collision detection.
- Press **ESC** or close the window to exit."#,
        Template::ActorService => r#"### Actor Concurrency
- Uses `std:actor` lightweight preemptive actor processes.
- Message passing and state supervision built-in."#,
        Template::WebApp => r#"### HTTP Service
- Includes route dispatching and JSON serialization.
- Test endpoint: `GET /api/status`."#,
    };

    format!(
        r#"# {}

{}

Generated with `shae new {} --template={}`.

## Getting Started

### 1. Run with Shae Interpreter / VM
```bash
shae run src/main.shae
# or simply
shae src/main.shae
```

### 2. Run with Native JIT
```bash
shae jit src/main.shae
```

### 3. Compile to Standalone Native Binary (ELF / Mach-O / PE)
```bash
shae llvm src/main.shae -o {}
./{}
```

### 4. Compile to WebAssembly (.wasm)
```bash
shae wasm src/main.shae -o {}.wasm
```

## Architecture
{}

## License
MIT or Apache-2.0.
"#,
        name,
        template.description(),
        name,
        template.as_str(),
        name,
        name,
        name,
        specific_guide
    )
}

fn generate_main_shae(name: &str, template: Template) -> String {
    match template {
        Template::Cli => format!(
            r#"// {name} - Command-line application in Shae

import "std:sys" as sys

fn print_banner() {{
    print("\x1b[1;36m=== {name} v0.1.0 ===\x1b[0m")
}}

fn print_help() {{
    print_banner()
    print("Usage: shae src/main.shae [options]")
    print("  --help, -h       Show this help message")
    print("  --version, -v    Show version information")
    print("  --name <target>  Target entity to greet")
}}

fn main() {{
    let args = sys.args()
    let target = "World"

    let i = 0
    while i < args.len() {{
        let arg = args[i]
        if arg == "--help" || arg == "-h" {{
            print_help()
            return
        }} else if arg == "--version" || arg == "-v" {{
            print("{name} v0.1.0")
            return
        }} else if arg == "--name" && i + 1 < args.len() {{
            target = args[i + 1]
            i = i + 1
        }}
        i = i + 1
    }}

    print_banner()
    print("✨ Hello, " + target + "! Welcome to your new Shae CLI project.")
}}

main()
"#,
            name = name
        ),
        Template::Game => format!(
            r#"// {name} - 60 FPS 2D Arcade Game in Shae

import "std:gui" as gui

let WIDTH = 320
let HEIGHT = 240
let TITLE = "{name}"

fn main() {{
    let win = gui.init_window(WIDTH, HEIGHT, TITLE)

    let x = 160.0
    let y = 120.0
    let vx = 2.5
    let vy = 1.8
    let radius = 12.0

    print("🎮 Starting " + TITLE + " [Press ESC or Close window to exit]")

    while !gui.should_close(win) {{
        // Update physics
        x = x + vx
        y = y + vy

        if x - radius <= 0.0 || x + radius >= 320.0 {{
            vx = -vx
        }}
        if y - radius <= 0.0 || y + radius >= 240.0 {{
            vy = -vy
        }}

        // Render scene
        gui.clear(win, 0x0F0F1A)
        gui.fill_circle(win, x, y, radius, 0x00E5FF)
        gui.draw_circle(win, x, y, radius + 2.0, 0xFFFFFF)
        gui.draw_text(win, 10, 10, TITLE + " - 60 FPS", 0x88FF88)

        gui.render(win)
    }}

    print("Game session ended.")
}}

main()
"#,
            name = name
        ),
        Template::ActorService => format!(
            r#"// {name} - High-concurrency Actor Service in Shae

import "std:actor" as actor

fn state_manager(state, msg) {{
    let action = msg.get("action", "")
    if action == "increment" {{
        let step = msg.get("step", 1)
        let next_val = state + step
        print("[Actor] State incremented: " + str(next_val))
        return next_val
    }} else if action == "get" {{
        print("[Actor] State requested: " + str(state))
        return state
    }} else if action == "reset" {{
        print("[Actor] State reset to zero")
        return 0
    }}
    return state
}}

fn main() {{
    print("🚀 Spawning {name} actor cluster...")
    let pid = actor.spawn(state_manager, 0)

    actor.send(pid, {{ "action": "increment", "step": 5 }})
    actor.send(pid, {{ "action": "increment", "step": 15 }})
    actor.send(pid, {{ "action": "get" }})
    actor.send(pid, {{ "action": "reset" }})

    actor.join()
    print("✨ {name} actor lifecycle completed.")
}}

main()
"#,
            name = name
        ),
        Template::WebApp => format!(
            r#"// {name} - Lightweight Web Microservice in Shae

import "std:json" as json

fn route_handler(req) {{
    let path = req.get("path", "/")
    if path == "/api/status" {{
        let payload = {{
            "service": "{name}",
            "status": "online",
            "version": "0.1.0",
            "uptime_seconds": 128
        }}
        return {{
            "status": 200,
            "headers": {{ "Content-Type": "application/json" }},
            "body": json.stringify(payload)
        }}
    }}

    return {{
        "status": 200,
        "headers": {{ "Content-Type": "text/html" }},
        "body": "<h1>Welcome to {name}!</h1><p>Powered by Shae.</p>"
    }}
}}

fn main() {{
    let port = 8080
    print("🌐 {name} HTTP Server initialized on port " + str(port))
    print("   Serving endpoints: / and /api/status")

    let sample_res = route_handler({{ "path": "/api/status" }})
    print("Sample API response: " + sample_res["body"])
}}

main()
"#,
            name = name
        ),
    }
}
