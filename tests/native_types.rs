use lisan::{
    checker, lexer, parser,
    source::SourceId,
    typed::{TypedExprKind, TypedStmtKind},
    types::Type,
};
fn check(s: &str) -> Result<lisan::typed::TypedProgram, Vec<lisan::diagnostic::Diagnostic>> {
    checker::check(
        &parser::parse_located(lexer::tokenize_located(s, SourceId(0)).unwrap()).unwrap(),
    )
}
fn constant(s: &str) -> (Type, i128) {
    let p = check(s).unwrap();
    match &p.main[0].kind {
        TypedStmtKind::Let { value, .. } => match value.kind {
            TypedExprKind::Int(n) => (value.ty.clone(), n),
            _ => panic!("not integer"),
        },
        _ => panic!(),
    }
}
#[test]
fn u64_max_is_exact() {
    assert_eq!(
        constant("ليكن س: ط64 = 18446744073709551615؛").1,
        18446744073709551615
    );
}
#[test]
fn signed_min_is_valid() {
    for (w, v) in [
        (8, -128i128),
        (16, -32768),
        (32, -2147483648),
        (64, -9223372036854775808),
    ] {
        assert_eq!(constant(&format!("ليكن س: ص{w} = {v}؛")).1, v);
    }
}
#[test]
fn integer_boundaries_reject() {
    for w in [8, 16, 32, 64] {
        for (prefix, n) in [
            ("ص", 1i128 << (w - 1)),
            ("ص", -(1i128 << (w - 1)) - 1),
            ("ط", 1i128 << w),
            ("ط", -1),
        ] {
            assert!(
                check(&format!("ليكن س: {prefix}{w} = {n}؛")).is_err(),
                "{prefix}{w}: {n}"
            );
        }
    }
}
#[test]
fn legacy_number_is_f64() {
    let p = check("ليكن س = 4؛").unwrap();
    match &p.main[0].kind {
        TypedStmtKind::Let { value, .. } => assert_eq!(value.ty, Type::Float { bits: 64 }),
        _ => panic!(),
    }
}
#[test]
fn conversion_rules_are_explicit() {
    for s in [
        "ليكن س: ص8 = 4؛ ليكن ص: ص16 = س؛",
        "ليكن س: ص8 = 4؛ ليكن ص: ط8 = س؛",
    ] {
        assert_eq!(check(s).unwrap_err()[0].code, "E_TYPE_MISMATCH");
    }
    assert!(check("ليكن س: ص8 = 4؛ ليكن ص: ص16 = تحويل<ص16>(س)؛").is_ok());
}
#[test]
fn typed_function_control_flow() {
    assert!(
        check("دالة ج(س: ص32): ص32 { إذا (س == 0) { أرجع 1؛ } وإلا { أرجع س * ج(س - 1)؛ } }")
            .is_ok()
    );
    assert_eq!(
        check("دالة ج(س: ص32): ص32 { إذا (س == 0) { أرجع 1؛ } }").unwrap_err()[0].code,
        "E_RETURN"
    );
}
#[test]
fn extern_and_export_symbols() {
    assert!(check("خارجي دالة ج(س: ص8): ص8 = \"echo\"؛ صدّر دالة ع(س: ص8): ص8 = \"lisan_echo\" { غير_آمن { أرجع ج(س)؛ } }").is_ok());
    for symbol in ["main", "lisan_fn_1", "llvm.foo", "a\"b", ""] {
        let s = format!("خارجي دالة ج(): فراغ = {:?}؛", symbol);
        assert!(check(&s).is_err());
    }
}
#[test]
fn boolean_expression_hints_describe_results() {
    for expression in [
        "(س < 2) == صحيح",
        "صحيح == (س < 2)",
        "!(س < 2) == خطأ",
        "(س < 2) == (س > 0)",
    ] {
        assert!(
            check(&format!("ليكن س: ص32 = 1؛ ليكن ب = {expression}؛")).is_ok(),
            "{expression}"
        );
    }
}
#[test]
fn lexical_scopes_and_loop_returns() {
    assert!(check("ليكن س: ص32 = 1؛ { ليكن س: ص8 = 2؛ } ليكن ب: ص32 = س؛").is_ok());
    assert!(check("{ ليكن س: ص32 = 1؛ } اطبع(س)؛").is_err());
    assert!(check("دالة ج(): ص32 { طالما (صحيح) { أرجع 1؛ } }").is_ok());
    assert!(check("دالة ج(): ص32 { طالما (صحيح) { توقف؛ } }").is_err());
}
#[test]
fn constant_casts_check_computed_values() {
    for expression in ["100 + 100", "-(-200)", "تحويل<ص16>(200)", "100.0 * 2.0"] {
        assert!(
            check(&format!("ليكن س = تحويل<ص8>({expression})؛")).is_err(),
            "{expression}"
        );
    }
    assert!(check("ليكن س = تحويل<ص8>(-128.9)؛ ليكن ب = تحويل<ط8>(-0.5)؛").is_ok());
}
#[test]
fn foreign_aliases_and_unsafe_scope() {
    assert!(check("خارجي دالة أ(س: ص32): ص32 = \"echo\"؛ خارجي دالة ب(س: ص32): ص32 = \"echo\"؛ غير_آمن { ب(1)؛ }").is_ok());
    assert!(
        check("خارجي دالة أ(س: ص32): ص32 = \"echo\"؛ خارجي دالة ب(س: ص8): ص8 = \"echo\"؛").is_err()
    );
    assert_eq!(
        check("خارجي دالة أ(): فراغ = \"echo\"؛ غير_آمن { أ()؛ } أ()؛").unwrap_err()[0].code,
        "E_UNSAFE"
    );
}
#[test]
fn constant_integer_to_f32_rounds_once() {
    let p = check("ليكن س = تحويل<ط64>(تحويل<ع32>(تحويل<ط64>(18446743523953737727)))؛").unwrap();
    assert!(matches!(p.main[0].kind, TypedStmtKind::Let { .. }));
}
