use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Program(PathBuf);
impl Program {
    fn compile(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lisan-numeric-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let input = path.join("input.lsn");
        fs::write(&input, source).unwrap();
        lisan::native::build_native(
            &input,
            &path.join(format!("program{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
        Self(path)
    }
    fn run(&self) -> Output {
        Command::new(
            self.0
                .join(format!("program{}", std::env::consts::EXE_SUFFIX)),
        )
        .output()
        .unwrap()
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn division_and_remainder_by_both_zero_signs_fail() {
    for expression in ["1 / 0", "1 / -0", "1 % 0", "1 % -0"] {
        let result = Program::compile(&format!("اطبع({expression})؛")).run();
        assert_eq!(result.status.code(), Some(1), "{expression}: {result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("صفر"));
    }
}

#[test]
fn nonfinite_arithmetic_is_rejected_at_runtime() {
    for expression in [
        "(-1) ** 0.5",
        "10 ** 400",
        "1e308 + 1e308",
        "-1e308 - 1e308",
        "1e308 * 2",
        "1e308 / 1e-308",
    ] {
        let result = Program::compile(&format!("اطبع({expression})؛")).run();
        assert_eq!(result.status.code(), Some(1), "{expression}: {result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("منتهية"));
    }
}

#[test]
fn native_range_validates_before_entering_body() {
    for args in [
        "0.5، 2",
        "0، 2، 0.5",
        "100001",
        "9007199254740992، 9007199254740994",
        "0، 1، 0",
        "0، 1، -0",
    ] {
        let result = Program::compile(&format!(
            "لكل (س في مدى({args})) {{ اطبع(\"دخل\")؛ توقف؛ }}"
        ))
        .run();
        assert_eq!(result.status.code(), Some(1), "{args}: {result:?}");
        assert!(!String::from_utf8_lossy(&result.stdout).contains("دخل"));
    }
}

#[test]
fn finite_arithmetic_and_range_control_flow_still_work() {
    let program = Program::compile("اطبع(6 / 2)؛ اطبع(-5 % 2)؛ اطبع(0 ** 0)؛ لكل (س في مدى(5، 0، -2)) { إذا (س == 3) { تابع؛ } اطبع(س)؛ } لكل (س في مدى(3، 0، 1)) { اطبع(99)؛ }");
    let output = program.run();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .replace("\r\n", "\n"),
        "3\n-1\n1\n5\n1\n"
    );
}
