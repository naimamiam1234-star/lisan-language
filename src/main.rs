use lisan::Interpreter;
use std::env;
use std::fs;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("لسان 0.2.0 | لغة برمجة عربية\nالاستخدام:\n  lisan ملف.لسان\n  lisan -e 'اطبع(\"مرحبا\")؛'\n  lisan       لتشغيل البيئة التفاعلية");
        return;
    }
    if args.len() == 2 && args[0] == "-e" {
        let mut interpreter = Interpreter::default();
        execute(&mut interpreter, &args[1]);
        return;
    }
    if args.len() == 1 {
        match fs::read_to_string(&args[0]) {
            Ok(src) => execute(&mut Interpreter::default(), &src),
            Err(err) => { eprintln!("تعذر فتح الملف: {}", err); std::process::exit(1); }
        }
        return;
    }
    if !args.is_empty() {
        eprintln!("وسائط غير صحيحة. اكتب lisan --help");
        std::process::exit(2);
    }
    println!("لسان 0.2.0 | اكتب خروج لإنهاء الجلسة");
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
