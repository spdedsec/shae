use clap::{Parser, Subcommand};
use shae::eval::Evaluator;
use shae::{run, run_in_evaluator, value::Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;

const BANNER: &str = r#"
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
  The programming language that respects your sanity.
"#;

const JOKES: &[&str] = &[
    "Why do programmers prefer dark mode? Because light attracts bugs.",
    "There are only 10 types of people: those who understand binary, and those who don't.",
    "A SQL query walks into a bar, walks up to two tables and asks: 'Can I join you?'",
    "Why was the JavaScript developer sad? Because they didn't Node how to Express themselves.",
    "Why do Rust programmers never tell secrets? Because the borrow checker won't let them share references.",
    "Shae's motto: If you need 50 lines of boilerplate to print 'Hello World', the language failed you, not your computer.",
];

#[derive(Parser, Debug)]
#[command(name = "shae")]
#[command(version = "0.1.0")]
#[command(
    about = "Shae — The Programming Language That Respects Your Sanity",
    before_help = BANNER
)]
struct Cli {
    /// File to run (e.g. `shae main.shae`)
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Show witty programmer humor
    #[arg(long)]
    joke: bool,

    /// Show a random developer tip
    #[arg(long)]
    tip: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a Shae script file
    Run {
        /// Script file path (.shae)
        file: PathBuf,
    },
    /// Start the interactive Shae REPL
    Repl,
    /// Create a new Shae project directory
    New {
        /// Name of the project directory
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    if cli.joke {
        print_random_joke();
        return;
    }

    if cli.tip {
        println!("💡 Shae Pro-tip: Functions return their last evaluated expression, or use explicit 'return'.");
        return;
    }

    if let Some(Commands::Run { file }) = cli.command {
        execute_file(&file);
        return;
    }

    if let Some(Commands::New { name }) = cli.command {
        create_project(&name);
        return;
    }

    if let Some(Commands::Repl) = cli.command {
        start_repl();
        return;
    }

    if let Some(file) = cli.file {
        execute_file(&file);
        return;
    }

    // Default when no args given: Start interactive REPL!
    start_repl();
}

fn execute_file(path: &Path) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Shae Error: Cannot read file '{}': {}", path.display(), err);
            process::exit(1);
        }
    };

    match run(&content) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    }
}

fn start_repl() {
    println!("{}", BANNER);
    println!("Shae v0.1.0 Interactive REPL");
    println!("Type 'exit' or press Ctrl+D to quit.\n");

    let mut evaluator = Evaluator::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("shae> ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed == "exit" || trimmed == "quit" {
                    println!("Goodbye!");
                    break;
                }

                match run_in_evaluator(trimmed, &mut evaluator) {
                    Ok(val) => {
                        if val != Value::Null {
                            println!("=> {}", val.to_repr());
                        }
                    }
                    Err(err) => {
                        eprintln!("{}", err);
                    }
                }
            }
            Err(err) => {
                eprintln!("Error reading input: {}", err);
                break;
            }
        }
    }
}

fn create_project(name: &str) {
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("Shae Error: Directory '{}' already exists!", name);
        process::exit(1);
    }

    if let Err(err) = fs::create_dir_all(dir) {
        eprintln!("Failed to create project directory: {}", err);
        process::exit(1);
    }

    let main_shae = r#"// Welcome to your new Shae project!

fn greet(name) {
    return "Hello, " + name + "! Welcome to Shae."
}

let developer = "Friend"
print(greet(developer))

// Built-in Web Server demo:
// fn handle(req) {
//     return "<h1>Hello from Shae!</h1>"
// }
// serve(8080, handle)
"#;

    let readme = format!(
        "# {}\n\nA project written in [Shae](https://github.com/spdedsec/shae).\n\n## Run\n```bash\nshae run main.shae\n```\n",
        name
    );

    let _ = fs::write(dir.join("main.shae"), main_shae);
    let _ = fs::write(dir.join("README.md"), readme);

    println!("✨ Created new Shae project '{}'!", name);
    println!("👉 To get started:");
    println!("   cd {}", name);
    println!("   shae run main.shae\n");
}

fn print_random_joke() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    let idx = nanos % JOKES.len();
    println!("😄 {}", JOKES[idx]);
}
