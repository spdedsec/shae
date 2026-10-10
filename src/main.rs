use shae::eval::Evaluator;
use shae::lexer;
use shae::parser;
use shae::run_in_evaluator;
use shae::value::Value;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

fn show_help() {
    println!(
        "Shae v{} - It runs, it does stuff, it leaves you alone.\n",
        env!("CARGO_PKG_VERSION")
    );
    println!("Usage:");
    println!("  shae                       Start the interactive REPL");
    println!("  shae repl                  Start the interactive REPL");
    println!(
        "  shae <file.shae>           Run a Shae script (default: VM; use --engine=ast for AST, --disasm)"
    );
    println!(
        "  shae run [file.shae]       Run a Shae script (or package entrypoint from shae.toml) (default: VM; use --engine=ast, --disasm)"
    );
    println!(
        "  shae compile <file.shae>   Compile a Shae script to bytecode (.shaec) (options: -o, --disasm)"
    );
    println!("  shae disasm <file>         Disassemble a Shae script or .shaec to bytecode");
    println!("  shae check <file.shae>     Lint and check syntax/declarations of a Shae script");
    println!(
        "  shae test [path]           Run Shae tests (*_test.shae) (default: VM; use --engine=ast)"
    );
    println!("  shae fmt <file.shae>       Format a Shae script");
    println!("  shae new <project_name>    Create a new Shae project folder");
    println!("  shae init [name]           Initialize package manifest (shae.toml)");
    println!("  shae add <dep> <source>    Add dependency (git URL or local path)");
    println!(
        "  shae install               Install dependencies into .shae/packages and update shae.lock"
    );
    println!("  shae pkg <cmd>             Package manager subcommands (init, add, install)");
    println!("  shae lsp                   Start the Language Server Protocol daemon (stdio)");
    println!("  shae bundle <entry.shae>   Bundle application into a standalone executable");
    println!("  shae --joke                Print a programming joke");
    println!("  shae --tip                 Print a Shae tip");
    println!("  shae --version, -V         Show version information");
    println!("  shae --help, -h            Show this help message");
}

fn show_joke() {
    let jokes = [
        "Why do programmers prefer dark mode? Because light attracts bugs!",
        "There are 10 types of people: those who understand binary, and those who don't.",
        "A SQL query walks into a bar, walks up to two tables and asks: 'Can I join you?'",
        "Why did the developer go broke? Because they used up all their cache.",
        "Shae doesn't have a borrow checker. You're welcome.",
    ];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize % jokes.len())
        .unwrap_or(0);
    println!("😄 {}", jokes[idx]);
}

fn show_tip() {
    let tips = [
        "Use safe navigation `?.` and null coalescing `??` to prevent crashes: `user?.profile?.name ?? \"Guest\"`",
        "Pipe data cleanly through transformations with `|>`: `items |> filter(is_valid) |> map(name)`",
        "Strings support inline interpolation: `\"Hello {name}!\"`. Escape literal braces with `\\{`.",
        "Functions have implicit returns: the last expression evaluated in a block is automatically returned.",
        "Use `shae check file.shae` to find typos and undeclared symbols before runtime.",
        "Use `assert(cond)` and `assert_eq(actual, expected)` to write bulletproof tests.",
    ];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize % tips.len())
        .unwrap_or(0);
    println!("💡 {}", tips[idx]);
}

fn new_project(name: &str) {
    if let Err(e) = fs::create_dir_all(name) {
        eprintln!("Error creating directory '{}': {}", name, e);
        process::exit(1);
    }
    let main_file = format!("{}/main.shae", name);
    let template = r#"// Welcome to your new Shae project!

fn greet(name) {
    "Hello {name}, welcome to Shae!"
}

print(greet("World"))
"#;
    if let Err(e) = fs::write(&main_file, template) {
        eprintln!("Error creating '{}': {}", main_file, e);
        process::exit(1);
    }
    println!("✨ Created new Shae project in '{}/'", name);
    println!("   Run with: cd {} && shae main.shae", name);
}

fn run_repl() {
    println!(
        "Shae v{} - It runs, it does stuff, it leaves you alone.",
        env!("CARGO_PKG_VERSION")
    );
    println!("Type 'exit' or press Ctrl+D to quit.\n");

    let mut ev = Evaluator::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("shae > ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            } // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed == "exit" || trimmed == "quit" {
                    break;
                }
                if trimmed.is_empty() {
                    continue;
                }
                match run_in_evaluator(trimmed, &mut ev) {
                    Ok(val) => {
                        if val != Value::Null {
                            println!("{}", val.to_display());
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", shae::render_error(&e, trimmed));
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }
}

fn should_use_vm(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--engine=ast") {
        return false;
    }
    if args.iter().any(|a| a == "--engine=vm" || a == "--vm") {
        return true;
    }
    if let Ok(val) = std::env::var("SHAE_ENGINE") {
        let val_lower = val.to_lowercase();
        if val_lower == "ast" {
            return false;
        }
        if val_lower == "vm" {
            return true;
        }
    }
    true
}

fn compile_file(source_file: &str, output_file: Option<&str>, disasm: bool) {
    let source = match fs::read_to_string(source_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", source_file, e);
            process::exit(1);
        }
    };
    let bytecode = match shae::compile_source_to_bytecode(&source) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}", shae::render_error(&e, &source));
            process::exit(1);
        }
    };
    let default_out = {
        let path = Path::new(source_file);
        let mut out = path.to_path_buf();
        out.set_extension("shaec");
        out.to_string_lossy().to_string()
    };
    let target_out = output_file.unwrap_or(&default_out);
    if let Err(e) = fs::write(target_out, &bytecode) {
        eprintln!("Error writing bytecode to '{}': {}", target_out, e);
        process::exit(1);
    }
    println!(
        "✨ Compiled '{}' -> '{}' ({} bytes)",
        source_file,
        target_out,
        bytecode.len()
    );
    if disasm {
        match shae::disassemble_bytecode(&bytecode, target_out) {
            Ok(d) => println!("\n{}", d),
            Err(e) => eprintln!("Disassembly error: {}", e),
        }
    }
}

fn disasm_file(filename: &str) {
    let bytes = match fs::read(filename) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };
    if bytes.starts_with(shae::chunk::BYTECODE_MAGIC) || filename.ends_with(".shaec") {
        match shae::disassemble_bytecode(&bytes, filename) {
            Ok(disasm) => print!("{}", disasm),
            Err(e) => {
                eprintln!("Error disassembling bytecode '{}': {}", filename, e);
                process::exit(1);
            }
        }
    } else {
        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: File '{}' is not valid UTF-8: {}", filename, e);
                process::exit(1);
            }
        };
        match shae::disassemble_source(&source) {
            Ok(disasm) => print!("{}", disasm),
            Err(e) => {
                eprintln!("{}", shae::render_error(&e, &source));
                process::exit(1);
            }
        }
    }
}

fn run_file(filename: &str, use_vm: bool) {
    let is_bytecode = filename.ends_with(".shaec")
        || fs::read(filename)
            .map(|b| b.starts_with(shae::chunk::BYTECODE_MAGIC))
            .unwrap_or(false);
    if is_bytecode && !use_vm {
        eprintln!(
            "Error: Cannot run precompiled bytecode with AST interpreter (--engine=ast). Use the VM."
        );
        process::exit(1);
    }
    let res = if use_vm {
        shae::run_file_vm(filename)
    } else {
        shae::run_file(filename)
    };
    if let Err(e) = res {
        if is_bytecode {
            eprintln!("{}", e);
        } else {
            let source = fs::read_to_string(filename).unwrap_or_default();
            eprintln!("{}", shae::render_error(&e, &source));
        }
        process::exit(1);
    }
}

fn check_file(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file {}: {}", filename, e);
            process::exit(1);
        }
    };
    println!("Checking syntax & declarations for {}...", filename);
    let tokens = match lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", shae::render_error(&e.into(), &source));
            process::exit(1);
        }
    };
    let program = match parser::parse(tokens) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", shae::render_error(&e.into(), &source));
            process::exit(1);
        }
    };

    let mut linter = shae::linter::Linter::new();
    let diagnostics = linter.lint_program(&program);

    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in diagnostics {
        let prefix = match diag.severity {
            shae::linter::DiagnosticSeverity::Error => {
                error_count += 1;
                "❌ Error"
            }
            shae::linter::DiagnosticSeverity::Warning => {
                warning_count += 1;
                "⚠️  Warning"
            }
        };

        let mut lines = source.lines();
        if let Some(source_line) = lines.nth(diag.span.line.saturating_sub(1)) {
            let caret = " ".repeat(diag.span.col.saturating_sub(1)) + "^";
            eprintln!(
                "{}: {}\n\n  {} | {}\n  {} | {}",
                prefix,
                diag.message,
                diag.span.line,
                source_line,
                " ".repeat(diag.span.line.to_string().len()),
                caret
            );
        } else {
            eprintln!("{}: {}", prefix, diag.message);
        }

        if let Some(hint) = &diag.hint {
            eprintln!("  💡 Hint: {}\n", hint);
        } else {
            eprintln!();
        }
    }

    if error_count > 0 {
        eprintln!(
            "Found {} error(s), {} warning(s).",
            error_count, warning_count
        );
        process::exit(1);
    } else if warning_count > 0 {
        println!(
            "✅ Syntax and declarations OK (with {} warning(s)).",
            warning_count
        );
    } else {
        println!("✅ Syntax and declarations OK. No issues found.");
    }
}

fn run_tests(target: Option<&str>, use_vm: bool) {
    let mut test_files = Vec::new();
    let root = target.unwrap_or("tests");

    if let Ok(metadata) = fs::metadata(root) {
        if metadata.is_file() {
            test_files.push(root.to_string());
        } else if metadata.is_dir() {
            find_test_files(Path::new(root), &mut test_files);
        }
    } else if target.is_none() {
        find_test_files(Path::new("."), &mut test_files);
    } else {
        eprintln!("Error: Path '{}' not found", root);
        process::exit(1);
    }

    if test_files.is_empty() {
        println!(
            "No test files (*_test.shae or test_*.shae) found in '{}'.",
            root
        );
        return;
    }

    println!("running {} tests in '{}'...\n", test_files.len(), root);
    let mut passed = 0;
    let mut failed = 0;

    for file in &test_files {
        let source = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                println!("test {} ... FAILED (Failed to read file: {})", file, e);
                failed += 1;
                continue;
            }
        };

        let res = if use_vm {
            shae::run_file_vm(file)
        } else {
            shae::run_file(file)
        };
        match res {
            Ok(_) => {
                println!("test {} ... ok", file);
                passed += 1;
            }
            Err(e) => {
                println!("test {} ... FAILED", file);
                eprintln!("{}\n", shae::render_error(&e, &source));
                failed += 1;
            }
        }
    }

    println!();
    if failed == 0 {
        println!("test result: ok. {} passed; 0 failed", passed);
    } else {
        eprintln!("test result: FAILED. {} passed; {} failed", passed, failed);
        process::exit(1);
    }
}

fn find_test_files(dir: &Path, out: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target" && name != ".git" && name != "node_modules" {
                    find_test_files(&path, out);
                }
            } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".shae")
                    && (file_name.ends_with("_test.shae") || file_name.starts_with("test_"))
                {
                    out.push(path.to_string_lossy().to_string());
                }
            }
        }
    }
}

fn run_bundled_archive(archive: shae::bundle::BundleArchive) {
    let entry_source = archive
        .files
        .get(&archive.entry_path)
        .or_else(|| {
            let p = Path::new(&archive.entry_path);
            let name = p.file_name()?.to_str()?;
            archive.files.get(name)
        })
        .or_else(|| {
            let p = Path::new(&archive.entry_path);
            let name = p.file_name()?.to_str()?;
            archive.files.get(&format!("./{}", name))
        })
        .cloned();

    if let Some(entry_source) = entry_source {
        let mut ev = Evaluator::new();
        ev.current_file = Some(std::path::PathBuf::from(&archive.entry_path));
        ev.embedded_archive = Some(std::sync::Arc::new(archive.files));
        if let Err(e) = shae::run_in_evaluator(&entry_source, &mut ev) {
            eprintln!("{}", shae::render_error(&e, &entry_source));
            process::exit(1);
        }
    } else {
        eprintln!(
            "Error: Bundled entry point '{}' not found in archive",
            archive.entry_path
        );
        process::exit(1);
    }
}

fn main() {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Ok(Some(archive)) = shae::bundle::read_embedded_bundle(&exe_path) {
            run_bundled_archive(archive);
            return;
        }
    }

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        run_repl();
        return;
    }

    match args[1].as_str() {
        "repl" => run_repl(),
        "--joke" => show_joke(),
        "--tip" => show_tip(),
        "--version" | "-V" => {
            println!("shae {}", env!("CARGO_PKG_VERSION"));
        }
        "--help" | "-h" => show_help(),
        "new" => {
            if args.len() < 3 {
                eprintln!("Usage: shae new <project_name>");
                process::exit(1);
            }
            new_project(&args[2]);
        }
        "init" => {
            let name = args.get(2).map(|s| s.as_str());
            match shae::pkg::init_project(Path::new("."), name) {
                Ok(m) => println!(
                    "✨ Initialized package '{}' (v{}) with shae.toml",
                    m.package.name, m.package.version
                ),
                Err(e) => {
                    eprintln!("Error initializing package: {}", e);
                    process::exit(1);
                }
            }
        }
        "add" => {
            if args.len() < 4 {
                eprintln!("Usage: shae add <dep_name> <source> [--branch <b/t/r>]");
                process::exit(1);
            }
            let dep_name = &args[2];
            let source = &args[3];
            let mut branch = None;
            let mut tag = None;
            let mut rev = None;
            let mut i = 4;
            while i < args.len() {
                if args[i] == "--branch" && i + 1 < args.len() {
                    branch = Some(args[i + 1].as_str());
                    i += 2;
                } else if args[i] == "--tag" && i + 1 < args.len() {
                    tag = Some(args[i + 1].as_str());
                    i += 2;
                } else if args[i] == "--rev" && i + 1 < args.len() {
                    rev = Some(args[i + 1].as_str());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            match shae::pkg::add_dependency(Path::new("."), dep_name, source, branch, tag, rev) {
                Ok(_) => println!("✅ Added and resolved dependency '{}'", dep_name),
                Err(e) => {
                    eprintln!("Error adding dependency: {}", e);
                    process::exit(1);
                }
            }
        }
        "install" => match shae::pkg::install_dependencies(Path::new(".")) {
            Ok(lock) => println!(
                "✅ Installed {} package(s). shae.lock up to date.",
                lock.packages.len()
            ),
            Err(e) => {
                eprintln!("Error installing dependencies: {}", e);
                process::exit(1);
            }
        },
        "pkg" => {
            if args.len() < 3 {
                println!("Usage: shae pkg <init|add|install>");
                return;
            }
            match args[2].as_str() {
                "init" => {
                    let name = args.get(3).map(|s| s.as_str());
                    match shae::pkg::init_project(Path::new("."), name) {
                        Ok(m) => println!(
                            "✨ Initialized package '{}' (v{}) with shae.toml",
                            m.package.name, m.package.version
                        ),
                        Err(e) => {
                            eprintln!("Error initializing package: {}", e);
                            process::exit(1);
                        }
                    }
                }
                "add" => {
                    if args.len() < 5 {
                        eprintln!("Usage: shae pkg add <dep_name> <source>");
                        process::exit(1);
                    }
                    let dep_name = &args[3];
                    let source = &args[4];
                    match shae::pkg::add_dependency(
                        Path::new("."),
                        dep_name,
                        source,
                        None,
                        None,
                        None,
                    ) {
                        Ok(_) => println!("✅ Added and resolved dependency '{}'", dep_name),
                        Err(e) => {
                            eprintln!("Error adding dependency: {}", e);
                            process::exit(1);
                        }
                    }
                }
                "install" => match shae::pkg::install_dependencies(Path::new(".")) {
                    Ok(lock) => println!(
                        "✅ Installed {} package(s). shae.lock up to date.",
                        lock.packages.len()
                    ),
                    Err(e) => {
                        eprintln!("Error installing dependencies: {}", e);
                        process::exit(1);
                    }
                },
                other => {
                    eprintln!("Unknown pkg command: {}", other);
                    process::exit(1);
                }
            }
        }
        "run" => {
            let use_vm = should_use_vm(&args);
            let disasm = args.iter().any(|a| a == "--disasm");
            let file_target = args[2..].iter().find(|a| !a.starts_with('-'));
            if let Some(f) = file_target {
                if disasm {
                    disasm_file(f);
                }
                run_file(f, use_vm);
            } else if let Ok(manifest) = shae::pkg::PackageManifest::load_from_dir(Path::new(".")) {
                let entry = Path::new(".").join(&manifest.package.entry);
                if entry.exists() {
                    let entry_str = entry.to_string_lossy().to_string();
                    if disasm {
                        disasm_file(&entry_str);
                    }
                    run_file(&entry_str, use_vm);
                } else {
                    eprintln!("Error: Package entry '{}' not found.", entry.display());
                    process::exit(1);
                }
            } else {
                eprintln!("Usage: shae run <file.shae> [--vm] [--engine=vm|ast] [--disasm]");
                process::exit(1);
            }
        }
        "compile" => {
            if args.len() < 3 {
                eprintln!("Usage: shae compile <file.shae> [-o <output.shaec>] [--disasm]");
                process::exit(1);
            }
            let mut source_file = None;
            let mut output_file = None;
            let disasm = args.iter().any(|a| a == "--disasm");
            let mut i = 2;
            while i < args.len() {
                if args[i] == "-o" || args[i] == "--output" {
                    if i + 1 < args.len() {
                        output_file = Some(args[i + 1].clone());
                        i += 2;
                    } else {
                        eprintln!("Error: -o requires an output path");
                        process::exit(1);
                    }
                } else if args[i] == "--disasm" {
                    i += 1;
                } else if !args[i].starts_with('-') && source_file.is_none() {
                    source_file = Some(args[i].clone());
                    i += 1;
                } else {
                    i += 1;
                }
            }
            if let Some(src) = source_file {
                compile_file(&src, output_file.as_deref(), disasm);
            } else {
                eprintln!("Usage: shae compile <file.shae> [-o <output.shaec>] [--disasm]");
                process::exit(1);
            }
        }
        "disasm" => {
            let file_target = args[2..].iter().find(|a| !a.starts_with('-'));
            if let Some(f) = file_target {
                disasm_file(f);
            } else {
                eprintln!("Usage: shae disasm <file.shae|file.shaec>");
                process::exit(1);
            }
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Usage: shae check <file.shae>");
                process::exit(1);
            }
            check_file(&args[2]);
        }
        "test" => {
            let use_vm = should_use_vm(&args);
            let target = args[2..]
                .iter()
                .find(|a| !a.starts_with('-'))
                .map(|s| s.as_str());
            run_tests(target, use_vm);
        }
        "fmt" => {
            let mut check_only = false;
            let mut force = false;
            let mut target = None;
            for arg in &args[2..] {
                if arg == "--help" || arg == "-h" {
                    println!(
                        "Usage: shae fmt [path] [--check] [--force]\n\nFormat Shae source files in place, or check formatting with --check."
                    );
                    return;
                } else if arg == "--check" {
                    check_only = true;
                } else if arg == "--force" || arg == "-f" {
                    force = true;
                } else if target.is_none() {
                    target = Some(arg.as_str());
                }
            }
            let path = target.unwrap_or(".");
            format_target(path, check_only, force);
        }
        "lsp" => {
            if let Err(e) = shae::lsp::start_lsp_server() {
                eprintln!("Shae LSP server error: {}", e);
                process::exit(1);
            }
        }
        "bundle" => {
            if args.len() < 3 {
                eprintln!("Usage: shae bundle <entry.shae> [-o <output_binary>]");
                process::exit(1);
            }
            let entry = Path::new(&args[2]);
            let mut output = None;
            let mut i = 3;
            while i < args.len() {
                if (args[i] == "-o" || args[i] == "--output") && i + 1 < args.len() {
                    output = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let out_path = output.unwrap_or_else(|| {
                let stem = entry
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("bundle");
                format!("{}.bin", stem)
            });
            match shae::bundle::create_standalone_binary(entry, Path::new(&out_path)) {
                Ok(_) => println!("✨ Successfully created standalone bundle: {}", out_path),
                Err(e) => {
                    eprintln!("Error bundling application: {}", e);
                    process::exit(1);
                }
            }
        }
        filename => {
            if filename.starts_with('-') {
                if let Some(actual_file) = args[1..].iter().find(|a| !a.starts_with('-')) {
                    let disasm = args.iter().any(|a| a == "--disasm");
                    if disasm {
                        disasm_file(actual_file);
                    }
                    let use_vm = should_use_vm(&args);
                    run_file(actual_file, use_vm);
                    return;
                }
                eprintln!("Unknown flag: {}", filename);
                show_help();
                process::exit(1);
            }
            let disasm = args.iter().any(|a| a == "--disasm");
            if disasm {
                disasm_file(filename);
            }
            let use_vm = should_use_vm(&args);
            run_file(filename, use_vm);
        }
    }
}

fn find_shae_files(dir: &Path, out: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target" && name != ".git" && name != "node_modules" {
                    find_shae_files(&path, out);
                }
            } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".shae") {
                    out.push(path.to_string_lossy().to_string());
                }
            }
        }
    }
}

fn format_target(target: &str, check_only: bool, force: bool) {
    let mut files = Vec::new();
    let path = Path::new(target);
    if path.is_file() {
        files.push(target.to_string());
    } else if path.is_dir() {
        find_shae_files(path, &mut files);
    } else {
        eprintln!("Error: Path '{}' not found", target);
        process::exit(1);
    }

    if files.is_empty() {
        println!("No .shae files found in '{}'.", target);
        return;
    }

    let mut unformatted = 0;
    let mut formatted_count = 0;

    for file in &files {
        let source = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading file '{}': {}", file, e);
                unformatted += 1;
                continue;
            }
        };

        let formatted = match shae::format_source(&source) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Syntax error in '{}':", file);
                eprintln!("{}\n", shae::render_error(&e, &source));
                unformatted += 1;
                continue;
            }
        };

        let has_comments = source.contains("//") || source.contains("/*");

        if source != formatted {
            if check_only {
                if has_comments {
                    println!("⚠️  Contains comments (would strip): {}", file);
                }
                println!("❌ Needs formatting: {}", file);
                unformatted += 1;
            } else if has_comments && !force {
                eprintln!(
                    "⚠️  Warning: '{}' contains comments which are not preserved by the AST formatter. Skipped to prevent comment loss (use --force to format anyway).",
                    file
                );
            } else {
                if let Err(e) = fs::write(file, &formatted) {
                    eprintln!("Error writing '{}': {}", file, e);
                    unformatted += 1;
                } else {
                    println!("Formatted {}", file);
                    formatted_count += 1;
                }
            }
        }
    }

    if check_only {
        if unformatted > 0 {
            eprintln!(
                "\n{} file(s) need formatting. Run `shae fmt` to fix.",
                unformatted
            );
            process::exit(1);
        } else {
            println!("All {} file(s) properly formatted.", files.len());
        }
    } else if formatted_count == 0 {
        println!("All {} file(s) already formatted.", files.len());
    } else {
        println!("Successfully formatted {} file(s).", formatted_count);
    }
}
