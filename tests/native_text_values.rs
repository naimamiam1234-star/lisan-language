use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
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
    let out = Command::new(exe).output().unwrap();
    fs::remove_dir_all(dir).unwrap();
    out
}
#[test]
fn owned_unicode_aliases_and_scopes() {
    let o=run("متغير س: نص = \"أ🙂\"؛ متغير ع: نص = س؛ س = س + \"ب\"؛ س = س؛ { متغير ج: نص = س[1] + ع[0]؛ اطبع(ج)؛ } اطبع(طول(س))؛ اطبع(ع)؛ اطبع(س == ع)؛");
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "🙂أ\n3\nأ🙂\nخطأ\n"
    );
}
#[test]
fn embedded_nul_bytes() {
    let o = run("متغير س: نص = \"a\0ب\"؛ اطبع(س)؛ اطبع(طول(س))؛ اطبع(س[1])؛");
    assert!(o.status.success());
    assert_eq!(
        o.stdout,
        if cfg!(windows) {
            b"a\0\xd8\xa8\r\n3\r\n\0\r\n".to_vec()
        } else {
            b"a\0\xd8\xa8\n3\n\0\n".to_vec()
        }
    );
}
#[test]
fn bad_indices_are_located() {
    for index in ["-1", "0.5", "18446744073709551616.0", "2"] {
        let o = run(&format!("متغير س: نص = \"أ\"؛\nاطبع(س[{index}])؛"));
        assert_eq!(o.status.code(), Some(1));
        let t = String::from_utf8_lossy(&o.stdout);
        assert!(t.contains("input.lsn:2:"), "{t}");
    }
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
        Command::new(self.0.join(format!("run{}", std::env::consts::EXE_SUFFIX)))
            .env("LISAN_TEST_FAIL", fail.to_string())
            .output()
            .unwrap()
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
fn generated_program_cleans_success_scopes_temporaries_and_every_oom() {
    let source="خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"a\"؛ غير_آمن { تحقق(1)؛ } { متغير ب: نص = أ + \"ب\"؛ غير_آمن { تحقق(2)؛ } اطبع(ب[1] + أ)؛ غير_آمن { تحقق(2)؛ } } غير_آمن { تحقق(1)؛ } أ = أ + \"c\"؛ أ = أ؛ غير_آمن { تحقق(1)؛ } اطبع(أ + (أ = \"d\"))؛ غير_آمن { تحقق(1)؛ }";
    let p = Instrumented::new(source);
    let good = p.run(0);
    assert!(good.status.success(), "{good:?}");
    assert_clean(&good);
    let err = String::from_utf8_lossy(&good.stderr);
    let calls: usize = err.split("CALLS=").nth(1).unwrap().trim().parse().unwrap();
    assert!(calls >= 6);
    for fail in 1..=calls {
        let out = p.run(fail);
        assert_eq!(out.status.code(), Some(1), "fail={fail} {out:?}");
        assert_clean(&out);
    }
}
#[test]
fn generated_text_and_recursive_scalar_failures_cleanup() {
    for source in ["متغير أ: نص = \"a\"؛ اطبع(أ[4])؛", "دالة ف(س: ص32): ص32 { إذا (س == 0) { أرجع 1 / س؛ } أرجع ف(س - 1)؛ } متغير أ: نص = \"a\" + \"ب\"؛ اطبع(ف(3))؛", "متغير أ: نص = \"a\"؛ متغير س: ص8 = 127؛ اطبع(س + 1)؛"]{let p=Instrumented::new(source);let out=p.run(0);assert_eq!(out.status.code(),Some(1),"{out:?}");assert_clean(&out);}
}
#[test]
fn index_widths_and_exact_float_values() {
    for ty in [
        "ص8", "ص16", "ص32", "ص64", "ط8", "ط16", "ط32", "ط64", "ع32", "ع64",
    ] {
        let out = run(&format!(
            "متغير س: نص = \"أ🙂\"؛ متغير ف: {ty} = 1؛ اطبع(س[ف])؛"
        ));
        assert!(out.status.success(), "{ty}: {out:?}");
        assert_eq!(
            out.stdout,
            if cfg!(windows) {
                "🙂\r\n".as_bytes()
            } else {
                "🙂\n".as_bytes()
            }
        );
    }
}
#[test]
fn unsupported_text_types_and_nested_contexts() {
    for source in [
        "خارجي دالة ف(س: نص): فراغ = \"f\"؛",
        "متغير س: مؤشر<نص> = مؤشر_فارغ<نص>()؛",
        "متغير س: نص = \"أ\"؛ اطبع(1 + س)؛",
        "متغير س: نص = \"أ\"؛ اطبع(س[صحيح])؛",
    ] {
        let tokens = lisan::lexer::tokenize_located(source, lisan::source::SourceId(0)).unwrap();
        let p = lisan::parser::parse_located(tokens).unwrap();
        assert!(lisan::checker::check(&p).is_err(), "{source}");
    }
}
#[test]
fn direct_ir_cannot_bypass_managed_structural_validation() {
    use lisan::{
        source::Span,
        typed::*,
        types::{Abi, FunctionSig, Type},
    };
    let span = Span {
        start: 17,
        end: 23,
        ..Default::default()
    };
    let text = TypedExpr {
        kind: TypedExprKind::Text("أ".into()),
        ty: Type::Text,
        span,
    };
    let length = TypedExpr {
        kind: TypedExprKind::TextLength(Box::new(text.clone())),
        ty: Type::Int {
            signed: false,
            bits: 64,
        },
        span,
    };
    let stmt = |kind| TypedStmt { kind, span };
    let mut programs = vec![];
    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::If(text.clone(), vec![], None))],
        ..Default::default()
    });
    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::While(text.clone(), vec![]))],
        ..Default::default()
    });
    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::Expr(TypedExpr {
            kind: TypedExprKind::Binary(
                Box::new(length.clone()),
                lisan::ast::Op::And,
                Box::new(length.clone()),
            ),
            ty: Type::Bool,
            span,
        }))],
        ..Default::default()
    });

    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::Print(TypedExpr {
            kind: TypedExprKind::Bool(true),
            ty: Type::Text,
            span,
        }))],
        ..Default::default()
    });
    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::Print(TypedExpr {
            kind: TypedExprKind::TextLength(Box::new(text.clone())),
            ty: Type::Bool,
            span,
        }))],
        ..Default::default()
    });
    programs.push(TypedProgram {
        main: vec![stmt(TypedStmtKind::Print(TypedExpr {
            kind: TypedExprKind::Variable(LocalId(0)),
            ty: Type::Text,
            span,
        }))],
        ..Default::default()
    });
    programs.push(TypedProgram {
        functions: vec![TypedFunction {
            id: FunctionId(0),
            name: "f".into(),
            symbol: "f".into(),
            signature: FunctionSig {
                params: vec![],
                result: Type::Int {
                    signed: false,
                    bits: 64,
                },
                abi: Abi::Lisan,
            },
            params: vec![],
            body: vec![stmt(TypedStmtKind::Return(Some(text.clone())))],
            span,
            annotated: true,
        }],
        ..Default::default()
    });
    programs.push(TypedProgram {
        externs: vec![ExternFunction {
            id: FunctionId(0),
            name: "f".into(),
            symbol: "f".into(),
            signature: FunctionSig {
                params: vec![Type::Pointer(Box::new(Type::Text))],
                result: Type::Void,
                abi: Abi::C,
            },
            span,
        }],
        ..Default::default()
    });
    for p in programs {
        let errors = lisan::native::typed_codegen::generate(&p).unwrap_err();
        assert_eq!(errors[0].primary, span);
    }
}
#[test]
fn managed_object_stays_single_input_and_library_gate_rejects_main() {
    let p = checked("متغير س: نص = \"أ\"؛ اطبع(س)؛");
    assert!(lisan::native::typed_codegen::generate_library(&p).is_err());
    let ir = lisan::native::typed_codegen::generate(&p).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "lisan-text-object-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.lsn");
    fs::write(&source, "sentinel").unwrap();
    let object = dir.join("text.o");
    lisan::native::compile_ir_with_options(
        &ir,
        &source,
        &object,
        &lisan::native::NativeBuildOptions {
            emit: lisan::native::NativeEmit::Object,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(object.is_file());
    assert_eq!(fs::read_to_string(&source).unwrap(), "sentinel");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn cr_text_content_is_preserved_with_nul() {
    let o = run("متغير س: نص = \"a\r\0b\"؛ اطبع(س)؛");
    assert!(o.status.success());
    let expected = if cfg!(windows) {
        b"a\r\0b\r\n".as_slice()
    } else {
        b"a\r\0b\n".as_slice()
    };
    assert_eq!(o.stdout, expected);
}
#[test]
fn scalar_callee_diagnostic_has_unicode_source_column() {
    let o = run("دالة ف(س: ص32): ص32 { أرجع 1 / س؛ }\nمتغير أ: نص = \"a\"؛ اطبع(ف(0))؛");
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("input.lsn:1:28:"),
        "{o:?}"
    );
}
#[test]
fn text_operands_snapshot_before_later_assignments() {
    let o=run("متغير س: نص = \"a\"؛ اطبع(س + (س = \"b\"))؛ اطبع((س = \"a\") + (س = \"b\"))؛ س = \"a\"؛ اطبع(س == (س = \"b\"))؛ س = \"ab\"؛ اطبع(س[طول(س = \"b\") - 1])؛");
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "ab\nab\nخطأ\na\n"
    );
}
#[test]
fn text_length_contextualizes_literals_on_either_side() {
    let o = run("متغير س: نص = \"أ\"؛ اطبع(1 + طول(س))؛ اطبع(طول(س) + 1)؛ اطبع(1 == طول(س))؛");
    assert!(o.status.success());
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "2\n2\nصحيح\n"
    );
}
#[test]
fn imported_scalar_failure_reports_import_source() {
    let dir = std::env::temp_dir().join(format!(
        "lisan-text-source-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("part.lsn"), "دالة ف(س: ص32): ص32 { أرجع 1 / س؛ }").unwrap();
    let source = dir.join("main.lsn");
    fs::write(
        &source,
        "استورد \"part.lsn\"؛ متغير أ: نص = \"أ\"؛ اطبع(ف(0))؛",
    )
    .unwrap();
    let exe = dir.join(format!("run{}", std::env::consts::EXE_SUFFIX));
    lisan::native::build_native(&source, &exe).unwrap();
    let out = Command::new(exe).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("part.lsn:1:28:"),
        "{out:?}"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn standalone_compiler_builds_embedded_runtime_outside_checkout() {
    let dir = std::env::temp_dir().join(format!(
        "lisan-text-package-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let compiler = dir.join(format!("compiler{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_lisan"), &compiler).unwrap();
    let source = dir.join("input.lsn");
    fs::write(&source, "متغير س: نص = \"أ\" + \"🙂\"؛ اطبع(س)؛").unwrap();
    let exe = dir.join(format!("run{}", std::env::consts::EXE_SUFFIX));
    let build = Command::new(compiler)
        .current_dir(&dir)
        .arg("--native")
        .arg(&source)
        .arg("--output")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let out = Command::new(exe).output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"),
        "أ🙂\n"
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn declared_scalar_length_name_keeps_resolution() {
    let o = run("دالة طول(س: ص32): ص32 { أرجع س + 1؛ } اطبع(طول(41))؛ اطبع(1 + طول(40))؛");
    assert!(o.status.success());
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "42\n42\n"
    );
}
#[test]
fn imported_scalar_length_name_keeps_resolution() {
    let p = Instrumented::new(
        "خارجي دالة طول(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"a\"؛ غير_آمن { طول(1)؛ }",
    );
    let o = p.run(0);
    assert!(o.status.success(), "{o:?}");
    assert_clean(&o);
}
#[test]
fn managed_runtime_does_not_change_cpp_link_input_language() {
    let dir = std::env::temp_dir().join(format!(
        "lisan-text-cpp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("input.lsn");
    fs::write(&source,"خارجي دالة ج(): ص32 = \"cpp_answer\"؛ متغير س: نص = \"أ\"؛ اطبع(س)؛ غير_آمن { اطبع(ج())؛ }").unwrap();
    let cpp = dir.join("answer.cpp");
    fs::write(&cpp, "extern \"C\" int cpp_answer(void) { return 42; }").unwrap();
    let exe = dir.join(format!("run{}", std::env::consts::EXE_SUFFIX));
    lisan::native::build_native_with_options(
        &source,
        &exe,
        &lisan::native::NativeBuildOptions {
            link_inputs: vec![cpp],
            ..Default::default()
        },
    )
    .unwrap();
    let out = Command::new(exe).output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"),
        "أ\n42\n"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn direct_ir_rejects_nontext_binary_operands_with_text_result() {
    use lisan::{ast::Op, source::Span, typed::*, types::Type};
    let span = Span {
        start: 41,
        end: 53,
        ..Default::default()
    };
    for (kind, ty) in [
        (
            TypedExprKind::Int(1),
            Type::Int {
                signed: true,
                bits: 64,
            },
        ),
        (TypedExprKind::Float(1.0), Type::Float { bits: 64 }),
        (TypedExprKind::Bool(true), Type::Bool),
        (
            TypedExprKind::NullPointer,
            Type::Pointer(Box::new(Type::Int {
                signed: true,
                bits: 32,
            })),
        ),
    ] {
        let operand = TypedExpr { kind, ty, span };
        let program = TypedProgram {
            main: vec![TypedStmt {
                kind: TypedStmtKind::Print(TypedExpr {
                    kind: TypedExprKind::Binary(
                        Box::new(operand.clone()),
                        Op::Add,
                        Box::new(operand),
                    ),
                    ty: Type::Text,
                    span,
                }),
                span,
            }],
            ..Default::default()
        };
        let diagnostic = lisan::native::managed::validate(&program).unwrap_err();
        assert_eq!(diagnostic.primary, span);
        let diagnostics = lisan::native::typed_codegen::generate(&program).unwrap_err();
        assert_eq!(diagnostics[0].primary, span);
    }
}

#[test]
fn control_flow_releases_conditions_and_loop_scopes() {
    let source = "خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"أ🙂\"؛ متغير ع: ص32 = 0؛ بينما (طول(أ + \"\") == 2 و ع < 100000) { متغير ب: نص = أ + \"ب\"؛ غير_آمن { تحقق(2)؛ } ع = ع + 1؛ إذا (ع < 99999) { متغير ج: نص = \"x\"؛ تابع؛ } إذا (ع == 100000) { متغير ج: نص = \"y\"؛ توقف؛ } } غير_آمن { تحقق(1)؛ } اطبع(أ)؛";
    let p = Instrumented::new(source);
    let o = p.run(0);
    assert!(o.status.success(), "{o:?}");
    assert_clean(&o);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "أ🙂\n"
    );
}

#[test]
fn control_flow_short_circuit_and_nested_runtime_discovery() {
    let o = run("متغير س: نص = \"a\"؛ إذا (طول(\"🙂\") == 1) { اطبع(\"أ\" + \"ب\")؛ } وإلا { اطبع(\"bad\")؛ } اطبع(خطأ و (طول(س[9]) == 1))؛ اطبع(صحيح أو (طول(س[9]) == 1))؛ بينما (خطأ) { اطبع(\"x\" + \"y\")؛ }");
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "أب\nخطأ\nصحيح\n"
    );
}

#[test]
fn finite_nested_loops_preserve_outer_slots_and_cleanup_every_oom() {
    let p = Instrumented::new("خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"أ\"؛ متغير ع: ص32 = 0؛ بينما (ع < 3 و طول(أ + \"\") == 1) { متغير ب: نص = \"🙂\"؛ متغير د: ص32 = 0؛ بينما (د < 3) { متغير ج: نص = ب + أ؛ د = د + 1؛ { متغير ك: نص = \"z\"؛ إذا (د == 1) { تابع؛ } إذا (د == 2) { توقف؛ } } } غير_آمن { تحقق(2)؛ } أ = أ[0]؛ ع = ع + 1؛ } غير_آمن { تحقق(1)؛ } اطبع(أ)؛");
    let good = p.run(0);
    assert!(good.status.success(), "{good:?}");
    assert_clean(&good);
    let calls: usize = String::from_utf8_lossy(&good.stderr)
        .split("CALLS=")
        .nth(1)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(calls > 15);
    for fail in 1..=calls {
        let o = p.run(fail);
        assert_eq!(o.status.code(), Some(1), "fail={fail} {o:?}");
        assert_clean(&o);
    }
}

#[test]
fn skipped_rhs_does_not_allocate_and_selected_rhs_failure_cleans() {
    let p = Instrumented::new("خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"a\"؛ اطبع(خطأ و (طول(أ + \"b\") == 2))؛ اطبع(صحيح أو (طول(أ[9]) == 1))؛ غير_آمن { تحقق(1)؛ }");
    let o = p.run(2);
    assert!(o.status.success(), "{o:?}");
    assert_clean(&o);
    assert!(String::from_utf8_lossy(&o.stderr).contains("CALLS=1"));
    for source in [
        "متغير أ: نص = \"a\"؛\nبينما (طول(أ[3]) == 1) { توقف؛ }",
        "متغير أ: نص = \"a\"؛\nإذا (صحيح و (طول(أ[3]) == 1)) { اطبع(1)؛ }",
        "متغير أ: نص = \"a\"؛ متغير ع: ص8 = 127؛\nبينما (صحيح) { متغير ب: نص = \"b\"؛ اطبع(ع + 1)؛ }",
    ] {
        let p = Instrumented::new(source);
        let o = p.run(0);
        assert_eq!(o.status.code(), Some(1), "{o:?}");
        assert_clean(&o);
        let located = run(source);
        assert!(String::from_utf8_lossy(&located.stdout).contains("input.lsn:2:"), "{located:?}");
    }
}

#[test]
fn both_if_arms_and_false_loop_condition_release_before_next_statement() {
    let p = Instrumented::new("خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ متغير أ: نص = \"أ\"؛ متغير ع: ص32 = 0؛ بينما (ع < 2 و طول(أ + \"\") == 1) { إذا (ع == 0) { متغير ب: نص = \"🙂\"؛ اطبع(ب)؛ } وإلا { متغير ب: نص = \"a\0ب\"؛ اطبع(ب)؛ } غير_آمن { تحقق(1)؛ } ع = ع + 1؛ } بينما (طول(أ + \"\") == 0) { اطبع(\"bad\")؛ } غير_آمن { تحقق(1)؛ }");
    let o = p.run(0);
    assert!(o.status.success(), "{o:?}");
    assert_clean(&o);
    assert_eq!(
        o.stdout,
        if cfg!(windows) {
            "🙂\r\na\0ب\r\n".as_bytes()
        } else {
            "🙂\na\0ب\n".as_bytes()
        }
    );
}

#[test]
fn direct_ir_accepts_main_text_control_flow() {
    let p = checked("متغير أ: نص = \"a\"؛ بينما (طول(أ) == 1) { إذا (خطأ أو (أ == \"a\")) { توقف؛ } } اطبع(صحيح و (أ == \"a\"))؛");
    let ir = lisan::native::typed_codegen::generate(&p).unwrap();
    assert!(ir.contains("LISAN_MANAGED_TEXT_RUNTIME_V1"));
}

#[test]
fn logical_nested_expression_preserves_enclosing_text_snapshot() {
    let o = run("دالة اختر(س: منطقي): ص32 { إذا (س) { أرجع 0؛ } أرجع 1؛ } متغير أ: نص = \"ab\"؛ اطبع(أ[اختر((\"x\" + \"y\") == \"xy\" و صحيح)])؛ اطبع(أ + (أ = \"c\"))؛");
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().replace("\r\n", "\n"),
        "a\nabc\n"
    );
}

#[test]
fn recursive_text_returns_and_argument_snapshots() {
    let p = Instrumented::new("دالة كرر(س: نص، ن: ص32): نص { متغير ع: نص = س + \"x\"؛ إذا (ن == 0) { أرجع ع؛ } أرجع كرر(ع، ن - 1)؛ } دالة اختر(أ: نص، ب: نص): نص { أ = أ + ب؛ أرجع أ؛ } متغير س: نص = \"a\"؛ اطبع(اختر(س، س = \"b\"))؛ اطبع(كرر(س، 8))؛");
    let out = p.run(0);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ab\nbxxxxxxxxx\n");
    assert_clean(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let calls: usize = stderr
        .split("CALLS=")
        .nth(1)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    for fail in 1..=calls {
        let out = p.run(fail);
        assert_eq!(out.status.code(), Some(1), "fail={fail} {out:?}");
        assert_clean(&out);
    }
}
#[test]
fn recursive_scalar_failure_cleans_all_text_frames() {
    let p = Instrumented::new("دالة ف(س: نص، ن: ص32): نص { متغير ع: نص = س + \"x\"؛ إذا (ن == 0) { متغير ق: ص32 = 1 / ن؛ } أرجع ف(ع، ن - 1)؛ } متغير س: نص = \"a\"؛ اطبع(ف(س، 12))؛");
    let out = p.run(0);
    assert_eq!(out.status.code(), Some(1));
    assert_clean(&out);
}

#[test]
fn managed_internal_functions_reject_library_and_foreign_entrypoints() {
    let p = checked("دالة ف(): نص { أرجع \"أ\"؛ }");
    assert!(lisan::native::typed_codegen::generate_library(&p).is_err());
    let p =
        checked("دالة ف(): ط64 { أرجع طول(\"أ\")؛ } صدّر دالة ج(): ط64 = \"entry\" { أرجع ف()؛ }");
    assert!(lisan::native::typed_codegen::generate(&p).is_err());
}
#[test]
fn repeated_recursive_calls_release_before_next_statement() {
    let p = Instrumented::new("خارجي دالة تحقق(س: ط64): فراغ = \"check_live\"؛ دالة ف(س: نص، ن: ص32): نص { إذا (ن == 0) { أرجع س؛ } أرجع ف(س + \"x\"، ن - 1)؛ } متغير س: نص = \"a\"؛ متغير ن: ص32 = 0؛ بينما (ن < 1000) { { متغير ع: نص = ف(س، 5)؛ } غير_آمن { تحقق(1)؛ } ن = ن + 1؛ }");
    let out = p.run(0);
    assert!(out.status.success(), "{out:?}");
    assert_clean(&out);
}

#[test]
fn direct_ir_rejects_missing_results_on_all_paths() {
    use lisan::{typed::*, types::Type};
    let p = checked("دالة ف(): نص { أرجع \"أ\"؛ }");
    let span = p.functions[0].span;
    let statement = |kind| TypedStmt { kind, span };
    let ret = p.functions[0].body[0].clone();
    let condition = TypedExpr {
        kind: TypedExprKind::Bool(true),
        ty: Type::Bool,
        span,
    };
    let bodies = vec![
        vec![],
        vec![statement(TypedStmtKind::Return(None))],
        vec![statement(TypedStmtKind::If(
            condition.clone(),
            vec![ret.clone()],
            None,
        ))],
        vec![statement(TypedStmtKind::If(
            condition.clone(),
            vec![ret.clone()],
            Some(vec![]),
        ))],
        vec![
            ret.clone(),
            statement(TypedStmtKind::Block(vec![statement(
                TypedStmtKind::Return(None),
            )])),
        ],
        vec![statement(TypedStmtKind::While(
            condition.clone(),
            vec![statement(TypedStmtKind::Break)],
        ))],
    ];
    for body in bodies {
        let mut invalid = p.clone();
        invalid.functions[0].body = body;
        assert!(lisan::native::typed_codegen::generate(&invalid).is_err());
    }
    let valid_bodies = vec![
        vec![statement(TypedStmtKind::If(
            condition.clone(),
            vec![ret.clone()],
            Some(vec![ret.clone()]),
        ))],
        vec![statement(TypedStmtKind::While(
            condition.clone(),
            vec![ret.clone()],
        ))],
        vec![statement(TypedStmtKind::While(
            condition,
            vec![statement(TypedStmtKind::Continue)],
        ))],
    ];
    for body in valid_bodies {
        let mut valid = p.clone();
        valid.functions[0].body = body;
        assert!(lisan::native::typed_codegen::generate(&valid).is_ok());
    }
    let mut scalar = checked("دالة ف(): ص32 { أرجع 1؛ }");
    scalar.functions[0].body = vec![statement(TypedStmtKind::Return(None))];
    assert!(lisan::native::typed_codegen::generate(&scalar).is_err());
    scalar.functions[0].body.clear();
    assert!(lisan::native::typed_codegen::generate(&scalar).is_err());
    scalar.functions[0].annotated = false;
    assert!(lisan::native::typed_codegen::generate(&scalar).is_ok());
}
