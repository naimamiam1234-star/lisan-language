//! File-based imports with relative paths, deduplication, and cycle detection.
use crate::ast::Stmt;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_IMPORT_DEPTH: usize = 32;
const MAX_SOURCE_BYTES: u64 = 2_000_000;

pub fn load_file(path: &Path) -> Result<Vec<Stmt>, String> {
    let mut visited = HashSet::new();
    let mut active = Vec::new();
    load_recursive(path, &mut visited, &mut active)
}

fn load_recursive(path: &Path, visited: &mut HashSet<PathBuf>, active: &mut Vec<PathBuf>) -> Result<Vec<Stmt>, String> {
    let path = fs::canonicalize(path)
        .map_err(|e| format!("لا يمكن العثور على ملف لسان '{}': {}", path.display(), e))?;
    if active.contains(&path) {
        return Err(format!("اكتُشفت دورة استيراد للملف '{}'", path.display()));
    }
    if visited.contains(&path) {
        return Ok(Vec::new());
    }
    if active.len() >= MAX_IMPORT_DEPTH {
        return Err("تجاوز الحد الآمن لعمق استيراد الملفات".into());
    }
    let metadata = fs::metadata(&path)
        .map_err(|e| format!("تعذر قراءة بيانات الملف '{}': {}", path.display(), e))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(format!("ملف لسان '{}' تجاوز الحجم المسموح", path.display()));
    }
    let source = fs::read_to_string(&path)
        .map_err(|e| format!("تعذر قراءة ملف UTF-8 '{}': {}", path.display(), e))?;
    let tokens = crate::lexer::tokenize(&source)
        .map_err(|e| format!("في الملف '{}': {}", path.display(), e))?;
    let statements = crate::parser::parse(tokens)
        .map_err(|e| format!("في الملف '{}': {}", path.display(), e))?;
    active.push(path.clone());
    let mut merged = Vec::new();
    for statement in statements {
        match statement {
            Stmt::Import(relative) => {
                let imported = Path::new(&relative);
                if relative.is_empty() || imported.is_absolute() {
                    return Err("استورد تقبل مسارًا نسبيًا غير فارغ فقط".into());
                }
                let imported = path.parent().expect("canonical file has parent").join(imported);
                merged.extend(load_recursive(&imported, visited, active)?);
            }
            other => {
                validate_no_nested_imports(&other)?;
                merged.push(other);
            }
        }
    }
    active.pop();
    visited.insert(path);
    Ok(merged)
}

fn validate_no_nested_imports(stmt: &Stmt) -> Result<(), String> {
    let nested: &[Stmt] = match stmt {
        Stmt::Import(_) => return Err("استورد مسموحة في المستوى الأعلى للملف فقط".into()),
        Stmt::Block(body) | Stmt::While(_, body) | Stmt::ForEach(_, _, body) => body,
        Stmt::Function(function) => &function.body,
        Stmt::If(_, yes, no) => {
            for entry in yes { validate_no_nested_imports(entry)?; }
            if let Some(no) = no { for entry in no { validate_no_nested_imports(entry)?; } }
            return Ok(());
        }
        _ => &[],
    };
    for entry in nested { validate_no_nested_imports(entry)?; }
    Ok(())
}
