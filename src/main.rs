use clap::{Parser as ClapParser, Subcommand};
use shae::eval::Evaluator;
use shae::{render_error, run_in_evaluator};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;

#[derive(ClapParser)]
#[command(
    name = "shae",
    about = "The programming language that respects your sanity"
)]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    file: Option<PathBuf>,

    #[arg(long)]
    joke: bool,

    #[arg(long)]
    tip: bool,
}

#[derive(Subcommand)]
enum Commands {
    Run { file: PathBuf },
    Repl,
    New { name: String },
}

fn main() {
    // Run everything in a thread with a large stack to prevent stack overflows
    // from crashing the host process natively before our internal depth limit kicks in.
    let builder = thread::Builder::new()
        .name("shae-main".into())
        .stack_size(256 * 1024 * 1024);

    let handler = builder
        .spawn(|| {
            run_cli();
        })
        .unwrap();

    handler.join().unwrap();
}

fn run_cli() {
    let cli = Cli::parse();

    if cli.joke {
        println!("Why do programmers prefer dark mode? Because light attracts bugs.");
        return;
    }

    if cli.tip {
        println!("💡 Tip: Use `?.` to safely access properties that might be null.");
        return;
    }

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Run { file } => run_file(file),
            Commands::Repl => run_repl(),
            Commands::New { name } => create_project(&name),
        }
    } else if let Some(file) = cli.file {
        run_file(file);
    } else {
        run_repl();
    }
}

fn run_file(path: PathBuf) {
    let source = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file {}: {}", path.display(), e);
            std::process::exit(1);
        }
    };

    let mut ev = Evaluator::new();
    if let Err(e) = run_in_evaluator(&source, &mut ev) {
        eprintln!("{}", render_error(&e, &source));
        std::process::exit(1);
    }
}

fn run_repl() {
    println!("Shae {} REPL", env!("CARGO_PKG_VERSION"));
    println!("Type 'exit' to quit");
    let mut ev = Evaluator::new();
    let mut input = String::new();

    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        input.clear();
        if io::stdin().read_line(&mut input).is_err() || input.trim() == "exit" {
            break;
        }

        if input.trim().is_empty() {
            continue;
        }

        match run_in_evaluator(&input, &mut ev) {
            Ok(val) => {
                if !matches!(val, shae::value::Value::Null) {
                    println!("{}", val.to_display());
                }
            }
            Err(e) => {
                eprintln!("{}", render_error(&e, &input));
            }
        }
    }
}

fn create_project(name: &str) {
    let mut path = PathBuf::from(name);
    if !path.exists() {
        fs::create_dir_all(&path).unwrap();
    }
    path.push("main.shae");
    let content = format!(
        r#"// Welcome to {}!
// Shae is a fast, fun, and strict-by-default language.

let name = "World"
print("Hello, \\{name}!")
"#,
        name
    );
    fs::write(&path, content).unwrap();
    println!("Created new Shae project in {}", name);
}
