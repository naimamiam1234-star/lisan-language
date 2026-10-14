use lisan::{run, Interpreter};

#[test]
fn arabic_hello_and_digits() {
    assert_eq!(run(include_str!("../examples/hello.lsn")).unwrap(),
               ["مرحبا يا العالم!", "عددنا: 12"]);
}

#[test]
fn loops_and_scopes() {
    assert_eq!(run(include_str!("../examples/loops.lsn")).unwrap(),
               ["فردي: 1", "زوجي: 2", "فردي: 3", "زوجي: 4", "فردي: 5"]);
}

#[test]
fn recursive_functions() {
    assert_eq!(run(include_str!("../examples/functions.lsn")).unwrap(), ["مضروب 5 = 120"]);
}

#[test]
fn precedence_and_comparisons() {
    assert_eq!(run("اطبع(2 + 3 * 4)؛ اطبع((2 + 3) * 4)؛ اطبع(5 >= 4 && !خطأ)؛").unwrap(),
               ["14", "20", "صحيح"]);
}

#[test]
fn accepts_both_semicolons_and_comma_styles() {
    assert_eq!(run("دالة جمع(س، ص) { أرجع س + ص; } اطبع(جمع(3، 4));").unwrap(), ["7"]);
}

#[test]
fn print_escaped_string() {
    assert_eq!(run("اطبع(\"سطر\\nثان\")؛").unwrap(), ["سطر\nثان"]);
}

#[test]
fn assignment_and_block_scope() {
    assert_eq!(run("دع س = 10؛ { دع س = 20؛ اطبع(س)؛ } اطبع(س)؛ س = 30؛ اطبع(س)؛").unwrap(),
               ["20", "10", "30"]);
}

#[test]
fn division_by_zero_is_error() {
    assert!(run("اطبع(3 / 0)؛").unwrap_err().contains("صفر"));
}

#[test]
fn missing_variable_is_error() {
    assert!(run("اطبع(مجهول)؛").unwrap_err().contains("غير معرّف"));
}

#[test]
fn syntax_error_reports_line() {
    assert!(run("دع س = 2\nاطبع(س)؛").unwrap_err().contains("السطر 2"));
}

#[test]
fn recursion_limit_prevents_excessive_calls() {
    let error = run("دالة تكرار() { أرجع تكرار()؛ } اطبع(تكرار())؛").unwrap_err();
    assert!(error.contains("الحد الآمن"));
}

#[test]
fn repl_preserves_state() {
    let mut interpreter = Interpreter::default();
    assert!(interpreter.run_source("دع قيمة = 7؛").unwrap().is_empty());
    assert_eq!(interpreter.run_source("اطبع(قيمة)؛").unwrap(), ["7"]);
}

#[test]
fn short_circuit_avoids_division_by_zero() {
    assert_eq!(run("اطبع(خطأ && (1 / 0 > 0))؛ اطبع(صحيح || (1 / 0 > 0))؛").unwrap(),
               ["خطأ", "صحيح"]);
}

#[test]
fn invalid_condition_type_is_error() {
    assert!(run("إذا (1) { اطبع(1)؛ }").unwrap_err().contains("الشرط"));
}

#[test]
fn lists_and_for_each_example() {
    assert_eq!(run(include_str!("../examples/lists.lsn")).unwrap(),
        ["المجموع: 90", "عدد القياسات: 4", "القياس الثاني: 22", "هل يوجد 26؟ صحيح"]);
}

#[test]
fn unicode_text_and_conversion_example() {
    assert_eq!(run(include_str!("../examples/text.lsn")).unwrap(),
        ["طول النص: 4", "ل", "س", "ا", "ن", "قيمة نصية: 13"]);
}

#[test]
fn empty_lists_and_concat() {
    assert_eq!(run("دع أ = []؛ دع ب = [1، 2] + [3]؛ اطبع(طول(أ))؛ اطبع(ب[2])؛").unwrap(),
        ["0", "3"]);
}

#[test]
fn nested_indexing() {
    assert_eq!(run("دع س = [[1، 2]، [3، 4]]؛ اطبع(س[1][0])؛").unwrap(), ["3"]);
}

#[test]
fn list_index_assignment() {
    assert_eq!(run("دع س = [1، 2]؛ س[0] = 9؛ اطبع(س[0])؛").unwrap(), ["9"]);
}

#[test]
fn for_each_has_own_iteration_scope() {
    assert_eq!(run("دع س = 10؛ لكل (س في [1، 2]) { اطبع(س)؛ } اطبع(س)؛").unwrap(),
        ["1", "2", "10"]);
}

#[test]
fn for_each_return_from_function() {
    let source = "دالة اول_كبير(قائمة) { لكل (س في قائمة) { إذا (س > 5) { أرجع س؛ } } أرجع لاشيء؛ } اطبع(اول_كبير([2، 7، 9]))؛";
    assert_eq!(run(source).unwrap(), ["7"]);
}

#[test]
fn arabic_logic_words_and_short_circuit() {
    assert_eq!(run("اطبع(صحيح و ليس خطأ)؛ اطبع(خطأ أو صحيح)؛").unwrap(),
        ["صحيح", "صحيح"]);
}

#[test]
fn range_builtin() {
    assert_eq!(run("لكل (س في مدى(2، 5)) { اطبع(س)؛ }").unwrap(), ["2", "3", "4"]);
}

#[test]
fn list_equality_and_type_name() {
    assert_eq!(run("اطبع([1، 2] == [1، 2])؛ اطبع(نوع([1]))؛ اطبع(نوع(لاشيء))؛").unwrap(),
        ["صحيح", "قائمة", "لاشيء"]);
}

#[test]
fn builtin_argument_count_error() {
    assert!(run("اطبع(طول())؛").unwrap_err().contains("معاملات"));
}

#[test]
fn invalid_index_error() {
    assert!(run("اطبع([1][2])؛").unwrap_err().contains("خارج حدود"));
    assert!(run("اطبع([1][-1])؛").unwrap_err().contains("غير سالب"));
    assert!(run("اطبع([1][0.5])؛").unwrap_err().contains("غير سالب"));
}

#[test]
fn cannot_assign_to_string_index() {
    assert!(run("دع س = \"لسان\"؛ س[0] = \"x\"؛").unwrap_err().contains("قائمة"));
}

#[test]
fn range_size_is_bounded() {
    assert!(run("دع س = مدى(100001)؛").unwrap_err().contains("الحد الأقصى"));
}

#[test]
fn checks_builtin_input_types() {
    assert!(run("اطبع(طول(5))؛").unwrap_err().contains("قائمة أو نص"));
    assert!(run("اطبع(عدد(صحيح))؛").unwrap_err().contains("رقمًا"));
}

#[test]
fn latin_only_number_conversion() {
    assert_eq!(run("اطبع(عدد(\"12.5\") + 0.5)؛").unwrap(), ["13"]);
    assert!(run("اطبع(عدد(\"١٢\"))؛").unwrap_err().contains("اللاتينية"));
}

#[test]
fn list_reassignment_persists_between_repl_lines() {
    let mut interpreter = Interpreter::default();
    interpreter.run_source("دع س = [1]؛").unwrap();
    interpreter.run_source("س = أضف(س، 2)؛").unwrap();
    assert_eq!(interpreter.run_source("اطبع(س[1])؛").unwrap(), ["2"]);
}

#[test]
fn malformed_foreach_reports_syntax_error() {
    assert!(run("لكل (س من [1]) { اطبع(س)؛ }").unwrap_err().contains("متوقع 'في'"));
}
