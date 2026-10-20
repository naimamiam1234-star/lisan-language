use lisan::{
    checker, diagnostic::Diagnostic, lexer, parser, source::SourceId, typed::TypedProgram,
};
fn check(source: &str) -> Result<TypedProgram, Vec<Diagnostic>> {
    checker::check(
        &parser::parse_located(lexer::tokenize_located(source, SourceId(0)).unwrap()).unwrap(),
    )
}
fn fails(source: &str, code: &str) {
    let errors = check(source).unwrap_err();
    assert_eq!(errors[0].code, code, "{errors:?}");
}
#[test]
fn scalar_out_parameter_and_dereference() {
    check("خارجي دالة املأ(س: مؤشر<ص32>): فراغ = \"fill\"؛ ليكن س: ص32 = 0؛ ليكن م = عنوان(س)؛ غير_آمن { املأ(م)؛ خزن(م، 42)؛ ليكن ن: ص32 = تحميل(م)؛ }").unwrap();
    for ty in ["ص8", "ط64", "ع32", "ع64", "منطقي"] {
        let value = if ty == "منطقي" { "صحيح" } else { "1" };
        check(&format!("ليكن س: {ty} = {value}؛ غير_آمن {{ خزن(عنوان(س)، {value})؛ ليكن ن: {ty} = تحميل(عنوان(س))؛ }}")).unwrap();
    }
}
#[test]
fn loads_and_stores_are_unsafe_and_exactly_typed() {
    fails("ليكن س: ص32 = 0؛ تحميل(عنوان(س))؛", "E_UNSAFE");
    fails("ليكن س: ص32 = 0؛ خزن(عنوان(س)، 1)؛", "E_UNSAFE");
    fails(
        "ليكن س: ص32 = 0؛ ليكن ن: ص8 = 1؛ غير_آمن { خزن(عنوان(س)، ن)؛ }",
        "E_TYPE_MISMATCH",
    );
    fails(
        "ليكن س: ص32 = 0؛ غير_آمن { ليكن ن = خزن(عنوان(س)، 1)؛ }",
        "E_TYPE_MISMATCH",
    );
}
#[test]
fn local_addresses_cannot_return_even_through_aliases_and_calls() {
    for body in [
        "أرجع عنوان(س)؛",
        "ليكن م = عنوان(س)؛ ليكن ن = م؛ أرجع ن؛",
        "أرجع هوية(عنوان(س))؛",
        "أرجع هوية(هوية(عنوان(س)))؛",
        "غير_آمن { أرجع خارجي_هوية(عنوان(س))؛ }",
    ] {
        let source = format!("دالة هوية(م: مؤشر<ص32>): مؤشر<ص32> {{ أرجع م؛ }} خارجي دالة خارجي_هوية(م: مؤشر<ص32>): مؤشر<ص32> = \"identity\"؛ دالة سيئة(): مؤشر<ص32> {{ ليكن س: ص32 = 1؛ {body} }}");
        fails(&source, "E_POINTER_ESCAPE");
    }
    check("دالة هوية(م: مؤشر<ص32>): مؤشر<ص32> { أرجع م؛ } دالة ن(م: مؤشر<ص32>): مؤشر<ص32> { أرجع هوية(م)؛ }").unwrap();
}
#[test]
fn pointer_bindings_and_parameters_are_immutable_including_loops() {
    for body in [
        "م = عنوان(س)؛",
        "طالما (صحيح) { م = عنوان(س)؛ توقف؛ }",
        "إذا (صحيح) { م = عنوان(س)؛ }",
    ] {
        fails(
            &format!("ليكن س: ص32 = 1؛ ليكن م = مؤشر_فارغ<ص32>()؛ {body}"),
            "E_POINTER_IMMUTABLE",
        );
    }
    fails(
        "دالة ج(م: مؤشر<ص32>): فراغ { م = مؤشر_فارغ<ص32>()؛ }",
        "E_POINTER_IMMUTABLE",
    );
}
#[test]
fn unsupported_pointer_containers_and_nonlocals_fail() {
    for source in [
        "عنوان(1)؛",
        "ليكن س = \"abc\"؛ عنوان(س)؛",
        "ليكن م = مؤشر_فارغ<ص32>()؛ عنوان(م)؛",
        "غير_آمن { تحميل(مؤشر_فارغ<فراغ>())؛ }",
        "غير_آمن { تحميل(مؤشر_فارغ<مؤشر<ص32>>())؛ }",
    ] {
        assert!(check(source).is_err(), "{source}");
    }
    fails(
        "ليكن س: ص32 = 1؛ ليكن م = عنوان(س)؛ غير_آمن { خزن(م، م)؛ }",
        "E_TYPE_MISMATCH",
    );
    check("غير_آمن { تحميل(مؤشر_فارغ<ص32>())؛ }").unwrap();
}

#[test]
fn call_result_inherits_all_pointer_arguments() {
    fails("دالة اختر(أ: مؤشر<ص32>، ب: مؤشر<ص32>): مؤشر<ص32> { أرجع أ؛ } دالة سيئة(): مؤشر<ص32> { ليكن س: ص32 = 1؛ أرجع اختر(مؤشر_فارغ<ص32>()، عنوان(س))؛ }", "E_POINTER_ESCAPE");
    fails(
        "خارجي دالة ج(م: مؤشر<ص32>): فراغ = \"write_value\"؛ ليكن س: ص32 = 1؛ ج(عنوان(س))؛",
        "E_UNSAFE",
    );
}
