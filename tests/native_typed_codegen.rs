use lisan::{ast::Op, source::Span, typed::*, types::*};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
fn e(kind: TypedExprKind, ty: Type) -> TypedExpr {
    TypedExpr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(v: i128, bits: u8, signed: bool) -> TypedExpr {
    e(TypedExprKind::Int(v), Type::Int { bits, signed })
}
fn fl(v: f64, bits: u8) -> TypedExpr {
    e(TypedExprKind::Float(v), Type::Float { bits })
}
fn s(kind: TypedStmtKind) -> TypedStmt {
    TypedStmt {
        kind,
        span: Span::default(),
    }
}
fn bin(a: TypedExpr, op: Op, b: TypedExpr) -> TypedExpr {
    let ty = a.ty.clone();
    e(TypedExprKind::Binary(Box::new(a), op, Box::new(b)), ty)
}
fn print(v: TypedExpr) -> TypedStmt {
    s(TypedStmtKind::Print(v))
}
fn execute(p: TypedProgram, c: Option<&str>) -> std::process::Output {
    execute_mode(p, c, false)
}
fn execute_mode(p: TypedProgram, c: Option<&str>, library: bool) -> std::process::Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lisan-typed-codegen-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let ir = if library {
        lisan::native::typed_codegen::generate_library(&p)
    } else {
        lisan::native::typed_codegen::generate(&p)
    }
    .unwrap();
    fs::write(dir.join("test.ll"), &ir).unwrap();
    let source = dir.join("test.lsn");
    fs::write(&source, "").unwrap();
    let executable = dir.join(format!("test{}", std::env::consts::EXE_SUFFIX));
    let mut options = lisan::native::NativeBuildOptions::default();
    if let Some(c) = c {
        let fixture = dir.join("fixture.c");
        fs::write(&fixture, c).unwrap();
        options.link_inputs.push(fixture);
    }
    lisan::native::compile_ir_with_options(&ir, &source, &executable, &options).unwrap();
    let mut out = Command::new(&executable).output().unwrap();
    out.stdout = String::from_utf8(out.stdout)
        .unwrap()
        .replace("\r\n", "\n")
        .into_bytes();
    fs::remove_dir_all(dir).unwrap();
    out
}
fn program(main: Vec<TypedStmt>) -> TypedProgram {
    TypedProgram {
        main,
        ..Default::default()
    }
}
#[test]
fn exact_scalar_widths_execute() {
    let out = execute(
        program(vec![
            print(int(9007199254740993, 64, true)),
            print(int(u64::MAX as i128, 64, false)),
            print(int(-128, 8, true)),
            print(fl(1.25, 32)),
            print(e(TypedExprKind::Bool(true), Type::Bool)),
        ]),
        None,
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "9007199254740993\n18446744073709551615\n-128\n1.25\nصحيح\n"
    );
}
#[test]
fn checked_integer_failures_execute() {
    for expr in [
        bin(int(127, 8, true), Op::Add, int(1, 8, true)),
        bin(int(0, 8, false), Op::Subtract, int(1, 8, false)),
        bin(int(65535, 16, false), Op::Multiply, int(2, 16, false)),
        bin(int(-128, 8, true), Op::Divide, int(-1, 8, true)),
        bin(int(-128, 8, true), Op::Modulo, int(-1, 8, true)),
        bin(int(1, 64, false), Op::Divide, int(0, 64, false)),
    ] {
        let out = execute(program(vec![print(expr)]), None);
        assert!(!out.status.success());
    }
}
#[test]
fn checked_float_and_cast_failures_execute() {
    for expr in [
        bin(fl(f64::MAX, 64), Op::Multiply, fl(2., 64)),
        bin(fl(1., 32), Op::Divide, fl(0., 32)),
        e(
            TypedExprKind::Cast(Box::new(int(256, 16, false))),
            Type::Int {
                bits: 8,
                signed: false,
            },
        ),
        e(
            TypedExprKind::Cast(Box::new(fl(18446744073709551616., 64))),
            Type::Int {
                bits: 64,
                signed: false,
            },
        ),
        e(
            TypedExprKind::Cast(Box::new(fl(-1., 64))),
            Type::Int {
                bits: 64,
                signed: false,
            },
        ),
        e(
            TypedExprKind::Cast(Box::new(fl(f64::MAX, 64))),
            Type::Float { bits: 32 },
        ),
    ] {
        assert!(!execute(program(vec![print(expr)]), None).status.success());
    }
}
fn var(id: u32, ty: Type) -> TypedExpr {
    e(TypedExprKind::Variable(LocalId(id)), ty)
}
fn call(id: u32, args: Vec<TypedExpr>, ty: Type) -> TypedExpr {
    e(
        TypedExprKind::Call {
            callee: FunctionId(id),
            args,
        },
        ty,
    )
}
fn compare(a: TypedExpr, op: Op, b: TypedExpr) -> TypedExpr {
    e(
        TypedExprKind::Binary(Box::new(a), op, Box::new(b)),
        Type::Bool,
    )
}
fn assign(id: u32, value: TypedExpr) -> TypedStmt {
    let ty = value.ty.clone();
    s(TypedStmtKind::Expr(e(
        TypedExprKind::Assign(LocalId(id), Box::new(value)),
        ty,
    )))
}
fn function(
    id: u32,
    symbol: &str,
    sig: FunctionSig,
    params: Vec<LocalId>,
    body: Vec<TypedStmt>,
) -> TypedFunction {
    TypedFunction {
        id: FunctionId(id),
        name: symbol.into(),
        symbol: symbol.into(),
        signature: sig,
        params,
        body,
        span: Span::default(),
        annotated: true,
    }
}
#[test]
fn recursion_control_flow_and_short_circuit_execute() {
    let ty = Type::Int {
        bits: 64,
        signed: true,
    };
    let v = || var(0, ty.clone());
    let fac = function(
        0,
        "fac",
        FunctionSig {
            params: vec![ty.clone()],
            result: ty.clone(),
            abi: Abi::Lisan,
        },
        vec![LocalId(0)],
        vec![
            s(TypedStmtKind::If(
                compare(v(), Op::LessEqual, int(1, 64, true)),
                vec![s(TypedStmtKind::Return(Some(int(1, 64, true))))],
                None,
            )),
            s(TypedStmtKind::Return(Some(bin(
                v(),
                Op::Multiply,
                call(
                    0,
                    vec![bin(v(), Op::Subtract, int(1, 64, true))],
                    ty.clone(),
                ),
            )))),
        ],
    );
    let mut p = program(vec![
        print(call(0, vec![int(10, 64, true)], ty.clone())),
        s(TypedStmtKind::Let {
            id: LocalId(0),
            name: "counter".into(),
            ty: ty.clone(),
            value: int(0, 64, true),
        }),
        s(TypedStmtKind::While(
            compare(v(), Op::Less, int(10, 64, true)),
            vec![
                assign(0, bin(v(), Op::Add, int(1, 64, true))),
                s(TypedStmtKind::If(
                    compare(v(), Op::Equal, int(2, 64, true)),
                    vec![s(TypedStmtKind::Continue)],
                    None,
                )),
                s(TypedStmtKind::If(
                    compare(v(), Op::Equal, int(4, 64, true)),
                    vec![s(TypedStmtKind::Break)],
                    None,
                )),
                print(v()),
            ],
        )),
    ]);
    p.functions.push(fac);
    for (lhs, op) in [(false, Op::And), (true, Op::Or)] {
        let bad = compare(
            bin(int(1, 8, true), Op::Divide, int(0, 8, true)),
            Op::Equal,
            int(0, 8, true),
        );
        p.main.push(print(e(
            TypedExprKind::Binary(
                Box::new(e(TypedExprKind::Bool(lhs), Type::Bool)),
                op,
                Box::new(bad),
            ),
            Type::Bool,
        )));
    }
    let out = execute(p, None);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3628800\n1\n3\nخطأ\nصحيح\n"
    );
}
#[test]
fn valid_casts_and_exact_unsigned_arithmetic_execute() {
    let cast = |v: TypedExpr, ty: Type| e(TypedExprKind::Cast(Box::new(v)), ty);
    let out = execute(
        program(vec![
            print(cast(
                int(-127, 8, true),
                Type::Int {
                    signed: true,
                    bits: 64,
                },
            )),
            print(cast(
                int(255, 8, false),
                Type::Int {
                    signed: true,
                    bits: 16,
                },
            )),
            print(cast(
                fl(9223372036854774784., 64),
                Type::Int {
                    signed: true,
                    bits: 64,
                },
            )),
            print(cast(
                fl(-9223372036854775808., 64),
                Type::Int {
                    signed: true,
                    bits: 64,
                },
            )),
            print(cast(
                fl(18446744073709549568., 64),
                Type::Int {
                    signed: false,
                    bits: 64,
                },
            )),
            print(cast(int(10, 64, false), Type::Float { bits: 32 })),
            print(cast(fl(1.25, 32), Type::Float { bits: 64 })),
            print(bin(
                int(u64::MAX as i128, 64, false),
                Op::Divide,
                int(2, 64, false),
            )),
            print(bin(int(-7, 32, true), Op::Modulo, int(3, 32, true))),
        ]),
        None,
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout),"-127\n255\n9223372036854774784\n-9223372036854775808\n18446744073709549568\n10\n1.25\n9223372036854775807\n-1\n");
}
#[test]
fn c_import_export_exact_abi_execute() {
    let types = [
        (
            "s8",
            Type::Int {
                signed: true,
                bits: 8,
            },
        ),
        (
            "u8",
            Type::Int {
                signed: false,
                bits: 8,
            },
        ),
        (
            "s16",
            Type::Int {
                signed: true,
                bits: 16,
            },
        ),
        (
            "u16",
            Type::Int {
                signed: false,
                bits: 16,
            },
        ),
        (
            "s32",
            Type::Int {
                signed: true,
                bits: 32,
            },
        ),
        (
            "u32",
            Type::Int {
                signed: false,
                bits: 32,
            },
        ),
        (
            "s64",
            Type::Int {
                signed: true,
                bits: 64,
            },
        ),
        (
            "u64",
            Type::Int {
                signed: false,
                bits: 64,
            },
        ),
        ("f32", Type::Float { bits: 32 }),
        ("f64", Type::Float { bits: 64 }),
        ("boolean", Type::Bool),
        (
            "pointer",
            Type::Pointer(Box::new(Type::Int {
                signed: false,
                bits: 8,
            })),
        ),
    ];
    let ctype = [
        "signed char",
        "unsigned char",
        "short",
        "unsigned short",
        "int",
        "unsigned int",
        "long long",
        "unsigned long long",
        "float",
        "double",
        "_Bool",
        "void *",
    ];
    let inputs = [
        "-127",
        "250",
        "-32000",
        "65000",
        "-2000000000",
        "4000000000U",
        "-9007199254740993LL",
        "18446744073709551615ULL",
        "1.25f",
        "2.5",
        "1",
        "(void*)0",
    ];
    let mut p = program(vec![]);
    let mut c = String::new();
    for (i, ((name, ty), ct)) in types.iter().zip(ctype).enumerate() {
        let sig = FunctionSig {
            params: vec![ty.clone()],
            result: ty.clone(),
            abi: Abi::C,
        };
        let export = format!("lisan_{name}");
        let import = format!("c_{name}");
        p.functions.push(function(
            i as u32,
            &export,
            sig.clone(),
            vec![LocalId(0)],
            vec![s(TypedStmtKind::Return(Some(call(
                i as u32 + 100,
                vec![var(0, ty.clone())],
                ty.clone(),
            ))))],
        ));
        p.exports.push(ExportFunction {
            function_id: FunctionId(i as u32),
            symbol: export.clone(),
            span: Span::default(),
        });
        p.externs.push(ExternFunction {
            id: FunctionId(i as u32 + 100),
            name: import.clone(),
            symbol: import.clone(),
            signature: sig,
            span: Span::default(),
        });
        c.push_str(&format!(
            "extern {ct} {export}({ct});\n{ct} {import}({ct} a) {{return a;}}\n"
        ));
    }
    p.functions.push(function(
        200,
        "lisan_void",
        FunctionSig {
            params: vec![],
            result: Type::Void,
            abi: Abi::C,
        },
        vec![],
        vec![s(TypedStmtKind::Return(None))],
    ));
    c.push_str("extern void lisan_void(void);\nlong long c_verify(void) {lisan_void();\n");
    for ((name, _), input) in types.iter().zip(inputs) {
        c.push_str(&format!(
            "if(lisan_{name}({input}) != ({input})) return 0;\n"
        ));
    }
    c.push_str("return 1;}\n");
    let result = Type::Int {
        signed: true,
        bits: 64,
    };
    p.externs.push(ExternFunction {
        id: FunctionId(300),
        name: "c_verify".into(),
        symbol: "c_verify".into(),
        signature: FunctionSig {
            params: vec![],
            result: result.clone(),
            abi: Abi::C,
        },
        span: Span::default(),
    });
    p.main.push(print(call(300, vec![], result)));
    let out = execute(p, Some(&c));
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"1\n");
}
#[test]
fn managed_text_expression_executes_and_is_released() {
    let out = execute(
        program(vec![s(TypedStmtKind::Expr(e(
            TypedExprKind::Text("dynamic".into()),
            Type::Text,
        )))]),
        None,
    );
    assert!(out.status.success());
}
#[test]
fn c_nonfinite_result_is_rejected() {
    let mut p = program(vec![print(call(0, vec![], Type::Float { bits: 64 }))]);
    p.externs.push(ExternFunction {
        id: FunctionId(0),
        name: "bad".into(),
        symbol: "bad".into(),
        signature: FunctionSig {
            params: vec![],
            result: Type::Float { bits: 64 },
            abi: Abi::C,
        },
        span: Span::default(),
    });
    assert!(
        !execute(p, Some("double bad(void){return __builtin_inf();}"))
            .status
            .success()
    );
}
#[test]
fn float_integer_cast_truncates_before_range_check() {
    let mut main = vec![];
    for (v, signed) in [(-128.9, true), (-0.5, false), (127.9, true), (255.9, false)] {
        main.push(print(e(
            TypedExprKind::Cast(Box::new(fl(v, 64))),
            Type::Int { signed, bits: 8 },
        )));
    }
    let out = execute(program(main), None);
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"-128\n0\n127\n255\n");
}
#[test]
fn float_zero_divisors_trap_for_both_signs() {
    for bits in [32, 64] {
        for zero in [0., -0.] {
            for op in [Op::Divide, Op::Modulo] {
                for lhs in [0., 1.] {
                    assert!(!execute(
                        program(vec![print(bin(fl(lhs, bits), op, fl(zero, bits)))]),
                        None
                    )
                    .status
                    .success());
                }
            }
        }
    }
}
#[test]
fn embedded_nul_text_print_preserves_bytes() {
    let out = execute(
        program(vec![print(e(
            TypedExprKind::Text("a\0b".into()),
            Type::Text,
        ))]),
        None,
    );
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"a\0b\n");
}
#[test]
fn library_object_links_with_c_host_main() {
    let ty = Type::Int {
        signed: true,
        bits: 8,
    };
    let mut p = program(vec![]);
    p.functions.push(function(
        0,
        "lisan_library",
        FunctionSig {
            params: vec![ty.clone()],
            result: ty.clone(),
            abi: Abi::C,
        },
        vec![LocalId(0)],
        vec![s(TypedStmtKind::Return(Some(var(0, ty))))],
    ));
    let ir = lisan::native::typed_codegen::generate_library(&p).unwrap();
    assert!(!ir.contains("define i32 @main"));
    let dir = std::env::temp_dir().join(format!("lisan-library-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lib.ll"), ir).unwrap();
    fs::write(dir.join("host.c"),"extern signed char lisan_library(signed char); int main(void){return lisan_library(-127)==-127?0:1;}").unwrap();
    let clang = std::env::var("LISAN_CLANG").unwrap_or_else(|_| "clang".into());
    let out = Command::new(&clang)
        .arg("-O2")
        .arg("-c")
        .arg(dir.join("lib.ll"))
        .arg("-o")
        .arg(dir.join("lib.o"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let host = dir.join(format!("host{}", std::env::consts::EXE_SUFFIX));
    let mut link = Command::new(&clang);
    link.arg(dir.join("host.c"))
        .arg(dir.join("lib.o"))
        .arg("-o")
        .arg(&host);
    #[cfg(all(windows, target_env = "msvc"))]
    link.arg("-Xlinker").arg("legacy_stdio_definitions.lib");
    let out = link.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(Command::new(&host).status().unwrap().success());
    fs::remove_dir_all(dir).unwrap();
    p.main.push(print(int(1, 8, true)));
    assert!(lisan::native::typed_codegen::generate_library(&p).is_err());
}
#[test]
fn reserved_runtime_symbols_are_diagnosed() {
    for name in [
        "main",
        "puts",
        "printf",
        "exit",
        "llvm.trunc.f64",
        ".lisan.text.0",
    ] {
        let mut p = program(vec![]);
        p.functions.push(function(
            0,
            name,
            FunctionSig {
                params: vec![],
                result: Type::Void,
                abi: Abi::C,
            },
            vec![],
            vec![],
        ));
        assert!(lisan::native::typed_codegen::generate(&p).is_err());
    }
}
#[test]
fn pure_void_internal_return_executes() {
    let mut p = program(vec![
        s(TypedStmtKind::Expr(call(0, vec![], Type::Void))),
        print(int(42, 8, true)),
    ]);
    p.functions.push(function(
        0,
        "pure_void",
        FunctionSig {
            params: vec![],
            result: Type::Void,
            abi: Abi::Lisan,
        },
        vec![],
        vec![s(TypedStmtKind::Return(None))],
    ));
    let out = execute(p, None);
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"42\n");
}
#[test]
fn every_integer_width_checks_arithmetic_and_cast_boundaries() {
    for bits in [8, 16, 32, 64] {
        for signed in [false, true] {
            let ty = Type::Int { signed, bits };
            let (lo, hi) = ty.int_bounds().unwrap();
            for expr in [
                bin(int(hi, bits, signed), Op::Add, int(1, bits, signed)),
                bin(int(lo, bits, signed), Op::Subtract, int(1, bits, signed)),
                bin(int(hi, bits, signed), Op::Multiply, int(2, bits, signed)),
                bin(int(1, bits, signed), Op::Modulo, int(0, bits, signed)),
            ] {
                assert!(!execute(program(vec![print(expr)]), None).status.success());
            }
            if signed {
                for op in [Op::Divide, Op::Modulo] {
                    assert!(!execute(
                        program(vec![print(bin(
                            int(lo, bits, true),
                            op,
                            int(-1, bits, true)
                        ))]),
                        None
                    )
                    .status
                    .success());
                }
            }
        }
    }
    for (v, signed) in [(-129., true), (128., true), (-1., false), (256., false)] {
        assert!(!execute(
            program(vec![print(e(
                TypedExprKind::Cast(Box::new(fl(v, 64))),
                Type::Int { signed, bits: 8 }
            ))]),
            None
        )
        .status
        .success());
    }
}

fn check_source(source: &str) -> TypedProgram {
    lisan::checker::check(
        &lisan::parser::parse_located(
            lisan::lexer::tokenize_located(source, lisan::source::SourceId(0)).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn identical_c_aliases_lower_once_and_execute() {
    let p=check_source("خارجي دالة أ(س: ص32): ص32 = \"echo\"؛ خارجي دالة ب(س: ص32): ص32 = \"echo\"؛ غير_آمن { اطبع(أ(12))؛ اطبع(ب(34))؛ }");
    let ir = lisan::native::typed_codegen::generate(&p).unwrap();
    assert_eq!(ir.matches("declare i32 @\"echo\"").count(), 1);
    let out = execute(p, Some("int echo(int value){return value;}"));
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"12\n34\n");
}
#[test]
fn exported_float_parameters_reject_nonfinite_before_body() {
    for bits in [32, 64] {
        for nonfinite in ["__builtin_inf()", "-__builtin_inf()", "__builtin_nan(\"\")"] {
            for printing in [false, true] {
                let ty = Type::Float { bits };
                let mut p = program(vec![]);
                let mut body = vec![];
                if printing {
                    body.push(print(var(0, ty.clone())));
                }
                body.push(s(TypedStmtKind::Return(Some(var(0, ty.clone())))));
                p.functions.push(function(
                    0,
                    "guarded",
                    FunctionSig {
                        params: vec![ty.clone()],
                        result: ty,
                        abi: Abi::C,
                    },
                    vec![LocalId(0)],
                    body,
                ));
                let ct = if bits == 32 { "float" } else { "double" };
                let c=format!("extern {ct} guarded({ct});int main(void){{guarded(({ct})({nonfinite}));return 0;}}");
                let out = execute_mode(p, Some(&c), true);
                assert_eq!(out.status.code(), Some(1));
                assert!(!String::from_utf8_lossy(&out.stdout).contains("inf"));
                assert!(!String::from_utf8_lossy(&out.stdout).contains("nan"));
            }
        }
    }
}
#[test]
fn extern_aliases_reject_conflicts_and_definition_collisions() {
    let sig = FunctionSig {
        params: vec![],
        result: Type::Void,
        abi: Abi::C,
    };
    let external = |id, sig| ExternFunction {
        id: FunctionId(id),
        name: format!("alias{id}"),
        symbol: "shared".into(),
        signature: sig,
        span: Span::default(),
    };
    let mut p = program(vec![]);
    p.externs.push(external(0, sig.clone()));
    p.externs.push(external(
        1,
        FunctionSig {
            params: vec![Type::Bool],
            ..sig.clone()
        },
    ));
    assert!(lisan::native::typed_codegen::generate(&p).is_err());
    p.externs.pop();
    p.functions.push(function(1, "shared", sig, vec![], vec![]));
    assert!(lisan::native::typed_codegen::generate(&p).is_err());
}
fn address(id: u32, ty: Type) -> TypedExpr {
    e(
        TypedExprKind::Address(LocalId(id)),
        Type::Pointer(Box::new(ty)),
    )
}
fn load(pointer: TypedExpr, ty: Type) -> TypedExpr {
    e(TypedExprKind::Load(Box::new(pointer)), ty)
}
fn store(pointer: TypedExpr, value: TypedExpr) -> TypedExpr {
    e(
        TypedExprKind::Store {
            pointer: Box::new(pointer),
            value: Box::new(value),
        },
        Type::Void,
    )
}
fn local(id: u32, value: TypedExpr) -> TypedStmt {
    s(TypedStmtKind::Let {
        id: LocalId(id),
        name: format!("local{id}"),
        ty: value.ty.clone(),
        value,
    })
}
#[test]
fn c_out_parameter_mutates_addressed_local() {
    let ty = Type::Int {
        bits: 32,
        signed: true,
    };
    let mut p = program(vec![
        local(0, int(0, 32, true)),
        s(TypedStmtKind::Expr(call(
            0,
            vec![address(0, ty.clone())],
            Type::Void,
        ))),
        print(var(0, ty.clone())),
    ]);
    p.externs.push(ExternFunction {
        id: FunctionId(0),
        name: "write42".into(),
        symbol: "write42".into(),
        signature: FunctionSig {
            params: vec![Type::Pointer(Box::new(ty))],
            result: Type::Void,
            abi: Abi::C,
        },
        span: Span::default(),
    });
    let out = execute(p, Some("void write42(int *value){*value=42;}"));
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"42\n");
}
#[test]
fn scalar_pointer_load_store_roundtrips() {
    let mut values = vec![];
    for bits in [8, 16, 32, 64] {
        values.push(int(-7, bits, true));
        values.push(int(42, bits, false));
    }
    values.push(fl(1.25, 32));
    values.push(fl(2.5, 64));
    values.push(e(TypedExprKind::Bool(true), Type::Bool));
    let mut p = program(vec![]);
    for (id, value) in values.into_iter().enumerate() {
        let ty = value.ty.clone();
        p.main.push(local(id as u32, value.clone()));
        p.main.push(s(TypedStmtKind::Expr(store(
            address(id as u32, ty.clone()),
            value,
        ))));
        p.main.push(print(load(address(id as u32, ty.clone()), ty)));
    }
    let out = execute(p, None);
    assert!(out.status.success());
    assert_eq!(
        &out.stdout,
        b"-7\n42\n-7\n42\n-7\n42\n-7\n42\n1.25\n2.5\n\xd8\xb5\xd8\xad\xd9\x8a\xd8\xad\n"
    );
}
#[test]
fn null_pointer_load_and_store_trap() {
    let ty = Type::Int {
        bits: 32,
        signed: true,
    };
    let null = || {
        e(
            TypedExprKind::NullPointer,
            Type::Pointer(Box::new(ty.clone())),
        )
    };
    for expr in [load(null(), ty.clone()), store(null(), int(42, 32, true))] {
        assert_eq!(
            execute(program(vec![s(TypedStmtKind::Expr(expr))]), None)
                .status
                .code(),
            Some(1)
        );
    }
}
#[test]
fn nonfinite_float_pointer_load_traps() {
    for bits in [32, 64] {
        let ty = Type::Float { bits };
        let ptr = Type::Pointer(Box::new(ty.clone()));
        let mut p = program(vec![print(load(call(0, vec![], ptr.clone()), ty))]);
        p.externs.push(ExternFunction {
            id: FunctionId(0),
            name: "bad_pointer".into(),
            symbol: "bad_pointer".into(),
            signature: FunctionSig {
                params: vec![],
                result: ptr,
                abi: Abi::C,
            },
            span: Span::default(),
        });
        let ct = if bits == 32 { "float" } else { "double" };
        assert_eq!(execute(p,Some(&format!("{ct} *bad_pointer(void){{static {ct} value;value=__builtin_inf();return &value;}}"))).status.code(),Some(1));
    }
}
#[test]
fn invalid_pointer_ir_is_diagnosed() {
    let i32ty = Type::Int {
        bits: 32,
        signed: true,
    };
    for expr in [
        load(int(0, 32, true), i32ty.clone()),
        load(
            e(
                TypedExprKind::NullPointer,
                Type::Pointer(Box::new(Type::Void)),
            ),
            Type::Void,
        ),
        load(
            e(
                TypedExprKind::NullPointer,
                Type::Pointer(Box::new(Type::Bool)),
            ),
            i32ty.clone(),
        ),
        store(
            e(TypedExprKind::NullPointer, Type::Pointer(Box::new(i32ty))),
            fl(1., 64),
        ),
        address(999, Type::Bool),
    ] {
        let err =
            lisan::native::typed_codegen::generate(&program(vec![s(TypedStmtKind::Expr(expr))]))
                .unwrap_err();
        assert_eq!(err[0].code, "E_NATIVE_TYPED");
    }
}
#[test]
fn foreign_unaligned_storage_uses_conservative_alignment() {
    let ty = Type::Int {
        bits: 32,
        signed: true,
    };
    let ptr = Type::Pointer(Box::new(ty.clone()));
    let mut p = program(vec![
        local(0, call(0, vec![], ptr.clone())),
        s(TypedStmtKind::Expr(store(
            var(0, ptr.clone()),
            int(42, 32, true),
        ))),
        print(load(var(0, ptr.clone()), ty)),
    ]);
    p.externs.push(ExternFunction {
        id: FunctionId(0),
        name: "unaligned".into(),
        symbol: "unaligned".into(),
        signature: FunctionSig {
            params: vec![],
            result: ptr,
            abi: Abi::C,
        },
        span: Span::default(),
    });
    let ir = lisan::native::typed_codegen::generate(&p).unwrap();
    assert!(ir.contains(", align 1"));
    let out = execute(
        p,
        Some("void *unaligned(void){static unsigned char storage[16];return storage+1;}"),
    );
    assert!(out.status.success());
    assert_eq!(&out.stdout, b"42\n");
}
#[test]
fn c_out_parameter_nonfinite_local_read_traps() {
    for bits in [32, 64] {
        for nonfinite in ["__builtin_inf()", "__builtin_nan(\"\")"] {
            let ty = Type::Float { bits };
            let mut p = program(vec![
                local(0, fl(0., bits)),
                s(TypedStmtKind::Expr(call(
                    0,
                    vec![address(0, ty.clone())],
                    Type::Void,
                ))),
                print(var(0, ty.clone())),
            ]);
            p.externs.push(ExternFunction {
                id: FunctionId(0),
                name: "write_bad".into(),
                symbol: "write_bad".into(),
                signature: FunctionSig {
                    params: vec![Type::Pointer(Box::new(ty))],
                    result: Type::Void,
                    abi: Abi::C,
                },
                span: Span::default(),
            });
            let ct = if bits == 32 { "float" } else { "double" };
            assert_eq!(
                execute(
                    p,
                    Some(&format!(
                        "void write_bad({ct} *value){{*value={nonfinite};}}"
                    ))
                )
                .status
                .code(),
                Some(1)
            );
        }
    }
}
