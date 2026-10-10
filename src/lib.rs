pub mod ast;
pub mod builtins;
pub mod bundle;
pub mod channel;
pub mod chunk;
pub mod compiler;
pub mod env;
pub mod eval;
pub mod fmt;
pub mod gc;
pub mod lexer;
pub mod linter;
pub mod lsp;
pub mod opcode;
pub mod parser;
pub mod pkg;
pub mod stdlib;
pub mod suggest;
pub mod token;
pub mod value;
pub mod vm;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShaeError {
    #[error(transparent)]
    Lexer(#[from] lexer::LexerError),

    #[error(transparent)]
    Parser(#[from] parser::ParserError),

    #[error(transparent)]
    Runtime(#[from] eval::RuntimeError),
}

pub fn render_error(err: &ShaeError, source: &str) -> String {
    let (line, col, msg) = match err {
        ShaeError::Lexer(lexer::LexerError::UnexpectedChar {
            line, col, hint, ..
        }) => (*line, *col, format!("{} {}", err, hint)),
        ShaeError::Lexer(e) => {
            // we could match to get line/col for other lexer errors
            let msg = e.to_string();
            return msg;
        }
        ShaeError::Parser(parser::ParserError::Advice { line, col, .. }) => {
            (*line, *col, err.to_string())
        }
        ShaeError::Parser(parser::ParserError::UnexpectedToken { line, col, .. }) => {
            (*line, *col, err.to_string())
        }
        ShaeError::Parser(parser::ParserError::Lexer(e)) => {
            return render_error(&ShaeError::Lexer(e.clone()), source);
        }
        ShaeError::Runtime(e) => {
            if let Some(span) = e.span {
                let mut out = e.to_string();
                if !e.stack.is_empty() {
                    out.push_str("\n\nStack Trace:");
                    for (func, sp) in e.stack.iter().rev() {
                        out.push_str(&format!("\n  at {} (line {}:{})", func, sp.line, sp.col));
                    }
                }
                (span.line, span.col, out)
            } else {
                return e.to_string();
            }
        }
    };

    let mut lines = source.lines();
    if let Some(source_line) = lines.nth(line.saturating_sub(1)) {
        let caret = " ".repeat(col.saturating_sub(1)) + "^";
        format!(
            "{}\n\n  {} | {}\n  {} | {}",
            msg,
            line,
            source_line,
            " ".repeat(line.to_string().len()),
            caret
        )
    } else {
        msg
    }
}

pub fn run(source: &str) -> Result<value::Value, ShaeError> {
    let mut ev = eval::Evaluator::new();
    run_in_evaluator(source, &mut ev)
}

pub fn run_vm(source: &str) -> Result<value::Value, ShaeError> {
    let program = parse_source(source)?;
    let chunk = compiler::Compiler::new()
        .compile_program(&program)
        .map_err(eval::RuntimeError::new)?;
    let mut vm = vm::VM::new();
    match vm.interpret(chunk) {
        vm::InterpretResult::Ok(val) => Ok(val),
        vm::InterpretResult::RuntimeError(msg) => Err(eval::RuntimeError::new(msg).into()),
        vm::InterpretResult::CompileError => {
            Err(eval::RuntimeError::new("VM compile error".into()).into())
        }
    }
}

pub fn run_file<P: AsRef<std::path::Path>>(path: P) -> Result<value::Value, ShaeError> {
    let path_ref = path.as_ref();
    let source = std::fs::read_to_string(path_ref).map_err(|e| {
        eval::RuntimeError::new(format!(
            "Failed to read file '{}': {}",
            path_ref.display(),
            e
        ))
    })?;
    let mut ev = eval::Evaluator::new();
    ev.current_file = std::fs::canonicalize(path_ref)
        .ok()
        .or_else(|| Some(path_ref.to_path_buf()));
    run_in_evaluator(&source, &mut ev)
}

pub fn run_file_vm<P: AsRef<std::path::Path>>(path: P) -> Result<value::Value, ShaeError> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref).map_err(|e| {
        eval::RuntimeError::new(format!(
            "Failed to read file '{}': {}",
            path_ref.display(),
            e
        ))
    })?;

    let chunk = if bytes.starts_with(crate::chunk::BYTECODE_MAGIC)
        || path_ref.extension().map_or(false, |ext| ext == "shaec")
    {
        chunk::Chunk::from_bytes(&bytes)
            .map_err(|e| eval::RuntimeError::new(format!("Bytecode load error in '{}': {}", path_ref.display(), e)))?
    } else {
        let source = String::from_utf8(bytes).map_err(|e| {
            eval::RuntimeError::new(format!(
                "File '{}' is not valid UTF-8: {}",
                path_ref.display(),
                e
            ))
        })?;
        let program = parse_source(&source)?;
        compiler::Compiler::new()
            .compile_program(&program)
            .map_err(eval::RuntimeError::new)?
    };

    let mut vm = vm::VM::new();
    vm.current_file = std::fs::canonicalize(path_ref)
        .ok()
        .or_else(|| Some(path_ref.to_path_buf()));
    match vm.interpret(chunk) {
        vm::InterpretResult::Ok(val) => Ok(val),
        vm::InterpretResult::RuntimeError(msg) => Err(eval::RuntimeError::new(msg).into()),
        vm::InterpretResult::CompileError => {
            Err(eval::RuntimeError::new("VM compile error".into()).into())
        }
    }
}

pub fn compile_source_to_bytecode(source: &str) -> Result<Vec<u8>, ShaeError> {
    let program = parse_source(source)?;
    let chunk = compiler::Compiler::new()
        .compile_program(&program)
        .map_err(eval::RuntimeError::new)?;
    Ok(chunk.to_bytes())
}

pub fn compile_file_to_bytecode<P: AsRef<std::path::Path>, Q: AsRef<std::path::Path>>(
    src: P,
    out: Q,
) -> Result<(), ShaeError> {
    let src_ref = src.as_ref();
    let source = std::fs::read_to_string(src_ref).map_err(|e| {
        eval::RuntimeError::new(format!(
            "Failed to read file '{}': {}",
            src_ref.display(),
            e
        ))
    })?;
    let bytes = compile_source_to_bytecode(&source)?;
    std::fs::write(out.as_ref(), bytes).map_err(|e| {
        eval::RuntimeError::new(format!(
            "Failed to write bytecode file '{}': {}",
            out.as_ref().display(),
            e
        ))
    })?;
    Ok(())
}

pub fn run_bytecode(bytes: &[u8]) -> Result<value::Value, ShaeError> {
    let chunk = chunk::Chunk::from_bytes(bytes).map_err(eval::RuntimeError::new)?;
    let mut vm = vm::VM::new();
    match vm.interpret(chunk) {
        vm::InterpretResult::Ok(val) => Ok(val),
        vm::InterpretResult::RuntimeError(msg) => Err(eval::RuntimeError::new(msg).into()),
        vm::InterpretResult::CompileError => {
            Err(eval::RuntimeError::new("VM compile error".into()).into())
        }
    }
}

pub fn run_bytecode_file<P: AsRef<std::path::Path>>(path: P) -> Result<value::Value, ShaeError> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref).map_err(|e| {
        eval::RuntimeError::new(format!(
            "Failed to read bytecode file '{}': {}",
            path_ref.display(),
            e
        ))
    })?;
    let chunk = chunk::Chunk::from_bytes(&bytes).map_err(eval::RuntimeError::new)?;
    let mut vm = vm::VM::new();
    vm.current_file = std::fs::canonicalize(path_ref)
        .ok()
        .or_else(|| Some(path_ref.to_path_buf()));
    match vm.interpret(chunk) {
        vm::InterpretResult::Ok(val) => Ok(val),
        vm::InterpretResult::RuntimeError(msg) => Err(eval::RuntimeError::new(msg).into()),
        vm::InterpretResult::CompileError => {
            Err(eval::RuntimeError::new("VM compile error".into()).into())
        }
    }
}

pub fn disassemble_bytecode(bytes: &[u8], name: &str) -> Result<String, ShaeError> {
    let chunk = chunk::Chunk::from_bytes(bytes).map_err(eval::RuntimeError::new)?;
    Ok(chunk.disassemble(name))
}

pub fn run_with_engine(source: &str, use_vm: bool) -> Result<value::Value, ShaeError> {
    if use_vm { run_vm(source) } else { run(source) }
}

pub fn run_file_with_engine<P: AsRef<std::path::Path>>(
    path: P,
    use_vm: bool,
) -> Result<value::Value, ShaeError> {
    if use_vm {
        run_file_vm(path)
    } else {
        run_file(path)
    }
}

pub fn disassemble_source(source: &str) -> Result<String, ShaeError> {
    let program = parse_source(source)?;
    let chunk = compiler::Compiler::new()
        .compile_program(&program)
        .map_err(eval::RuntimeError::new)?;
    Ok(chunk.disassemble("main"))
}

pub fn run_in_evaluator(source: &str, ev: &mut eval::Evaluator) -> Result<value::Value, ShaeError> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let val = ev.eval_program(&program)?;
    Ok(val)
}

pub fn parse_source(source: &str) -> Result<ast::Program, ShaeError> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    Ok(program)
}

pub use fmt::{format_program, format_source};
