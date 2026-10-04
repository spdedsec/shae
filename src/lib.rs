pub mod ast;
pub mod env;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod value;

use eval::{Evaluator, RuntimeError};
use lexer::LexerError;
use parser::ParserError;
use thiserror::Error;
use value::Value;

#[derive(Error, Debug, PartialEq)]
pub enum ShaeError {
    #[error("{0}")]
    Lexer(#[from] LexerError),

    #[error("{0}")]
    Parser(#[from] ParserError),

    #[error("{0}")]
    Runtime(#[from] RuntimeError),
}

/// Executes a Shae script and returns the result of execution.
pub fn run(source: &str) -> Result<Value, ShaeError> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let mut evaluator = Evaluator::new();
    let result = evaluator.eval_program(&program)?;
    Ok(result)
}

/// Runs a script against an existing persistent evaluator (useful for REPL).
pub fn run_in_evaluator(source: &str, evaluator: &mut Evaluator) -> Result<Value, ShaeError> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let result = evaluator.eval_program(&program)?;
    Ok(result)
}
