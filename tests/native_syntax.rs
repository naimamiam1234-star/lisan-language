use lisan::{
    lexer::tokenize_located,
    parser::parse_located,
    source::{SourceId, SourceMap},
    syntax::{self, ExprKind, StmtKind},
};
fn parse(s: &str) -> syntax::Program {
    parse_located(tokenize_located(s, SourceId(0)).unwrap()).unwrap()
}
#[test]
fn typed_signature_round_trips() {
    let p =
        parse("دالة جمع(أ: ص32، ب: ص32): ص32 { أرجع أ + ب؛ } ليكن س: ط64 = 18446744073709551615؛");
    match &p.statements[0].kind {
        StmtKind::Function(f) => {
            assert_eq!(f.params[0].name, "أ");
            assert!(f.params[0].ty.is_some());
            assert!(f.result.is_some());
        }
        _ => panic!(),
    }
    match &p.statements[1].kind {
        StmtKind::Let { value, .. } => match &value.kind {
            ExprKind::Number(n) => assert_eq!(n.text, "18446744073709551615"),
            _ => panic!(),
        },
        _ => panic!(),
    }
}
#[test]
fn extern_requires_complete_signature() {
    for s in [
        "خارجي دالة س(أ): ص32 = \"foo\"؛",
        "خارجي دالة س(أ: ص32) = \"foo\"؛",
        "خارجي دالة س(): ص32؛",
    ] {
        assert!(parse_located(tokenize_located(s, SourceId(0)).unwrap()).is_err());
    }
}
#[test]
fn export_definition_has_symbol_and_body() {
    let p = parse("صدّر دالة إعداد(): فراغ = \"lisan_setup\" { أرجع؛ }");
    assert!(
        matches!(&p.statements[0].kind, StmtKind::ExportFunction{symbol,..} if symbol=="lisan_setup")
    );
}
#[test]
fn legacy_ast_adapter_preserves_program() {
    let p = parse("ليكن س = 2؛ اطبع(س + 3)؛");
    let legacy = syntax::to_legacy(&p).unwrap();
    assert_eq!(
        lisan::native::generate_llvm(&legacy).unwrap(),
        lisan::native::generate_llvm(
            &lisan::parser::parse(lisan::lexer::tokenize("ليكن س = 2؛ اطبع(س + 3)؛").unwrap())
                .unwrap()
        )
        .unwrap()
    );
    assert!(syntax::to_legacy(&parse("ليكن س: ص32 = 2؛")).is_err());
}
#[test]
fn arabic_span_renders_scalar_column() {
    let mut sm = SourceMap::default();
    let id = sm.add("مثال.lsn", "ليكن س: مجهول = 1؛");
    let e = parse_located(tokenize_located(sm.text(id).unwrap(), id).unwrap()).unwrap_err();
    assert!(e.render(&sm).contains("مثال.lsn:1:9"), "{}", e.render(&sm));
}
#[test]
fn imports_preserve_locations_and_deduplicate() {
    let dir = std::env::temp_dir().join(format!("lisan-located-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.lsn"),
        "استورد \"dep.lsn\"؛ استورد \"dep.lsn\"؛",
    )
    .unwrap();
    std::fs::write(dir.join("dep.lsn"), "ليكن س: ص8 = 128؛").unwrap();
    let loaded = lisan::modules::load_program(&dir.join("main.lsn")).unwrap();
    assert_eq!(loaded.program.statements.len(), 1);
    let error = lisan::checker::check(&loaded.program)
        .unwrap_err()
        .remove(0);
    assert!(error.render(&loaded.sources).contains("dep.lsn:1:14"));
    std::fs::write(dir.join("dep.lsn"), "استورد \"main.lsn\"؛").unwrap();
    let error = lisan::modules::load_program(&dir.join("main.lsn"))
        .unwrap_err()
        .remove(0);
    assert_eq!(error.code, "E_IMPORT");
    assert!(error.message.contains("dep.lsn:1:1"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn contextual_cast_name_still_allows_legacy_comparison() {
    let source = "ليكن تحويل = 1؛ ليكن س = 2؛ اطبع(تحويل < س)؛";
    let legacy = lisan::parser::parse(lisan::lexer::tokenize(source).unwrap()).unwrap();
    let located = parse(source);
    assert!(!syntax::requires_typed(&located));
    assert_eq!(
        lisan::native::generate_llvm(&legacy).unwrap(),
        lisan::native::generate_llvm(&syntax::to_legacy(&located).unwrap()).unwrap()
    );
}
#[test]
fn lexer_error_spans_are_structural() {
    for (source, offending) in [
        ("اطبع(\"a\\q\");", "q"),
        ("اطبع(\"abc\\", ""),
        ("\nاطبع(1e999);", "1e999"),
    ] {
        let error = tokenize_located(source, SourceId(7)).unwrap_err();
        let start = if offending.is_empty() {
            source.len()
        } else {
            source.find(offending).unwrap()
        };
        assert_eq!(error.primary.source, SourceId(7));
        assert_eq!(error.primary.start, start, "{source}");
        assert_eq!(&source[error.primary.start..error.primary.end], offending);
        let mut sources = SourceMap::default();
        for _ in 0..7 {
            sources.add("placeholder.lsn", "");
        }
        sources.add("error.lsn", source);
        let expected = if offending == "q" {
            (1, 9)
        } else if offending.is_empty() {
            (1, 11)
        } else {
            (2, 6)
        };
        assert_eq!(sources.line_column(error.primary), expected);
        assert!(error
            .render(&sources)
            .contains(&format!("error.lsn:{}:{}", expected.0, expected.1)));
        assert_eq!(lisan::lexer::tokenize(source).unwrap_err(), error.message);
    }
}
