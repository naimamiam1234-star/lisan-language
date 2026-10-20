use lisan::{lexer,parser,native};
use std::path::PathBuf;

fn compile_ir(source:&str)->Result<String,String>{
    native::generate_llvm(&parser::parse(lexer::tokenize(source)?)?)
}

#[test]
fn native_lists_index_and_loop_have_real_llvm(){
    let ir=compile_ir("ليكن س = [1، 2، 3]؛ س[0] = 7؛ لكل (ق في س) { اطبع(ق)؛ }").unwrap();
    assert!(ir.contains("extractvalue { i64, ptr }"));
    assert!(ir.contains("getelementptr double"));
    assert!(ir.contains("foreach.cond"));
    assert!(!ir.contains("Interpreter"));
}
#[test]
fn native_strings_concatenate_compare_and_unicode_length(){
    let ir=compile_ir("ليكن ج = \"لغة \" + \"لسان\"؛ تحقق(ج == \"لغة لسان\")؛ اطبع(طول(\"سلام\"))؛").unwrap();
    assert!(ir.contains("@strcat"));
    assert!(ir.contains("@strcmp"));
    assert!(ir.contains("@lisan_utf8_len"));
}
#[test]
fn native_range_and_math_functions(){
    let ir=compile_ir("لكل (س في مدى(5، 0، -2)) { اطبع(مطلق(س))؛ }").unwrap();
    assert!(ir.contains("range.cond"));
    assert!(ir.contains("@llvm.fabs.f64"));
}
#[test]
fn native_rejects_unsupported_nested_maps(){
    let err=compile_ir("ليكن س = {\"مفتاح\": 5}؛").unwrap_err();
    assert!(err.contains("لم تُنفّذ"));
}
#[test]
fn native_collections_binary_executes_without_interpreter(){
    let source=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/native_collections.lsn");
    let target=std::env::temp_dir().join(format!("lisan-collections-{}{}",std::process::id(),std::env::consts::EXE_SUFFIX));
    native::build_native(&source,&target).unwrap();
    let process=std::process::Command::new(&target).output().unwrap();
    let _=std::fs::remove_file(&target);
    assert!(process.status.success(),"{}",String::from_utf8_lossy(&process.stderr));
    assert_eq!(String::from_utf8_lossy(&process.stdout).replace("\r\n", "\n"),"مجموع القيم = 65\nعدد الحروف = 4\n5\n3\n1\nنجحت الاختبارات الأصلية\n");
}

#[test]
fn native_math_binary_executes_without_interpreter() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/native_math.lsn");
    let target = std::env::temp_dir().join(format!("lisan-math-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    native::build_native(&source, &target).unwrap();
    let process = std::process::Command::new(&target).output().unwrap();
    let _ = std::fs::remove_file(&target);
    assert!(process.status.success(), "{}", String::from_utf8_lossy(&process.stderr));
    assert_eq!(String::from_utf8(process.stdout).unwrap().replace("\r\n", "\n"), "9\n9\n3\n3\n8\nلغة لسان 8\n");
}
