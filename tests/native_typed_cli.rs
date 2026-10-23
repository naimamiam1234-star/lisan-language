use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new(source: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "lisan typed عربية {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        fs::write(p.join("main.lsn"), source).unwrap();
        Self(p)
    }
    fn cli(&self, mode: &str, tail: &[&std::ffi::OsStr]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lisan"))
            .arg(mode)
            .arg(self.0.join("main.lsn"))
            .args(tail)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn clang(&self) -> Command {
        Command::new(std::env::var_os("LISAN_CLANG").unwrap_or_else(|| "clang".into()))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[test]
fn typed_source_compiles_and_runs_exact_values_and_functions() {
    let w = Workspace::new("دالة مضروب(س: ص32): ص32 { إذا (س <= 1) { أرجع 1؛ } وإلا { أرجع س * مضروب(س - 1)؛ } } ليكن كبير: ط64 = 18446744073709551615؛ اطبع(كبير)؛ اطبع(مضروب(5))؛ ليكن ص: ع32 = 1.5؛ اطبع(ص)؛ اطبع(ص > 1)؛");
    let o = w.cli("--test", &[]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(text(&o), "18446744073709551615\n120\n1.5\nصحيح\n");
}

#[test]
fn imported_typed_functions_and_diagnostics_keep_their_source() {
    let w = Workspace::new("استورد \"حساب.lsn\"؛ اطبع(جمع(20، 22))؛");
    let imported = w.0.join("حساب.lsn");
    fs::write(&imported, "دالة جمع(أ: ص32، ب: ص32): ص32 { أرجع أ + ب؛ }").unwrap();
    let o = w.cli("--test", &[]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(text(&o), "42\n");
    fs::write(&imported, "دالة جمع(أ: ص32، ب: ص32): ص32 {\nأرجع صحيح؛\n}").unwrap();
    let o = w.cli("--check", &[]);
    assert_eq!(o.status.code(), Some(1));
    let error = String::from_utf8_lossy(&o.stderr);
    assert!(error.contains("حساب.lsn:2:"), "{error}");
    assert!(error.contains("E_TYPE_MISMATCH"), "{error}");
}

#[test]
fn native_cli_links_a_real_c_object() {
    let w = Workspace::new("خارجي دالة زد(س: ص32): ص32 = \"c_add_two\"؛ غير_آمن { اطبع(زد(40))؛ }");
    let c = w.0.join("C adapter.c");
    let object = w.0.join("C adapter.o");
    fs::write(
        &c,
        "#include <stdint.h>\nint32_t c_add_two(int32_t x) { return x + 2; }\n",
    )
    .unwrap();
    let built = w
        .clang()
        .arg("-c")
        .arg(&c)
        .arg("-o")
        .arg(&object)
        .output()
        .unwrap();
    assert!(built.status.success(), "{built:?}");
    let binary = w.0.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let o = w.cli(
        "--native",
        &[
            "--output".as_ref(),
            binary.as_os_str(),
            "--link".as_ref(),
            object.as_os_str(),
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let out = Command::new(binary).output().unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out), "42\n");
}

#[test]
fn native_cli_emits_a_library_object_callable_from_c() {
    let w = Workspace::new("صدّر دالة جمع(أ: ص32، ب: ص32): ص32 = \"lisan_add\" { أرجع أ + ب؛ }");
    let object = w.0.join("library.o");
    let o = w.cli("--emit-object", &["--output".as_ref(), object.as_os_str()]);
    assert!(o.status.success(), "{o:?}");
    let c = w.0.join("host.c");
    let binary = w.0.join(format!("host{}", std::env::consts::EXE_SUFFIX));
    fs::write(&c, "#include <stdint.h>\nextern int32_t lisan_add(int32_t,int32_t);\nint main(void) { return lisan_add(20,22) == 42 ? 0 : 1; }\n").unwrap();
    let mut cmd = w.clang();
    cmd.arg(&c).arg(&object).arg("-o").arg(&binary);
    if cfg!(all(windows, target_env = "msvc")) {
        cmd.arg("-Xlinker").arg("legacy_stdio_definitions.lib");
    } else {
        cmd.arg("-lm");
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(Command::new(binary).status().unwrap().success());
}

#[test]
fn typed_runtime_errors_fail_without_interpreter_fallback() {
    let w = Workspace::new("ليكن س: ص8 = 127؛ اطبع(س + 1)؛");
    let o = w.cli("--test", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("خطأ"), "{o:?}");
    fs::write(w.0.join("main.lsn"), "ليكن س: ص8 = 128؛").unwrap();
    let o = w.cli("--check", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("E_TYPE_MISMATCH"),
        "{o:?}"
    );
}

#[test]
fn c_out_parameter_updates_a_lisan_scalar() {
    let w = Workspace::new("خارجي دالة املأ(ق: مؤشر<ص32>): فراغ = \"fill_value\"؛ ليكن س: ص32 = 0؛ غير_آمن { املأ(عنوان(س))؛ اطبع(س)؛ ليكن ق = عنوان(س)؛ خزن(ق، 45)؛ اطبع(تحميل(ق))؛ }");
    let c = w.0.join("out parameter.c");
    fs::write(
        &c,
        "#include <stdint.h>\nvoid fill_value(int32_t *p) { *p = 42; }\n",
    )
    .unwrap();
    let binary = w.0.join(format!("pointer{}", std::env::consts::EXE_SUFFIX));
    let o = w.cli(
        "--native",
        &[
            "--output".as_ref(),
            binary.as_os_str(),
            "--link".as_ref(),
            c.as_os_str(),
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let o = Command::new(binary).output().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert_eq!(text(&o), "42\n45\n");
}

#[test]
fn every_native_output_mode_preserves_imported_sources() {
    for mode in ["--native", "--emit-llvm", "--emit-object"] {
        let w = Workspace::new("استورد \"dependency.lsn\"؛");
        let dependency = w.0.join("dependency.lsn");
        let source = "صدّر دالة قيمة(): ص32 = \"lisan_value\" { أرجع 42؛ }";
        fs::write(&dependency, source).unwrap();
        let o = w.cli(mode, &["--output".as_ref(), dependency.as_os_str()]);
        assert_eq!(o.status.code(), Some(1), "{mode}: {o:?}");
        assert_eq!(fs::read_to_string(dependency).unwrap(), source);
    }
}

#[test]
fn uncreated_output_parent_cannot_hide_an_imported_source_alias() {
    for mode in ["--native", "--emit-llvm", "--emit-object"] {
        let w = Workspace::new("استورد \"dependency.lsn\"؛");
        let dependency = w.0.join("dependency.lsn");
        let source = "صدّر دالة قيمة(): ص32 = \"lisan_value\" { أرجع 42؛ }";
        fs::write(&dependency, source).unwrap();
        let output = w.0.join("missing/../dependency.lsn");
        let o = w.cli(mode, &["--output".as_ref(), output.as_os_str()]);
        assert_eq!(o.status.code(), Some(1), "{mode}: {o:?}");
        assert_eq!(fs::read_to_string(dependency).unwrap(), source);
    }
}

#[cfg(unix)]
#[test]
fn imported_source_output_symlink_is_preserved() {
    let w = Workspace::new("استورد \"dependency.lsn\"؛");
    let dependency = w.0.join("dependency.lsn");
    fs::write(
        &dependency,
        "صدّر دالة قيمة(): ص32 = \"lisan_value\" { أرجع 42؛ }",
    )
    .unwrap();
    let alias = w.0.join("output alias");
    std::os::unix::fs::symlink(&dependency, &alias).unwrap();
    for mode in ["--native", "--emit-llvm", "--emit-object"] {
        let o = w.cli(mode, &["--output".as_ref(), alias.as_os_str()]);
        assert_eq!(o.status.code(), Some(1), "{mode}: {o:?}");
        assert!(alias.symlink_metadata().unwrap().file_type().is_symlink());
    }
}
