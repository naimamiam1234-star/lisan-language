use lisan::{check_file, Interpreter};
use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("لسان 0.6.0 | لغة برمجة عربية\nالاستخدام:\n  lisan ملف.lsn\n  lisan --check ملف.lsn  لفحص قواعد الملف واستيراداته\n  lisan --test ملف.lsn   تشغيل تحققات البرنامج\n  lisan --build ملف.lsn [--output الناتج]  بناء برنامج مستقل باستخدام Cargo\n  lisan --tokens ملف.lsn  عرض الرموز\n  lisan --ast ملف.lsn  عرض شجرة بناء التعليمات\n  lisan -e 'اطبع(\"مرحبا\")؛'\n  lisan                 البيئة التفاعلية");
        return;
    }
    if args.len() == 1 && (args[0] == "-V" || args[0] == "--version") {
        println!("لسان 0.6.0");
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
    if args.len() == 1 {
        match Interpreter::default().run_file(&args[0]) {
            Ok(lines) => for output in lines { println!("{}", output); },
            Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); }
        }
        return;
    }
    if !args.is_empty() {
        eprintln!("وسائط غير صحيحة. اكتب lisan --help");
        std::process::exit(2);
    }
    println!("لسان 0.6.0 | اكتب خروج لإنهاء الجلسة");
    let mut interpreter = Interpreter::default();
    loop {
        print!("لسان> ");
        if io::stdout().flush().is_err() { break; }
        let mut line = String::new();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 { break; }
        if line.trim() == "خروج" || line.trim() == ":q" { break; }
        match interpreter.run_source(&line) {
            Ok(lines) => for output in lines { println!("{}", output); },
            Err(err) => eprintln!("خطأ: {}", err),
        }
    }
}

fn execute(interpreter: &mut Interpreter, source: &str) {
    match interpreter.run_source(source) {
        Ok(lines) => for output in lines { println!("{}", output); },
        Err(err) => { eprintln!("خطأ: {}", err); std::process::exit(1); },
    }
}
