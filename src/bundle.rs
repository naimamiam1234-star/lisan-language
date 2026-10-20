//! Bundle a parsed Lisan program and the runtime into an independent native executable.
//! This is a frozen-AST bundler, not LLVM/native code generation for Lisan expressions.
use crate::ast::{Expr, Function, Op, Stmt};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_GENERATED_SOURCE: usize = 12_000_000;
const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("ast.rs", include_str!("ast.rs")),
    ("lexer.rs", include_str!("lexer.rs")),
    ("parser.rs", include_str!("parser.rs")),
    ("runtime.rs", include_str!("runtime.rs")),
    ("modules.rs", include_str!("modules.rs")),
    ("source.rs", include_str!("source.rs")),
    ("diagnostic.rs", include_str!("diagnostic.rs")),
    ("syntax.rs", include_str!("syntax.rs")),
    ("types.rs", include_str!("types.rs")),
    ("parser/located.rs", include_str!("parser/located.rs")),
];

fn str_value(text: &str) -> String {
    format!("{:?}.to_string()", text)
}
fn vec_expr<T>(values: &[T], mut convert: impl FnMut(&T) -> String) -> String {
    format!(
        "vec![{}]",
        values
            .iter()
            .map(&mut convert)
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn boxed(expr: &Expr) -> String {
    format!("Box::new({})", emit_expr(expr))
}
fn emit_op(op: Op) -> &'static str {
    match op {
        Op::Add => "Op::Add",
        Op::Subtract => "Op::Subtract",
        Op::Multiply => "Op::Multiply",
        Op::Divide => "Op::Divide",
        Op::Modulo => "Op::Modulo",
        Op::Power => "Op::Power",
        Op::Equal => "Op::Equal",
        Op::NotEqual => "Op::NotEqual",
        Op::Less => "Op::Less",
        Op::LessEqual => "Op::LessEqual",
        Op::Greater => "Op::Greater",
        Op::GreaterEqual => "Op::GreaterEqual",
        Op::And => "Op::And",
        Op::Or => "Op::Or",
        Op::Not => "Op::Not",
        Op::Negate => "Op::Negate",
    }
}
fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => format!("Expr::Number({n:?})"),
        Expr::Text(t) => format!("Expr::Text({})", str_value(t)),
        Expr::Bool(b) => format!("Expr::Bool({b})"),
        Expr::Null => "Expr::Null".into(),
        Expr::List(items) => format!("Expr::List({})", vec_expr(items, emit_expr)),
        Expr::Map(pairs) => format!(
            "Expr::Map({})",
            vec_expr(pairs, |(key, value)| format!(
                "({}, {})",
                str_value(key),
                emit_expr(value)
            ))
        ),
        Expr::Variable(n) => format!("Expr::Variable({})", str_value(n)),
        Expr::Assign(n, expr) => format!("Expr::Assign({}, {})", str_value(n), boxed(expr)),
        Expr::AssignIndex(n, index, expr) => format!(
            "Expr::AssignIndex({}, {}, {})",
            str_value(n),
            boxed(index),
            boxed(expr)
        ),
        Expr::Index(base, index) => format!("Expr::Index({}, {})", boxed(base), boxed(index)),
        Expr::Unary(op, expr) => format!("Expr::Unary({}, {})", emit_op(*op), boxed(expr)),
        Expr::Binary(left, op, right) => format!(
            "Expr::Binary({}, {}, {})",
            boxed(left),
            emit_op(*op),
            boxed(right)
        ),
        Expr::Call(name, args) => format!(
            "Expr::Call({}, {})",
            str_value(name),
            vec_expr(args, emit_expr)
        ),
    }
}
fn emit_function(f: &Function) -> String {
    format!(
        "Function {{name: {}, params: {}, body: {}}}",
        str_value(&f.name),
        vec_expr(&f.params, |s| str_value(s)),
        vec_expr(&f.body, emit_stmt)
    )
}
fn emit_stmt(stmt: &Stmt) -> String {
    match stmt {
        Stmt::Import(_) => unreachable!("module loader resolves imports before bundling"),
        Stmt::Let(name, expr) => format!("Stmt::Let({}, {})", str_value(name), emit_expr(expr)),
        Stmt::Print(expr) => format!("Stmt::Print({})", emit_expr(expr)),
        Stmt::Expr(expr) => format!("Stmt::Expr({})", emit_expr(expr)),
        Stmt::Block(stmts) => format!("Stmt::Block({})", vec_expr(stmts, emit_stmt)),
        Stmt::If(cond, yes, no) => {
            let no = match no {
                None => "None".to_string(),
                Some(x) => format!("Some({})", vec_expr(x, emit_stmt)),
            };
            format!(
                "Stmt::If({}, {}, {})",
                emit_expr(cond),
                vec_expr(yes, emit_stmt),
                no
            )
        }
        Stmt::While(cond, body) => format!(
            "Stmt::While({}, {})",
            emit_expr(cond),
            vec_expr(body, emit_stmt)
        ),
        Stmt::ForEach(name, expr, body) => format!(
            "Stmt::ForEach({}, {}, {})",
            str_value(name),
            emit_expr(expr),
            vec_expr(body, emit_stmt)
        ),
        Stmt::Function(fun) => format!("Stmt::Function({})", emit_function(fun)),
        Stmt::Return(opt) => match opt {
            Some(e) => format!("Stmt::Return(Some({}))", emit_expr(e)),
            None => "Stmt::Return(None)".into(),
        },
        Stmt::Break => "Stmt::Break".into(),
        Stmt::Continue => "Stmt::Continue".into(),
    }
}

/// Emit a standalone Rust main that carries both the program's resolved AST and the runtime.
pub fn generate_rust_main(program: &[Stmt]) -> Result<String, String> {
    let ast = vec_expr(program, emit_stmt);
    let source = format!(
        r#"mod ast;
mod lexer;
mod modules;
mod parser;
mod runtime;
mod source;
mod diagnostic;
mod syntax;
mod types;
use ast::{{Expr, Function, Op, Stmt}};
fn main() {{
    let program: Vec<Stmt> = {ast};
    match runtime::Interpreter::default().run_program(&program) {{
        Ok(lines) => for line in lines {{ println!("{{}}", line); }},
        Err(error) => {{ eprintln!("خطأ: {{}}", error); std::process::exit(1); }}
    }}
}}
"#
    );
    if source.len() > MAX_GENERATED_SOURCE {
        return Err("البرنامج أكبر من الحجم المدعوم لحزم البرامج".into());
    }
    Ok(source)
}

struct TemporaryProject(PathBuf);
impl Drop for TemporaryProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Requires an installed Rust toolchain (cargo). The output binary embeds Lisan's interpreter.
pub fn build_executable(input: &Path, output: Option<&Path>) -> Result<PathBuf, String> {
    let program = crate::modules::load_file(input)?;
    let rust_main = generate_rust_main(&program)?;
    let name = input
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("lisan_app");
    let output_path = match output {
        Some(path) => path.to_path_buf(),
        None => {
            let mut path = input.with_file_name(format!("{name}-program"));
            if cfg!(windows) {
                path.set_extension("exe");
            }
            path
        }
    };
    let out_path = if output_path.is_absolute() {
        output_path
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(output_path)
    };
    // Avoid overwriting an existing binary (or the source file) without explicit handling.
    if out_path.exists() {
        return Err(format!("ملف الناتج موجود بالفعل: {}", out_path.display()));
    }
    let moment = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lisan_bundle_{}_{}", std::process::id(), moment));
    fs::create_dir(&dir).map_err(|e| format!("لا يمكن إنشاء مجلد البناء: {e}"))?;
    let _cleanup = TemporaryProject(dir.clone());
    fs::create_dir(dir.join("src")).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"lisan_standalone\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .map_err(|e| e.to_string())?;
    fs::write(dir.join("src/main.rs"), rust_main).map_err(|e| e.to_string())?;
    for (file, contents) in RUNTIME_SOURCES {
        let destination = dir.join("src").join(file);
        fs::create_dir_all(destination.parent().ok_or("مسار ملف الحزمة غير صالح")?)
            .map_err(|e| e.to_string())?;
        fs::write(destination, contents).map_err(|e| e.to_string())?;
    }
    let result = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--offline")
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("تعذر تشغيل Cargo. ثبّت Rust وCargo أولًا: {e}"))?;
    if !result.status.success() {
        let logs = String::from_utf8_lossy(&result.stderr);
        return Err(format!("فشل بناء برنامج لسان بواسطة Cargo:\n{logs}"));
    }
    let bin = dir
        .join("target")
        .join("release")
        .join(format!("lisan_standalone{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&bin, &out_path)
        .map_err(|e| format!("فشل نسخ البرنامج إلى '{}': {e}", out_path.display()))?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_rust_supports_all_major_ast_nodes() {
        let program = crate::parser::parse(crate::lexer::tokenize(r#"ليكن م = {"ع": [1, 2]}؛ دالة ح(س) { إذا (س > 0) { أرجع س؛ } وإلا { أرجع 0؛ } } لكل (س في م["ع"]) { اطبع(ح(س))؛ }"#).unwrap()).unwrap();
        let output = generate_rust_main(&program).unwrap();
        assert!(output.contains("Expr::Map"));
        assert!(output.contains("Stmt::Function"));
        assert!(output.contains("Stmt::ForEach"));
        assert!(output.contains("Stmt::If"));
    }
}
