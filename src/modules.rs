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

fn load_recursive(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    active: &mut Vec<PathBuf>,
) -> Result<Vec<Stmt>, String> {
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
                let imported = path
                    .parent()
                    .expect("canonical file has parent")
                    .join(imported);
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
            for entry in yes {
                validate_no_nested_imports(entry)?;
            }
            if let Some(no) = no {
                for entry in no {
                    validate_no_nested_imports(entry)?;
                }
            }
            return Ok(());
        }
        _ => &[],
    };
    for entry in nested {
        validate_no_nested_imports(entry)?;
    }
    Ok(())
}

/// Located native input; canonical import identity and source provenance survive
/// flattening. Language functions still share one scope, as in the interpreter.
#[derive(Debug, Clone)]
pub struct LoadedProgram {
    pub program: crate::syntax::Program,
    pub sources: crate::source::SourceMap,
}

pub fn load_program(path: &Path) -> Result<LoadedProgram, Vec<crate::diagnostic::Diagnostic>> {
    let mut sources = crate::source::SourceMap::default();
    let mut visited = HashSet::new();
    let mut active = Vec::new();
    match load_located(
        path,
        &mut visited,
        &mut active,
        &mut sources,
        crate::source::Span::default(),
    ) {
        Ok(statements) => Ok(LoadedProgram {
            program: crate::syntax::Program { statements },
            sources,
        }),
        Err(mut error) => {
            // A failed load cannot return its SourceMap; retain the rendered path
            // and scalar position in the diagnostic message for callers.
            error.message = error.render(&sources);
            Err(vec![error])
        }
    }
}

fn load_located(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    active: &mut Vec<PathBuf>,
    sources: &mut crate::source::SourceMap,
    site: crate::source::Span,
) -> Result<Vec<crate::syntax::Stmt>, crate::diagnostic::Diagnostic> {
    use crate::{diagnostic::Diagnostic, syntax::StmtKind};
    let failure = |msg: String| Diagnostic::new("E_IMPORT", site, msg);
    let path = fs::canonicalize(path)
        .map_err(|e| failure(format!("تعذر فتح '{}': {e}", path.display())))?;
    if active.contains(&path) {
        let chain = active
            .iter()
            .chain(std::iter::once(&path))
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(failure(format!("دورة استيراد: {chain}")));
    }
    if visited.contains(&path) {
        return Ok(vec![]);
    }
    if active.len() >= MAX_IMPORT_DEPTH {
        return Err(failure("تجاوز الحد الآمن لعمق استيراد الملفات".into()));
    }
    if fs::metadata(&path)
        .map_err(|e| failure(e.to_string()))?
        .len()
        > MAX_SOURCE_BYTES
    {
        return Err(failure(format!(
            "ملف '{}' تجاوز الحجم المسموح",
            path.display()
        )));
    }
    let source = fs::read_to_string(&path)
        .map_err(|e| failure(format!("تعذر قراءة '{}': {e}", path.display())))?;
    let id = sources.add(path.clone(), source.clone());
    let tokens = crate::lexer::tokenize_located(&source, id)?;
    let program = crate::parser::parse_located(tokens)?;
    active.push(path.clone());
    let mut result = vec![];
    for statement in program.statements {
        if let StmtKind::Import(relative) = &statement.kind {
            let imported = Path::new(relative);
            if relative.is_empty() || imported.is_absolute() {
                return Err(Diagnostic::new(
                    "E_IMPORT",
                    statement.span,
                    "استورد تقبل مسارًا نسبيًا غير فارغ فقط",
                ));
            }
            let imported = path
                .parent()
                .expect("canonical file has parent")
                .join(imported);
            result.extend(load_located(
                &imported,
                visited,
                active,
                sources,
                statement.span,
            )?);
        } else {
            no_nested_located_imports(&statement)?;
            result.push(statement);
        }
    }
    active.pop();
    visited.insert(path);
    Ok(result)
}

fn no_nested_located_imports(s: &crate::syntax::Stmt) -> Result<(), crate::diagnostic::Diagnostic> {
    use crate::syntax::StmtKind as S;
    let body = match &s.kind {
        S::Import(_) => {
            return Err(crate::diagnostic::Diagnostic::new(
                "E_IMPORT",
                s.span,
                "استورد مسموحة في المستوى الأعلى للملف فقط",
            ))
        }
        S::Block(b) | S::Unsafe(b) | S::While(_, b) | S::ForEach(_, _, b) => b,
        S::Function(f) | S::ExportFunction { function: f, .. } => &f.body,
        S::If(_, yes, no) => {
            for s in yes {
                no_nested_located_imports(s)?;
            }
            if let Some(no) = no {
                for s in no {
                    no_nested_located_imports(s)?;
                }
            }
            return Ok(());
        }
        _ => return Ok(()),
    };
    for s in body {
        no_nested_located_imports(s)?;
    }
    Ok(())
}
