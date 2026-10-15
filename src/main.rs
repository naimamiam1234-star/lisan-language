use lisan::{check_file, Interpreter};
use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("لسان 0.7.0 | لغة برمجة عربية\nالاستخدام:\n  lisan ملف.lsn  يترجم إلى ملف آلة أصلي (الخيار الافتراضي)\n  lisan --interpret ملف.lsn  المسار القديم للتوافق فقط\n  lisan --check ملف.lsn  لفحص قواعد الملف واستيراداته\n  lisan --test ملف.lsn   تشغيل تحققات البرنامج\n  lisan --native ملف.lsn --output البرنامج  ترجمة إلى ملف آلة أصلي عبر Clang\n  lisan --emit-llvm ملف.lsn --output الملف.ll  إخراج LLVM IR\n  lisan --build ملف.lsn [--output الناتج]  الطريقة القديمة (تضم المفسّر)\n  lisan --tokens ملف.lsn  عرض الرموز\n  lisan --ast ملف.lsn  عرض شجرة بناء التعليمات\n  lisan -e 'اطبع(\"مرحبا\")؛'\n  lisan                 عرض تعليمات المترجم");
        return;
    }
    if args.len() == 1 && (args[0] == "-V" || args[0] == "--version") {
        println!("لسان 0.7.0");
        return;
    }
    if args.len() >= 2 && (args[0] == "--native" || args[0] == "--emit-llvm") {
        if args.len() != 2 && !(args.len() == 4 && args[2] == "--output") {
            eprintln!("الاستخدام: lisan --native ملف.lsn [--output الناتج]، أو --emit-llvm");
            std::process::exit(2);
        }
        let input = std::path::Path::new(&args[1]);
        let default_name = if args[0] == "--emit-llvm" { "program.ll" }
                           else if cfg!(windows) { "program.exe" } else { "program" };
        let target = std::path::Path::new(if args.len() == 4 { args[3].as_str() } else { default_name });
        let result = if args[0] == "--emit-llvm" {
            lisan::native::emit_file(input).and_then(|ir| {
                std::fs::write(target, ir).map_err(|e| format!("تعذر حفظ LLVM IR: {}",e))?;
                Ok(target.to_path_buf())
            })
        } else {
            lisan::native::build_native(input, target)
        };
        match result {
            Ok(path) => println!("نجحت الترجمة الأصلية: {}", path.display()),
            Err(error) => { eprintln!("خطأ في الترجمة الأصلية: {}",error); std::process::exit(1); }
        }
        return;
    }
    if !args.is_empty() && args[0] == "--build" {
        if args.len() != 2 && args.len() != 4 {
            eprintln!("الاستخدام: lisan --build ملف.lsn [--output الناتج]");
            std::process::exit(2);
        }
        if args.len() == 4 && args[2] != "--output" {
            eprintln!("الاستخدام: lisan --build ملف.lsn [--output الناتج]");
            std::process::exit(2);
        }
        let target = if args.len() == 4 { Some(std::path::Path::new(&args[3])) } else { None };
        match lisan::bundle::build_executable(std::path::Path::new(&args[1]), target) {
            Ok(path) => println!("أُنشئ البرنامج المستقل: {}", path.display()),
            Err(err) => { eprintln!("خطأ في البناء: {}", err); std::process::exit(1); }
        }
        return;
    }
    if args.len() == 2 && (args[0] == "--tokens" || args[0] == "--ast") {
        let source = match std::fs::read_to_string(&args[1]) {
            Ok(text) => text,
            Err(err) => { eprintln!("تعذّرت قراءة الملف: {}", err); std::process::exit(1); }
        };
        let tokens = match lisan::lexer::tokenize(&source) {
            Ok(tokens) => tokens,
            Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); }
        };
        if args[0] == "--tokens" {
            for token in tokens { println!("{}:{} {:?}", token.line, token.column, token.kind); }
        } else {
            match lisan::parser::parse(tokens) {
                Ok(ast) => println!("{:#?}", ast),
                Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); }
            }
        }
        return;
    }
    if args.len() == 2 && args[0] == "--test" {
        match Interpreter::default().run_file(&args[1]) {
            Ok(_) => println!("نجحت اختبارات لسان"),
            Err(err) => { eprintln!("فشلت اختبارات لسان: {}", err); std::process::exit(1); }
        }
        return;
    }
    if args.len() == 2 && args[0] == "--check" {
        match check_file(&args[1]) {
            Ok(()) => println!("نجح فحص ملفات لسان"),
            Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); }
        }
        return;
    }
    if args.len() == 2 && args[0] == "-e" {
        execute(&mut Interpreter::default(), &args[1]);
        return;
    }
    if args.len() == 2 && args[0] == "--interpret" {
        match Interpreter::default().run_file(&args[1]) {
            Ok(lines) => for output in lines { println!("{}", output); },
            Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); }
        }
        return;
    }
    if args.len() == 1 && args[0].ends_with(".lsn") {
        let source = std::path::Path::new(&args[0]);
        let stem = source.file_stem().unwrap().to_string_lossy();
        let name = if cfg!(windows) { format!("{}.exe", stem) } else { stem.to_string() };
        match lisan::native::build_native(source, std::path::Path::new(&name)) {
            Ok(path) => println!("أُنشئ الملف التنفيذي الأصلي: {}", path.display()),
            Err(error) => { eprintln!("خطأ في الترجمة الأصلية: {}", error); std::process::exit(1); }
        }
        return;
    }
    if !args.is_empty() {
        eprintln!("وسائط غير صحيحة. اكتب lisan --help");
        std::process::exit(2);
    }
    println!("لسان: مترجم عربي أصلي. الاستخدام: lisan ملف.lsn أو lisan --native ملف.lsn --output البرنامج");
}

fn execute(interpreter: &mut Interpreter, source: &str) {
    match interpreter.run_source(source) {
        Ok(lines) => for output in lines { println!("{}", output); },
        Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); },
    }
}
