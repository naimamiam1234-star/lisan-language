use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "lisan build عربية {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        fs::write(p.join("input.lsn"), "اطبع(42)؛").unwrap();
        Self(p)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_builder_never_overwrites_its_source() {
    let w = Workspace::new();
    let source = w.0.join("input.lsn");
    let before = fs::read(&source).unwrap();
    assert!(lisan::native::build_native(&source, &source).is_err());
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn compiler_override_failure_preserves_previous_output() {
    let w = Workspace::new();
    let output = w.0.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    fs::write(&output, b"previous build").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_lisan"))
        .arg("--native")
        .arg(w.0.join("input.lsn"))
        .arg("--output")
        .arg(&output)
        .env("LISAN_CLANG", w.0.join("missing compiler"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(output).unwrap(), b"previous build");
    assert_eq!(fs::read_dir(&w.0).unwrap().count(), 2);
}

#[test]
fn emit_llvm_never_overwrites_its_source() {
    let w = Workspace::new();
    let source = w.0.join("input.lsn");
    let before = fs::read(&source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_lisan"))
        .arg("--emit-llvm")
        .arg(&source)
        .arg("--output")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn linker_failure_preserves_previous_output_and_cleans_build_directory() {
    let w = Workspace::new();
    let output = w.0.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    fs::write(&output, b"previous build").unwrap();
    let ir = "declare void @lisan_missing_link_symbol()\ndefine i32 @main() {\ncall void @lisan_missing_link_symbol()\nret i32 0\n}\n";
    let error = lisan::native::compile_ir(ir, &w.0.join("input.lsn"), &output).unwrap_err();
    assert!(error.contains("فشلت الترجمة الأصلية"), "{error}");
    assert_eq!(fs::read(output).unwrap(), b"previous build");
    assert_eq!(fs::read_dir(&w.0).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn predictable_old_temporary_symlink_is_never_followed() {
    let w = Workspace::new();
    let sentinel = w.0.join("sentinel.txt");
    fs::write(&sentinel, b"do not modify").unwrap();
    let output = w.0.join("program");
    let old_temp =
        w.0.join(format!(".lisan-ir-{}-program.ll", std::process::id()));
    std::os::unix::fs::symlink(&sentinel, &old_temp).unwrap();
    lisan::native::build_native(&w.0.join("input.lsn"), &output).unwrap();
    assert_eq!(fs::read(sentinel).unwrap(), b"do not modify");
    assert!(old_temp
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn concurrent_builds_publish_only_complete_executables() {
    let w = Workspace::new();
    let output = w.0.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let source = w.0.join("input.lsn");
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let barrier = &barrier;
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let source = &source;
                let output = &output;
                scope.spawn(move || {
                    barrier.wait();
                    lisan::native::build_native(source, output)
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
    });
    let result = Command::new(&output).output().unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout)
            .unwrap()
            .replace("\r\n", "\n"),
        "42\n"
    );
    assert_eq!(fs::read_dir(&w.0).unwrap().count(), 2);
}
