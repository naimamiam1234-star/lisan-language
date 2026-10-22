use std::{env, path::PathBuf, process::Command};

fn harness(sanitize: bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = env::temp_dir().join(format!("lisan-runtime-{}-{}", std::process::id(), sanitize));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("harness{}", env::consts::EXE_SUFFIX));
    let compiler = env::var("LISAN_CLANG").unwrap_or_else(|_| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "gcc".into()
        }
    });
    let mut cmd = Command::new(compiler);
    cmd.current_dir(&root).args([
        "-std=c11",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-g",
        "-Iruntime/common",
    ]);
    if sanitize {
        cmd.args([
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
            "-no-pie",
        ]);
    }
    let output = cmd
        .args([
            "tests/fixtures/managed/runtime_harness.c",
            "runtime/common/value.c",
            "runtime/host/allocator.c",
            "-o",
        ])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut run = Command::new(&exe);
    if sanitize {
        run.env(
            "ASAN_OPTIONS",
            format!(
                "detect_leaks={}",
                env::var("LISAN_SANITIZER_LEAKS").unwrap_or_else(|_| "1".into())
            ),
        );
    }
    let output = run.output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"A\0B");
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn managed_runtime_contract() {
    harness(false);
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn managed_runtime_sanitizers() {
    harness(true);
}
