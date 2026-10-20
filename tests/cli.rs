use std::{fs, path::PathBuf, process::{Command, Output}, sync::atomic::{AtomicU64, Ordering}, time::{Duration, Instant}};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lisan cli عربية {} {}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap(); fs::create_dir(path.join("temporary files")).unwrap(); Self(path)
    }
    fn source(&self, source: &str) -> PathBuf { let p=self.0.join("برنامج اختبار.lsn"); fs::write(&p, source).unwrap(); p }
    fn run(&self, args: &[&std::ffi::OsStr], no_clang: bool) -> Output {
        let mut cmd=Command::new(env!("CARGO_BIN_EXE_lisan"));
        cmd.env("TMPDIR", self.0.join("temporary files")).env("TMP", self.0.join("temporary files")).env("TEMP", self.0.join("temporary files"));
        cmd.args(args).current_dir(&self.0).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if no_clang { cmd.env("PATH", &self.0).env("LISAN_CLANG", self.0.join("absent-clang")); }
        let mut child=cmd.spawn().unwrap(); let start=Instant::now();
        loop { if child.try_wait().unwrap().is_some() { let output = child.wait_with_output().unwrap(); if args.first() == Some(&flag("--test")) { assert_eq!(fs::read_dir(self.0.join("temporary files")).unwrap().count(), 0, "native test temporary directory leaked"); } return output; }
            if start.elapsed()>Duration::from_secs(45) { child.kill().unwrap(); let out=child.wait_with_output().unwrap(); panic!("CLI timeout: {:?}", out); }
            std::thread::sleep(Duration::from_millis(10)); }
    }
}
impl Drop for Workspace { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }
fn flag(s:&str)->&std::ffi::OsStr { s.as_ref() }
#[test]
fn native_check_rejects_legacy_dictionary_and_bad_assignment() {
    let w=Workspace::new();
    for source in ["ليكن س = {\"مفتاح\": 5}؛", "ليكن س = 2؛ س = صحيح؛", "اطبع(مجهول)؛"] {
        let p=w.source(source); let o=w.run(&[flag("--check"),p.as_os_str()],true); assert_eq!(o.status.code(),Some(1),"{:?}",o);
    }
}
#[test]
fn native_check_does_not_require_clang() { let w=Workspace::new(); let p=w.source("اطبع(42)؛"); assert!(w.run(&[flag("--check"),p.as_os_str()],true).status.success()); }
#[test]
fn native_test_forwards_output_and_leaves_no_artifacts() {
    let w=Workspace::new(); let p=w.source("اطبع(\"مرحبا\")؛ تحقق(صحيح)؛");
    let before=fs::read_dir(&w.0).unwrap().count(); let o=w.run(&[flag("--test"),p.as_os_str()],false);
    assert!(o.status.success(),"{:?}",o); assert_eq!(String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),"مرحبا\n"); assert_eq!(fs::read_dir(&w.0).unwrap().count(),before);
}
#[test]
fn native_test_rejects_dynamic_features_and_assertions() {
    let w=Workspace::new();
    for source in ["تحقق(خطأ)؛", "ليكن س = {\"مفتاح\": 5}؛"] { let p=w.source(source); assert_eq!(w.run(&[flag("--test"),p.as_os_str()],false).status.code(),Some(1)); }
}
#[test]
fn native_test_requires_clang() { let w=Workspace::new(); let p=w.source("اطبع(42)؛"); assert_eq!(w.run(&[flag("--test"),p.as_os_str()],true).status.code(),Some(1)); }
#[test]
fn explicit_legacy_modes_work() {
    let w=Workspace::new(); let p=w.source("ليكن س = {\"مفتاح\": 5}؛ اطبع(س[\"مفتاح\"])؛");
    for mode in ["--check-legacy","--test-legacy","--interpret"] { assert!(w.run(&[flag(mode),p.as_os_str()],true).status.success(),"{}",mode); }
    assert_eq!(w.run(&[flag("-e"),flag("اطبع(7)؛")],true).stdout,b"7\n");
}
#[test]
fn help_version_and_invalid_arguments() {
    let w=Workspace::new();
    for mode in ["--help","--version"] { assert!(w.run(&[flag(mode)],true).status.success()); }
    for args in [vec!["--test"],vec!["--native"],vec!["--check","a","extra"],vec!["--unknown"],vec!["--native","a","--wrong","b"],vec!["--build"]] { let a:Vec<_>=args.iter().map(|s|flag(s)).collect(); assert_eq!(w.run(&a,true).status.code(),Some(2),"{:?}",args); }
}
#[test]
fn native_build_emit_and_default_commands() {
    let w=Workspace::new(); let p=w.source("اطبع(9)؛"); let ir=w.0.join("ناتج test.ll"); let binary=w.0.join(if cfg!(windows){"native test.exe"}else{"native test"});
    assert!(w.run(&[flag("--emit-llvm"),p.as_os_str(),flag("--output"),ir.as_os_str()],true).status.success());
    assert!(fs::read_to_string(ir).unwrap().contains("define i32 @main"));
    assert!(w.run(&[flag("--native"),p.as_os_str(),flag("--output"),binary.as_os_str()],false).status.success());
    assert_eq!(String::from_utf8(Command::new(binary).output().unwrap().stdout).unwrap().replace("\r\n", "\n"),"9\n");
    assert!(w.run(&[p.as_os_str()],false).status.success());
}

#[test]
fn build_is_explicit_legacy_and_runs_dictionary() {
    let w=Workspace::new();
    let p=w.source("ليكن س = {\"مفتاح\": 5}؛ اطبع(س[\"مفتاح\"])؛");
    let binary=w.0.join(if cfg!(windows) { "legacy bundle.exe" } else { "legacy bundle" });
    let output=w.run(&[flag("--build"),p.as_os_str(),flag("--output"),binary.as_os_str()],false);
    assert!(output.status.success(), "{:?}",output);
    let output=Command::new(binary).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().replace("\r\n", "\n"),"5\n");
}

#[cfg(unix)]
#[test]
fn native_check_accepts_non_utf8_file_paths() {
    use std::os::unix::ffi::OsStringExt;
    let w=Workspace::new(); let path=w.0.join(std::ffi::OsString::from_vec(b"source-\xff.lsn".to_vec()));
    fs::write(&path,"اطبع(3)؛").unwrap();
    let output=w.run(&[flag("--check"),path.as_os_str()],true);
    assert!(output.status.success(),"{:?}",output);
}
