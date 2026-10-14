use lisan::{check_file, Interpreter};
use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("لسان 0.3.0 | لغة برمجة عربية\nالاستخدام:\n  lisan ملف.lsn\n  lisan --check ملف.lsn  لفحص قواعد الملف واستيراداته\n  lisan -e 'اطبع(\"مرحبا\")؛'\n  lisan                 البيئة التفاعلية");
        return;
    }
    if args.len() == 1 && (args[0] == "-V" || args[0] == "--version") {
        println!("لسان 0.3.0");
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
    println!("لسان 0.3.0 | اكتب خروج لإنهاء الجلسة");
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
