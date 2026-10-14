pub mod ast;
pub mod lexer;
pub mod parser;
pub mod runtime;

pub use runtime::{Interpreter, Value};

pub fn run(source: &str) -> Result<Vec<String>, String> {
    Interpreter::default().run_source(source)
}
