use std::path::PathBuf;

#[test]
fn native_ir_contains_no_interpreter() {
    let path=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/native.lsn");
    let ir=lisan::native::emit_file(&path).unwrap();
    assert!(ir.contains("define double @lisan_fn_0"));
    assert!(ir.contains("define i32 @main"));
    assert!(ir.contains("br i1"));
    assert!(!ir.contains("Interpreter"));
    assert!(!ir.contains("run_source"));
}

#[test]
fn native_rejects_dynamic_list_with_clear_message() {
    let ast=lisan::parser::parse(lisan::lexer::tokenize("اطبع([1،2])؛").unwrap()).unwrap();
    assert!(lisan::native::generate_llvm(&ast).unwrap_err().contains("لم تُنفّذ"));
}

#[cfg(target_os="linux")]
#[test]
fn builds_and_runs_real_machine_binary() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        eprintln!("تخطٍ: أداة Clang غير متوفرة");return;
    }
    let base=std::env::temp_dir().join(format!("lisan-native-test-{}",std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let executable=base.join("lisan-native-test");
    let source=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/native.lsn");
    lisan::native::build_native(&source,&executable).unwrap();
    let result=std::process::Command::new(&executable).output().unwrap();
    assert!(result.status.success());
    assert_eq!(String::from_utf8(result.stdout).unwrap(),"0\n1\n9\n16\nانتهى\n");
    let _=std::fs::remove_dir_all(base);
}
