use std::env;
use std::fs;
use std::process;
use shae::run;
use shae::lexer;
use shae::parser;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: shae <command> <file>");
        println!("Commands:");
        println!("  run <file>    Execute a Shae script");
        println!("  check <file>  Syntax check a Shae script");
        println!("  fmt <file>    Format a Shae script (Coming soon)");
        process::exit(1);
    }
    
    let command = if args.len() == 2 && !args[1].ends_with(".shae") {
        "run".to_string()
    } else if args.len() == 2 {
        "run".to_string()
    } else {
        args[1].clone()
    };
    
    let filename = if args.len() == 2 { &args[1] } else { &args[2] };

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file {}: {}", filename, e);
            process::exit(1);
        }
    };
    
    match command.as_str() {
        "run" => {
            if let Err(e) = run(&source) {
                eprintln!("{}", shae::render_error(&e, &source));
                process::exit(1);
            }
        }
        "check" => {
            println!("Checking syntax for {}...", filename);
            match lexer::tokenize(&source) {
                Ok(tokens) => {
                    match parser::parse(tokens) {
                        Ok(_) => {
                            println!("✅ Syntax OK.");
                        }
                        Err(e) => {
                            eprintln!("❌ Syntax Error: {:?}", e);
                            process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Lexer Error: {:?}", e);
                    process::exit(1);
                }
            }
        }
        "fmt" => {
            println!("Formatting is coming soon!");
        }
        _ => {
            if args.len() == 2 {
                if let Err(e) = run(&source) {
                    eprintln!("{}", shae::render_error(&e, &source));
                    process::exit(1);
                }
            } else {
                eprintln!("Unknown command: {}", command);
                process::exit(1);
            }
        }
    }
}