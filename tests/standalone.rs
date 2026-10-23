use lisan::bundle::{build_executable, generate_rust_main};
use lisan::modules;
use std::fs;
use std::process::Command;

#[test]
fn emits_valid_program_code_for_imported_app() {
    let source = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/modules/main.lsn");
    let program = modules::load_file(std::path::Path::new(source)).unwrap();
    let emitted = generate_rust_main(&program).unwrap();
    assert!(emitted.contains("مربع 7 = "));
    assert!(!emitted.contains("Stmt::Import"));
}

#[test]
fn compiles_and_executes_legacy_frozen_ast_binary() {
    let name = format!("lisan_standalone_test_{}_{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let dir = std::env::temp_dir().join(name);
    fs::create_dir(&dir).unwrap();
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/modules/main.lsn");
    let result = dir.join(format!("standalone{}", std::env::consts::EXE_SUFFIX));
    build_executable(&input, Some(&result)).unwrap();
    let output = Command::new(&result).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "مربع 7 = 49");
    assert!(build_executable(&input, Some(&result)).unwrap_err().contains("موجود"));
    fs::remove_dir_all(dir).unwrap();
}
