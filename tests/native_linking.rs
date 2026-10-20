use lisan::native::{compile_ir_with_options, NativeBuildOptions, NativeEmit};
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
        let path = std::env::temp_dir().join(format!(
            "lisan linking عربية {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("source.lsn"), "source sentinel").unwrap();
        Self(path)
    }
    fn build(&self, ir: &str, options: &NativeBuildOptions) -> Result<PathBuf, String> {
        compile_ir_with_options(
            ir,
            &self.0.join("source.lsn"),
            &self
                .0
                .join(format!("program{}", std::env::consts::EXE_SUFFIX)),
            options,
        )
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const MAIN: &str = "declare i32 @c_answer()\ndefine i32 @main() {\n%answer = call i32 @c_answer()\nret i32 %answer\n}\n";
#[test]
fn generated_llvm_calls_c_shim() {
    let w = Workspace::new();
    let shim = w.0.join("answer shim.c");
    fs::write(&shim, "int c_answer(void) { return 42; }").unwrap();
    let options = NativeBuildOptions {
        link_inputs: vec![shim],
        ..Default::default()
    };
    let executable = w.build(MAIN, &options).unwrap();
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
}
#[test]
fn function_only_object_links_into_c_main() {
    let w = Workspace::new();
    let object = w.0.join(if cfg!(windows) {
        "answer.obj"
    } else {
        "answer.o"
    });
    compile_ir_with_options(
        "define i32 @lisan_answer() { ret i32 42 }\n",
        &w.0.join("source.lsn"),
        &object,
        &NativeBuildOptions {
            emit: NativeEmit::Object,
            ..Default::default()
        },
    )
    .unwrap();
    let options = NativeBuildOptions {
        link_inputs: vec![object.clone()],
        ..Default::default()
    };
    let executable = w.build("declare i32 @lisan_answer()\ndefine i32 @main() {\n%answer = call i32 @lisan_answer()\nret i32 %answer\n}\n", &options).unwrap();
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
    let main = w.0.join("main.c");
    fs::write(
        &main,
        "extern int lisan_answer(void); int main(void) { return lisan_answer(); }",
    )
    .unwrap();
    let executable = w.0.join(format!("c-main{}", std::env::consts::EXE_SUFFIX));
    let result = Command::new(std::env::var_os("LISAN_CLANG").unwrap_or_else(|| "clang".into()))
        .arg(main)
        .arg(object)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
}
#[test]
fn invalid_link_inputs_and_object_options_preserve_files() {
    let w = Workspace::new();
    let source = w.0.join("source.lsn");
    let options = NativeBuildOptions {
        link_inputs: vec![w.0.join("missing.o")],
        ..Default::default()
    };
    assert!(w.build(MAIN, &options).is_err());
    let options = NativeBuildOptions {
        link_inputs: vec![source.clone()],
        ..Default::default()
    };
    assert!(compile_ir_with_options(MAIN, &source, &source, &options).is_err());
    let link_input = w.0.join("protected.o");
    fs::write(&link_input, "link input sentinel").unwrap();
    let options = NativeBuildOptions {
        link_inputs: vec![link_input.clone()],
        ..Default::default()
    };
    assert!(compile_ir_with_options(MAIN, &source, &link_input, &options).is_err());
    assert_eq!(
        fs::read_to_string(link_input).unwrap(),
        "link input sentinel"
    );
    for options in [
        NativeBuildOptions {
            emit: NativeEmit::Object,
            link_inputs: vec![source.clone()],
            ..Default::default()
        },
        NativeBuildOptions {
            emit: NativeEmit::Object,
            libraries: vec!["m".into()],
            ..Default::default()
        },
        NativeBuildOptions {
            emit: NativeEmit::Object,
            library_paths: vec![w.0.clone()],
            ..Default::default()
        },
    ] {
        assert!(w.build(MAIN, &options).is_err());
    }
    assert_eq!(fs::read_to_string(source).unwrap(), "source sentinel");
}
#[test]
fn missing_library_preserves_previous_output() {
    let w = Workspace::new();
    let output = w.0.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    fs::write(&output, "previous executable").unwrap();
    let options = NativeBuildOptions {
        library_paths: vec![w.0.clone()],
        libraries: vec!["lisan_nonexistent_library_937529".into()],
        ..Default::default()
    };
    assert!(w
        .build("define i32 @main() { ret i32 0 }\n", &options)
        .is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "previous executable");
    assert_eq!(fs::read_dir(&w.0).unwrap().count(), 2);
}
