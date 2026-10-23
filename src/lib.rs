pub mod ast;
pub mod bundle;
pub mod lexer;
pub mod modules;
pub mod native;
pub mod parser;
pub mod runtime;

pub use runtime::{Interpreter, Value};

pub fn run_file(path: impl AsRef<std::path::Path>) -> Result<Vec<String>, String> {
    Interpreter::default().run_file(path)
}

pub fn check_file(path: impl AsRef<std::path::Path>) -> Result<(), String> {
    modules::load_file(path.as_ref()).map(|_| ())
}

/// Checks whether this file can generate native LLVM IR without invoking Clang.
/// This is a capability check for the current backend, not a complete type checker.
pub fn check_native_file(path: impl AsRef<std::path::Path>) -> Result<(), String> {
    native::emit_file(path.as_ref()).map(|_| ())
}

pub fn run(source: &str) -> Result<Vec<String>, String> {
    Interpreter::default().run_source(source)
}

pub mod checker;
pub mod diagnostic;
pub mod ffi;
pub mod source;
pub mod syntax;
pub mod typed;
pub mod types;
