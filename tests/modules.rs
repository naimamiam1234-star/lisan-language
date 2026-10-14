use lisan::{check_file, run, run_file};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn workspace() -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lisan_modules_{}_{}", std::process::id(), id));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn imports_functions_from_another_file() {
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/modules/main.lsn");
    assert_eq!(run_file(file).unwrap(), ["مربع ٧ = 49"]);
    check_file(file).unwrap();
}

#[test]
fn repeated_imports_are_only_executed_once() {
    let dir = workspace();
    fs::write(dir.join("part.lsn"), r#"اطبع("مرة")؛"#).unwrap();
    fs::write(dir.join("app.lsn"), r#"استورد "part.lsn"؛ استورد "part.lsn"؛"#).unwrap();
    assert_eq!(run_file(dir.join("app.lsn")).unwrap(), ["مرة"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cyclic_import_fails() {
    let dir = workspace();
    fs::write(dir.join("a.lsn"), r#"استورد "b.lsn"؛"#).unwrap();
    fs::write(dir.join("b.lsn"), r#"استورد "a.lsn"؛"#).unwrap();
    assert!(check_file(dir.join("a.lsn")).unwrap_err().contains("دورة استيراد"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nested_import_fails_on_check() {
    let dir = workspace();
    fs::write(dir.join("a.lsn"), r#"دالة حمل() { استورد "b.lsn"؛ }"#).unwrap();
    assert!(check_file(dir.join("a.lsn")).unwrap_err().contains("المستوى الأعلى"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn syntax_errors_include_filename() {
    let dir = workspace();
    fs::write(dir.join("broken.lsn"), "دع س = ؛").unwrap();
    assert!(check_file(dir.join("broken.lsn")).unwrap_err().contains("broken.lsn"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cannot_import_from_inline_source() {
    assert!(run(r#"استورد "module.lsn"؛"#).unwrap_err().contains("ملف .lsn"));
}

#[test]
fn absolute_path_import_fails() {
    let dir = workspace();
    let absolute = dir.join("target.lsn").to_string_lossy().replace('\\', "/");
    fs::write(dir.join("main.lsn"), format!("استورد \"{}\"؛", absolute)).unwrap();
    assert!(check_file(dir.join("main.lsn")).unwrap_err().contains("نسبيًا"));
    fs::remove_dir_all(dir).unwrap();
}
