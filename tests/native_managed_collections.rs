use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn normalized(mut output: std::process::Output) -> std::process::Output {
    let mut bytes = Vec::with_capacity(output.stdout.len());
    for (i, &byte) in output.stdout.iter().enumerate() {
        if byte != b'\r' || output.stdout.get(i + 1) != Some(&b'\n') {
            bytes.push(byte);
        }
    }
    output.stdout = bytes;
    output
}
fn run(source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!(
        "lisan-text-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("input.lsn");
    fs::write(&src, source).unwrap();
    let exe = dir.join(format!("run{}", std::env::consts::EXE_SUFFIX));
    lisan::native::build_native(&src, &exe).unwrap();
    let out = normalized(Command::new(exe).output().unwrap());
    fs::remove_dir_all(dir).unwrap();
    out
}
fn checked(source: &str) -> lisan::typed::TypedProgram {
    lisan::checker::check(
        &lisan::parser::parse_located(
            lisan::lexer::tokenize_located(source, lisan::source::SourceId(0)).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}
struct Instrumented(std::path::PathBuf);
impl Instrumented {
    fn new(source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lisan-text-count-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let ir = lisan::native::typed_codegen::generate(&checked(source)).unwrap();
        fs::write(dir.join("program.ll"), ir).unwrap();
        fs::write(
            dir.join("value.h"),
            include_str!("../runtime/common/value.h"),
        )
        .unwrap();
        fs::write(
            dir.join("value.c"),
            include_str!("../runtime/common/value.c"),
        )
        .unwrap();
        fs::write(dir.join("counter.c"), COUNTER).unwrap();
        let mut command =
            Command::new(std::env::var_os("LISAN_CLANG").unwrap_or_else(|| "clang".into()));
        #[cfg(all(windows, target_env = "msvc"))]
        command.arg("-Xlinker").arg("legacy_stdio_definitions.lib");
        let out = command
            .arg("-std=c11")
            .arg("-O2")
            .arg(dir.join("program.ll"))
            .arg(dir.join("value.c"))
            .arg(dir.join("counter.c"))
            .arg("-o")
            .arg(dir.join(format!("run{}", std::env::consts::EXE_SUFFIX)))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Self(dir)
    }
    fn run(&self, fail: usize) -> std::process::Output {
        normalized(
            Command::new(self.0.join(format!("run{}", std::env::consts::EXE_SUFFIX)))
                .env("LISAN_TEST_FAIL", fail.to_string())
                .output()
                .unwrap(),
        )
    }
}
impl Drop for Instrumented {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const COUNTER: &str = r#"
#include "value.h"
#include <stdlib.h>
#include <stdio.h>
static size_t calls,live,fail;
static void report(void){fprintf(stderr,"LIVE=%zu CALLS=%zu\n",live,calls);if(live) abort();}
static void *allocate(void *state,size_t size,size_t alignment){(void)state;(void)alignment;calls++;if(calls==fail)return NULL;void *p=malloc(size);if(p)live++;return p;}
static void release(void *state,void *p,size_t size,size_t alignment){(void)state;(void)size;(void)alignment;if(p){live--;free(p);}}
LisanAllocator lisan_rt_host_allocator(void){const char *env=getenv("LISAN_TEST_FAIL");fail=env?(size_t)strtoull(env,NULL,10):0;atexit(report);return (LisanAllocator){NULL,allocate,release};}
void check_live(uint64_t expected){if(live!=expected){fprintf(stderr,"CHECK expected=%llu live=%zu\n",(unsigned long long)expected,live);abort();}}
"#;
fn assert_clean(out: &std::process::Output) {
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("LIVE=0 "),
        "{out:?}"
    );
}
#[test]
fn list_aliases_context_and_scalar_types() {
    let out = run("متغير أ: قائمة<ص32> = [1, 2]؛ متغير ب = أ؛ ب[0] = 9؛ أضف(ب, 3)؛ متغير خ: قائمة<ص32> = []؛ أضف(خ, 7)؛ اطبع(أ[0])؛ اطبع(ب[0])؛ اطبع(طول(ب))؛ اطبع(خ[0])؛ اطبع(أ == [1, 2])؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n9\n3\n7\nصحيح\n");
    for ty in [
        "ص8", "ص16", "ص32", "ص64", "ط8", "ط16", "ط32", "ط64", "ع32", "ع64",
    ] {
        let out = run(&format!(
            "متغير أ: قائمة<{ty}> = [1, 2]؛ أ[0] = 3؛ أضف(أ, 4)؛ اطبع(أ[0])؛ اطبع(أ[2])؛"
        ));
        assert!(out.status.success(), "{ty}: {out:?}");
        assert_eq!(out.stdout, b"3\n4\n");
    }
    let out = run("متغير أ: قائمة<منطقي> = [صحيح, خطأ]؛ أ[1] = صحيح؛ اطبع(أ[1])؛ متغير ب = [2, 3]؛ اطبع(ب[0])؛");
    assert!(out.status.success());
    assert_eq!(out.stdout, "صحيح\n2\n".as_bytes());
}
#[test]
fn list_text_functions_recursion_and_evaluation_order() {
    let out = run("دالة نسخ(س: قائمة<نص>, ن: ص32): قائمة<نص> { إذا (ن == 0) { أضف(س, \"ج\")؛ أرجع س؛ } أرجع نسخ(س, ن - 1)؛ } متغير أ: قائمة<نص> = [\"أ\", \"ب\"]؛ متغير ب = نسخ(أ, 3)؛ ب[0] = ب[0] + \"د\"؛ اطبع(أ[0])؛ اطبع(ب[0])؛ اطبع(ب[2])؛ اطبع(طول(أ))؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, "أ\nأد\nج\n2\n".as_bytes());
    let out=run("دالة أول(أ: قائمة<ص32>, ب: قائمة<ص32>): ص32 { أرجع أ[0]؛ } متغير أ: قائمة<ص32> = [1]؛ اطبع(أول(أ, أ = [2]))؛ أ[0] = (أ = [9])[0]؛ اطبع(أ[0])؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"1\n9\n");
}
#[test]
fn list_oom_and_active_frames_cleanup() {
    let p=Instrumented::new("دالة ف(أ: قائمة<نص>, ن: ص32): قائمة<نص> { متغير ب = أ؛ أضف(ب, \"ب\" + \"ج\")؛ إذا (ن == 0) { ب[0] = \"د\"؛ أرجع ب؛ } أرجع ف(ب, ن - 1)؛ } متغير أ: قائمة<نص> = [\"أ\"]؛ متغير ب = ف(أ, 3)؛ اطبع(ب[1])؛");
    let good = p.run(0);
    assert!(good.status.success(), "{good:?}");
    assert_clean(&good);
    let err = String::from_utf8_lossy(&good.stderr);
    let calls: usize = err.split("CALLS=").nth(1).unwrap().trim().parse().unwrap();
    assert!(calls > 10);
    for fail in 1..=calls {
        let out = p.run(fail);
        assert_eq!(out.status.code(), Some(1), "fail={fail} {out:?}");
        assert_clean(&out);
    }
    for index in ["-1", "0.5", "2"] {
        let p = Instrumented::new(&format!("متغير أ: قائمة<نص> = [\"أ\"]؛ اطبع(أ[{index}])؛"));
        let out = p.run(0);
        assert_eq!(out.status.code(), Some(1));
        assert_clean(&out);
    }
}
#[test]
fn unsupported_lists_remain_diagnostic() {
    for source in [
        "متغير أ = []؛",
        "متغير أ: قائمة<ص32> = [1, صحيح]؛",
        "متغير أ: قائمة<قائمة<ص32>> = [[1]]؛",
        "متغير أ: قائمة<مؤشر<ص32>> = []؛",
        "خارجي دالة ف(أ: قائمة<ص32>): فراغ = \"f\"؛",
        "خارجي دالة ف(أ: مؤشر<قائمة<ص32>>): فراغ = \"f\"؛",
    ] {
        let syntax = lisan::parser::parse_located(
            lisan::lexer::tokenize_located(source, lisan::source::SourceId(0)).unwrap(),
        )
        .unwrap();
        assert!(lisan::checker::check(&syntax).is_err(), "{source}");
    }
}

#[test]
fn list_repeated_scopes_loop_exits_and_text_item_snapshots() {
    let p=Instrumented::new("خارجي دالة تحقق(ن: ط64): فراغ = \"check_live\"؛ متغير أ: قائمة<نص> = [\"أ\"]؛ متغير ن: ص32 = 0؛ طالما (ن < 2000) { { متغير ب = أ؛ ب[0] = ب[0] + \"ب\"؛ أضف(ب, \"ج\")؛ } غير_آمن { تحقق(2)؛ } ن = ن + 1؛ إذا (ن < 1999) { تابع؛ } توقف؛ } غير_آمن { تحقق(2)؛ } متغير س: نص = \"a\"؛ متغير ج: قائمة<نص> = [س, س = \"b\"]؛ اطبع(ج[0])؛ اطبع(ج[1])؛");
    let out = p.run(0);
    assert!(out.status.success(), "{out:?}");
    assert_clean(&out);
    assert_eq!(out.stdout, b"a\nb\n");
}
#[test]
fn list_bounds_are_located_and_recursive_failure_releases() {
    let out = run("متغير أ: قائمة<ص32> = [1]؛\nاطبع(أ[4])؛");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("input.lsn:2:"),
        "{out:?}"
    );
    let p=Instrumented::new("دالة ف(أ: قائمة<نص>, ن: ص32): قائمة<نص> { متغير ب = أ؛ أضف(ب, \"ج\")؛ إذا (ن == 0) { ب[99] = \"د\"؛ أرجع ب؛ } أرجع ف(ب, ن - 1)؛ } متغير أ: قائمة<نص> = [\"أ\"]؛ متغير ب = ف(أ, 5)؛");
    let out = p.run(0);
    assert_eq!(out.status.code(), Some(1));
    assert_clean(&out);
}
#[test]
fn direct_ir_rejects_list_operator_and_scalar_result_confusion() {
    use lisan::{
        ast::Op,
        typed::{TypedExprKind as E, TypedStmtKind as S},
        types::Type,
    };
    let mut p = checked("متغير أ: قائمة<ص32> = [1]؛ اطبع(أ == أ)؛");
    if let S::Print(e) = &mut p.main[1].kind {
        if let E::Binary(_, op, _) = &mut e.kind {
            *op = Op::Add;
        }
    }
    assert!(lisan::native::typed_codegen::generate(&p).is_err());
    let mut p = checked("متغير أ: قائمة<ص32> = [1]؛ اطبع(أ[0])؛");
    if let S::Print(e) = &mut p.main[1].kind {
        e.ty = Type::Text;
    }
    assert!(lisan::native::typed_codegen::generate(&p).is_err());
}

#[test]
fn list_numeric_literals_use_homogeneous_nonliteral_context() {
    let out =
        run("متغير ن: ص16 = 4؛ متغير أ = [1, ن]؛ متغير ب: قائمة<ص16> = أ؛ اطبع(ب[0])؛ اطبع(ب[1])؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"1\n4\n");
}

#[test]
fn mutation_snapshots_target_before_index_and_rhs_side_effects() {
    let out=run("متغير أ: قائمة<ص32> = [1, 2]؛ أ[0] = (أ = [9, 8, 7])[0]؛ اطبع(طول(أ))؛ اطبع(أ[0])؛ اطبع(أ[1])؛ أ[طول(أ = [5, 6, 7]) - 3] = 4؛ اطبع(طول(أ))؛ اطبع(أ[0])؛ اطبع(أ[1])؛ أ = [1]؛ أضف(أ, (أ = [9, 8])[0])؛ اطبع(طول(أ))؛ اطبع(أ[0])؛ اطبع(أ[1])؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"2\n9\n2\n2\n4\n2\n2\n1\n9\n");
}

#[test]
fn declared_list_builtin_names_keep_function_resolution() {
    let out=run("دالة أضف(أ: ص32, ب: ص32): ص32 { أرجع أ + ب؛ } دالة طول(أ: ص32): ص32 { أرجع أ + 1؛ } متغير أ: قائمة<ص32> = [1]؛ اطبع(أضف(2, 3))؛ اطبع(طول(4))؛ اطبع(أ[0])؛");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"5\n5\n1\n");
}
