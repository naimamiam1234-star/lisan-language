use lisan::{run, Interpreter};

#[test]
fn break_stops_only_innermost_for_each() {
    let src = r#"
ليكن عدد = 0؛
لكل (س في مدى(3)) {
    لكل (ص في مدى(5)) {
        إذا (ص == 2) { توقف؛ }
        عدد = عدد + 1؛
    }
}
اطبع(عدد)؛
"#;
    assert_eq!(run(src).unwrap(), ["6"]);
}

#[test]
fn continue_skips_iteration_without_escaping() {
    let src = r#"
ليكن ناتج = 0؛
لكل (س في مدى(7)) {
    إذا (س % 2 == 0) { تابع؛ }
    ناتج = ناتج + س؛
}
اطبع(ناتج)؛
"#;
    assert_eq!(run(src).unwrap(), ["9"]);
}

#[test]
fn while_break_and_continue() {
    let src = r#"
ليكن س = 0؛
ليكن ج = 0؛
بينما (س < 20) {
    س = س + 1؛
    إذا (س < 4) { تابع؛ }
    إذا (س == 7) { توقف؛ }
    ج = ج + س؛
}
اطبع(ج)؛
"#;
    assert_eq!(run(src).unwrap(), ["15"]); // 4+5+6
}

#[test]
fn function_return_traverses_loop() {
    let src = r#"
دالة ابحث() {
    لكل (ن في مدى(0، 8)) {
        إذا (ن == 5) { أرجع ن؛ }
    }
    أرجع -1؛
}
اطبع(ابحث())؛
"#;
    assert_eq!(run(src).unwrap(), ["5"]);
}

#[test]
fn cannot_break_or_continue_outside_loop() {
    for keyword in ["توقف", "تابع"] {
        let src = format!("{}؛", keyword);
        assert!(run(&src).unwrap_err().contains("خارج الحلقة"));
        let src = format!("دالة تجربة() {{ {}؛ }}", keyword);
        assert!(run(&src).unwrap_err().contains("خارج الحلقة"));
    }
}

#[test]
fn function_declared_in_loop_cannot_control_callers_loop() {
    let src = r#"لكل (س في [1]) { دالة تجربة() { توقف؛ } }"#;
    assert!(run(src).unwrap_err().contains("خارج الحلقة"));
}

#[test]
fn return_outside_function_is_rejected() {
    assert!(run("أرجع 12؛").unwrap_err().contains("خارج الدالة"));
}

#[test]
fn power_is_right_associative_with_unary_precedence() {
    assert_eq!(run("اطبع(2 ** 3 ** 2)؛ اطبع(-2 ** 2)؛ اطبع(2 ** -2)؛").unwrap(),
        ["512", "-4", "0.25"]);
}

#[test]
fn scientific_numbers_use_latin_digits() {
    assert_eq!(run("اطبع(1e3 + 2.5E-1)؛").unwrap(), ["1000.25"]);
    assert!(run("اطبع(1e+)؛").unwrap_err().contains("أس العدد"));
    assert!(run("اطبع(1e9999)؛").unwrap_err().contains("النطاق"));
    assert!(run("اطبع(١e3)؛").unwrap_err().contains("اللاتينية"));
    assert!(run("اطبع(१२)؛").unwrap_err().contains("اللاتينية"));
}

#[test]
fn exponential_overflow_is_rejected() {
    assert!(run("اطبع(1e200 * 1e200)؛").unwrap_err().contains("النطاق"));
    assert!(run("اطبع(10 ** 1000)؛").unwrap_err().contains("النطاق"));
    assert!(run("اطبع((-1) ** 0.5)؛").unwrap_err().contains("النطاق"));
}

#[test]
fn range_supports_descending_and_variable_steps() {
    assert_eq!(run("اطبع(مدى(5، 0، -2))؛ اطبع(مدى(0، 7، 3))؛").unwrap(),
        ["[5، 3، 1]", "[0، 3، 6]"]);
    assert_eq!(run("اطبع(مدى(5، 2))؛ اطبع(مدى(0، 3، -1))؛").unwrap(),
        ["[]", "[]"]);
}

#[test]
fn range_rejects_bad_steps_and_sizes() {
    assert!(run("اطبع(مدى(1، 10، 0))؛").unwrap_err().contains("صفر"));
    assert!(run("اطبع(مدى(1، 100000، 1))؛").unwrap_err().contains("الحد الأقصى"));
    assert!(run("اطبع(مدى(1، 10، 0.5))؛").unwrap_err().contains("صحيحة"));
}

#[test]
fn assertions_pass_and_fail_with_clear_errors() {
    assert_eq!(run("تحقق(صحيح)؛ تحقق_يساوي([1، 2]، [1، 2])؛ اطبع(7)؛").unwrap(), ["7"]);
    assert!(run("تحقق(خطأ)؛").unwrap_err().contains("فشل التحقق"));
    assert!(run("تحقق_يساوي(2، 3)؛").unwrap_err().contains("فشل التحقق"));
    assert!(run("تحقق(8)؛").unwrap_err().contains("قيمة منطقية"));
}

#[test]
fn interpreter_retains_bindings_after_continue_and_break() {
    let mut interpreter = Interpreter::default();
    interpreter.run_source("ليكن ن = 0؛").unwrap();
    interpreter.run_source("لكل (س في مدى(5)) { إذا (س == 2) { تابع؛ } ن = ن + 1؛ }").unwrap();
    assert_eq!(interpreter.run_source("اطبع(ن)؛").unwrap(), ["4"]);
}

#[test]
fn bundled_ast_supports_control_flow_and_power() {
    let statements = lisan::parser::parse(lisan::lexer::tokenize(
        "لكل (س في مدى(4)) { إذا (س == 2) { تابع؛ } إذا (س > 2) { توقف؛ } اطبع(س ** 2)؛ }"
    ).unwrap()).unwrap();
    let source = lisan::bundle::generate_rust_main(&statements).unwrap();
    assert!(source.contains("Stmt::Break"));
    assert!(source.contains("Stmt::Continue"));
    assert!(source.contains("Op::Power"));
}
