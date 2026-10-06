pub mod ast;
pub mod builtins;
pub mod env;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod suggest;
pub mod token;
pub mod value;
pub mod opcode;
pub mod chunk;
pub mod compiler;
pub mod vm;
pub mod linter;

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

pub fn run_in_evaluator(source: &str, ev: &mut eval::Evaluator) -> Result<value::Value, ShaeError> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let val = ev.eval_program(&program)?;
    Ok(val)
}
