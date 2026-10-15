pub mod ast;
pub mod bundle;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod runtime;

pub use runtime::{Interpreter, Value};

pub fn run_file(path: impl AsRef<std::path::Path>) -> Result<Vec<String>, String> {
    Interpreter::default().run_file(path)
}

pub fn check_file(path: impl AsRef<std::path::Path>) -> Result<(), String> {
    modules::load_file(path.as_ref()).map(|_| ())
}

pub fn run(source: &str) -> Result<Vec<String>, String> {
    Interpreter::default().run_source(source)
}
