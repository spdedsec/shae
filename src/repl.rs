use crate::value::Value;
use crate::vm::{InterpretResult, VM};
use std::io::{self, BufRead, Write};

pub struct ReplState {
    pub vm: VM,
    pub ast_eval: crate::eval::Evaluator,
    pub use_vm: bool,
}

impl Default for ReplState {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ReplState {
    pub fn new(use_vm: bool) -> Self {
        ReplState {
            vm: VM::new(),
            ast_eval: crate::eval::Evaluator::new(),
            use_vm,
        }
    }

    pub fn reset(&mut self) {
        self.vm = VM::new();
        self.ast_eval = crate::eval::Evaluator::new();
    }

    pub fn user_globals(&self) -> Vec<(String, Value)> {
        let dummy_env = {
            let mut env = crate::env::Environment::new();
            crate::builtins::register(&mut env);
            env.export_map()
        };
        let mut out: Vec<(String, Value)> = self
            .vm
            .globals
            .iter()
            .filter(|(k, _)| k.as_str() != "gc" && !dummy_env.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn eval_line(&mut self, input: &str) -> Result<Option<Value>, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        if self.use_vm {
            let program = crate::parse_source(trimmed).map_err(|e| format!("{}", e))?;
            let chunk = crate::compiler::Compiler::new()
                .compile_program(&program)
                .map_err(|e| format!("Compile error: {}", e))?;
            match self.vm.interpret(chunk) {
                InterpretResult::Ok(val) => Ok(Some(val)),
                InterpretResult::RuntimeError(msg) => Err(format!("Runtime error: {}", msg)),
                InterpretResult::CompileError => Err("VM compile error".to_string()),
            }
        } else {
            let val = crate::run_in_evaluator(trimmed, &mut self.ast_eval)
                .map_err(|e| format!("{}", e))?;
            Ok(Some(val))
        }
    }
}

pub fn is_input_complete(input: &str) -> bool {
    let mut braces = 0isize;
    let mut parens = 0isize;
    let mut brackets = 0isize;
    let mut in_string = false;
    let mut in_single_comment = false;
    let mut escape = false;

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_single_comment {
            if c == '\n' {
                in_single_comment = false;
            }
            i += 1;
            continue;
        }

        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            in_single_comment = true;
            i += 2;
            continue;
        }

        match c {
            '"' => in_string = true,
            '{' => braces += 1,
            '}' => braces -= 1,
            '(' => parens += 1,
            ')' => parens -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            _ => {}
        }
        i += 1;
    }

    !in_string && braces <= 0 && parens <= 0 && brackets <= 0
}

pub fn run_interactive_repl(initial_use_vm: bool) {
    println!(
        "Shae v{} - It runs, it does stuff, it leaves you alone.",
        env!("CARGO_PKG_VERSION")
    );
    let engine_name = if initial_use_vm { "Bytecode VM" } else { "AST" };
    println!("Engine: {} (use :engine [vm|ast] to toggle)", engine_name);
    println!("Type :help for meta-commands or 'exit' / Ctrl+D to quit.\n");

    let mut state = ReplState::new(initial_use_vm);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buffer = String::new();

    loop {
        if buffer.is_empty() {
            print!("shae > ");
        } else {
            print!(" ... > ");
        }
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if buffer.is_empty() {
                    if trimmed == "exit" || trimmed == "quit" || trimmed == ":exit" || trimmed == ":quit" {
                        break;
                    }
                    if trimmed.starts_with(':') {
                        handle_repl_command(&mut state, trimmed);
                        continue;
                    }
                }

                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(line.trim_end());

                if buffer.trim().is_empty() {
                    buffer.clear();
                    continue;
                }

                if !is_input_complete(&buffer) {
                    continue;
                }

                let to_eval = std::mem::take(&mut buffer);
                match state.eval_line(&to_eval) {
                    Ok(Some(val)) => {
                        if val != Value::Null {
                            println!("{}", val.to_display());
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("{}", e);
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

fn handle_repl_command(state: &mut ReplState, cmd: &str) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied().unwrap_or("") {
        ":help" => {
            println!("Shae REPL Commands:");
            println!("  :help              Show this help message");
            println!("  :disasm <code>     Disassemble code snippet to bytecode");
            println!("  :globals           List user-defined globals in active session");
            println!("  :reset             Reset REPL environment and clear globals");
            println!("  :engine            Show current execution engine");
            println!("  :engine vm         Switch to Bytecode VM engine");
            println!("  :engine ast        Switch to AST Interpreter engine");
            println!("  :exit, :quit       Exit the REPL");
        }
        ":reset" => {
            state.reset();
            println!("✨ Environment reset.");
        }
        ":engine" => {
            if parts.len() > 1 {
                match parts[1] {
                    "vm" => {
                        state.use_vm = true;
                        println!("Engine switched to Bytecode VM.");
                    }
                    "ast" => {
                        state.use_vm = false;
                        println!("Engine switched to AST Interpreter.");
                    }
                    other => {
                        eprintln!("Unknown engine '{}'. Valid choices: vm, ast", other);
                    }
                }
            } else {
                let cur = if state.use_vm { "Bytecode VM" } else { "AST Interpreter" };
                println!("Current engine: {}", cur);
            }
        }
        ":globals" => {
            let globals = state.user_globals();
            if globals.is_empty() {
                println!("No user-defined globals.");
            } else {
                println!("User Globals ({}):", globals.len());
                for (name, val) in globals {
                    println!("  {}: {} = {}", name, val.type_name(), val.to_display());
                }
            }
        }
        ":disasm" => {
            let code = cmd.strip_prefix(":disasm").unwrap_or("").trim();
            if code.is_empty() {
                eprintln!("Usage: :disasm <shae code>");
                return;
            }
            match crate::parse_source(code) {
                Ok(prog) => match crate::compiler::Compiler::new().compile_program(&prog) {
                    Ok(chunk) => print!("{}", chunk.disassemble("repl")),
                    Err(e) => eprintln!("Compilation error: {}", e),
                },
                Err(e) => eprintln!("Syntax error: {}", e),
            }
        }
        other => {
            eprintln!("Unknown REPL command '{}'. Type :help for commands.", other);
        }
    }
}
