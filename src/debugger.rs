use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::Value;
use crate::vm::{InterpretResult, VM};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugStepMode {
    Continue,
    StepIn,
    StepOver {
        target_depth: usize,
        start_line: usize,
    },
    StepOut {
        target_depth: usize,
    },
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: usize,
    pub file: String,
    pub line: usize,
    pub verified: bool,
}

#[derive(Debug, Clone)]
pub struct StackFrameInfo {
    pub id: usize,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub slots_offset: usize,
    pub locals: Vec<(String, Value)>,
}

pub struct DebugSession {
    pub breakpoints: HashMap<String, HashSet<usize>>,
    pub next_bp_id: usize,
    pub step_mode: DebugStepMode,
    pub last_stopped_line: usize,
    pub last_stopped_file: String,
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugSession {
    pub fn new() -> Self {
        DebugSession {
            breakpoints: HashMap::new(),
            next_bp_id: 1,
            step_mode: DebugStepMode::Continue,
            last_stopped_line: 0,
            last_stopped_file: String::new(),
        }
    }

    pub fn set_breakpoint(&mut self, file: &str, line: usize) -> Breakpoint {
        let normalized = normalize_path(file);
        self.breakpoints
            .entry(normalized.clone())
            .or_default()
            .insert(line);

        let id = self.next_bp_id;
        self.next_bp_id += 1;

        Breakpoint {
            id,
            file: normalized,
            line,
            verified: true,
        }
    }

    pub fn clear_breakpoints_for_file(&mut self, file: &str) {
        let normalized = normalize_path(file);
        self.breakpoints.remove(&normalized);
    }

    pub fn remove_breakpoint(&mut self, file: &str, line: usize) -> bool {
        let normalized = normalize_path(file);
        if let Some(set) = self.breakpoints.get_mut(&normalized) {
            set.remove(&line)
        } else {
            false
        }
    }

    pub fn should_stop(
        &mut self,
        current_file: &str,
        current_line: usize,
        call_depth: usize,
    ) -> Option<String> {
        if current_line == 0 {
            return None;
        }

        let normalized = normalize_path(current_file);

        // Check explicit breakpoints
        if let Some(lines) = self.breakpoints.get(&normalized) {
            if lines.contains(&current_line)
                && (current_line != self.last_stopped_line || normalized != self.last_stopped_file)
            {
                self.last_stopped_line = current_line;
                self.last_stopped_file = normalized;
                return Some("breakpoint".to_string());
            }
        }

        // Check stepping conditions
        match self.step_mode {
            DebugStepMode::Pause => {
                self.last_stopped_line = current_line;
                self.last_stopped_file = normalized;
                Some("pause".to_string())
            }
            DebugStepMode::StepIn => {
                if current_line != self.last_stopped_line || normalized != self.last_stopped_file {
                    self.last_stopped_line = current_line;
                    self.last_stopped_file = normalized;
                    Some("step".to_string())
                } else {
                    None
                }
            }
            DebugStepMode::StepOver {
                target_depth,
                start_line,
            } => {
                if call_depth <= target_depth
                    && (current_line != start_line
                        || normalized != self.last_stopped_file
                        || call_depth < target_depth)
                {
                    self.last_stopped_line = current_line;
                    self.last_stopped_file = normalized;
                    Some("step".to_string())
                } else {
                    None
                }
            }
            DebugStepMode::StepOut { target_depth } => {
                if call_depth < target_depth {
                    self.last_stopped_line = current_line;
                    self.last_stopped_file = normalized;
                    Some("step".to_string())
                } else {
                    None
                }
            }
            DebugStepMode::Continue => None,
        }
    }
}

pub fn normalize_path(path_str: &str) -> String {
    let p = Path::new(path_str);
    if let Ok(canonical) = p.canonicalize() {
        canonical.to_string_lossy().to_string()
    } else {
        path_str.replace('\\', "/")
    }
}

pub fn extract_stack_frames(vm: &VM) -> Vec<StackFrameInfo> {
    let mut frames = Vec::new();
    let current_file_str = vm
        .current_file
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "<script>".to_string());

    for (idx, frame) in vm.frames.iter().enumerate().rev() {
        let chunk = &frame.closure.function.chunk;
        let line = if frame.ip > 0 && frame.ip <= chunk.lines.len() {
            chunk.lines[frame.ip - 1]
        } else if !chunk.lines.is_empty() {
            chunk.lines[0]
        } else {
            1
        };

        let fn_name = frame.closure.function.name.clone().unwrap_or_else(|| {
            if idx == 0 {
                "<main>".to_string()
            } else {
                "<anonymous>".to_string()
            }
        });

        let mut locals = Vec::new();
        let slot_start = frame.slots_offset;
        let next_slot = if idx + 1 < vm.frames.len() {
            vm.frames[idx + 1].slots_offset
        } else {
            vm.stack.len()
        };

        if slot_start < vm.stack.len() {
            let end = next_slot.min(vm.stack.len());
            for (slot_idx, val) in vm.stack[slot_start..end].iter().enumerate() {
                locals.push((format!("var_{}", slot_idx), val.clone()));
            }
        }

        frames.push(StackFrameInfo {
            id: idx + 1,
            name: fn_name,
            file: current_file_str.clone(),
            line,
            slots_offset: slot_start,
            locals,
        });
    }

    frames
}

pub fn run_interactive_debugger(path_str: &str) -> Result<(), String> {
    let source = fs::read_to_string(path_str)
        .map_err(|e| format!("Failed to read file '{}': {}", path_str, e))?;

    let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();

    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("Lexing error: {:?}", e))?;

    let mut parser = Parser::new(tokens);
    let program = parser
        .parse()
        .map_err(|e| format!("Parsing error: {:?}", e))?;

    let chunk = Compiler::new()
        .compile_program(&program)
        .map_err(|e| format!("Compilation error: {}", e))?;

    println!("============================================================");
    println!(
        "             Shae Interactive Debugger v{}                  ",
        env!("CARGO_PKG_VERSION")
    );
    println!("Type 'help' for commands, 'b <line>' for breakpoint, 'run' to start.");
    println!("============================================================");

    let session = Arc::new(Mutex::new(DebugSession::new()));
    session.lock().unwrap().step_mode = DebugStepMode::StepIn; // Stop on first line

    let mut vm = VM::new();
    let norm_path = normalize_path(path_str);
    vm.current_file = Some(PathBuf::from(&norm_path));
    vm.debug_session = Some(session.clone());

    let paused_cv = Arc::new((Mutex::new(false), Condvar::new()));
    let cv_clone = paused_cv.clone();

    // Spawn execution
    let handle = std::thread::spawn(move || vm.interpret(chunk));

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line_buf = String::new();

    loop {
        print!("(shae-dbg) ");
        io::stdout().flush().unwrap();
        line_buf.clear();
        if reader.read_line(&mut line_buf).unwrap_or(0) == 0 {
            break;
        }

        let cmd = line_buf.trim();
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "b" | "break" => {
                if parts.len() < 2 {
                    println!("Usage: b <line_number>");
                } else if let Ok(l) = parts[1].parse::<usize>() {
                    session.lock().unwrap().set_breakpoint(&norm_path, l);
                    println!("Breakpoint set at line {}", l);
                } else {
                    println!("Invalid line number: {}", parts[1]);
                }
            }
            "d" | "delete" => {
                if parts.len() < 2 {
                    session
                        .lock()
                        .unwrap()
                        .clear_breakpoints_for_file(&norm_path);
                    println!("Cleared all breakpoints");
                } else if let Ok(l) = parts[1].parse::<usize>() {
                    session.lock().unwrap().remove_breakpoint(&norm_path, l);
                    println!("Removed breakpoint at line {}", l);
                }
            }
            "c" | "continue" | "r" | "run" => {
                session.lock().unwrap().step_mode = DebugStepMode::Continue;
                let (lock, cvar) = &*cv_clone;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
            }
            "s" | "step" | "stepin" => {
                session.lock().unwrap().step_mode = DebugStepMode::StepIn;
                let (lock, cvar) = &*cv_clone;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
            }
            "n" | "next" => {
                let cur_line = session.lock().unwrap().last_stopped_line;
                session.lock().unwrap().step_mode = DebugStepMode::StepOver {
                    target_depth: 1,
                    start_line: cur_line,
                };
                let (lock, cvar) = &*cv_clone;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
            }
            "l" | "list" => {
                let cur = session.lock().unwrap().last_stopped_line;
                let start = cur.saturating_sub(4).max(1);
                let end = (cur + 4).min(lines.len());
                for l in start..=end {
                    let marker = if l == cur { "=>" } else { "  " };
                    if l - 1 < lines.len() {
                        println!("{} {:4} | {}", marker, l, lines[l - 1]);
                    }
                }
            }
            "h" | "help" => {
                println!("Available commands:");
                println!("  b <line>       Set breakpoint at line");
                println!("  d [line]       Delete breakpoint (or all)");
                println!("  c, continue    Continue execution");
                println!("  n, next        Step over current line");
                println!("  s, step        Step into function");
                println!("  l, list        View source lines around current line");
                println!("  q, quit        Exit debugger");
            }
            "q" | "quit" | "exit" => {
                println!("Terminating debug session.");
                break;
            }
            other => {
                println!(
                    "Unknown command: '{}'. Type 'help' for instructions.",
                    other
                );
            }
        }

        if handle.is_finished() {
            break;
        }
    }

    match handle.join() {
        Ok(InterpretResult::Ok(val)) => {
            println!("\nProgram finished successfully: {}", val.to_display());
            Ok(())
        }
        Ok(InterpretResult::RuntimeError(msg)) => {
            println!("\nProgram terminated with runtime error: {}", msg);
            Err(msg)
        }
        Ok(InterpretResult::CompileError) => Err("Compilation error".to_string()),
        Err(_) => Err("Debug execution thread panicked".to_string()),
    }
}
