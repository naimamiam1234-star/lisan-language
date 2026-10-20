use lisan::{check_file, check_native_file, Interpreter};
use std::env;

fn main() {
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("لسان {} | لغة برمجة عربية\nالاستخدام:\n  lisan ملف.lsn  يترجم إلى ملف آلة أصلي (الخيار الافتراضي)\n  lisan --interpret ملف.lsn  المسار القديم للتوافق فقط\n  lisan --check ملف.lsn  فحص قابلية توليد LLVM الأصلية دون Clang\n  lisan --check-legacy ملف.lsn  فحص القواعد والاستيرادات القديم\n  lisan --test ملف.lsn   تجميع وتشغيل برنامج أصلي مؤقت عبر Clang\n  lisan --test-legacy ملف.lsn   تشغيل التحققات بالمفسّر القديم\n  lisan --native ملف.lsn --output البرنامج  ترجمة إلى ملف آلة أصلي عبر Clang\n  lisan --emit-object ملف.lsn --output الملف.o  مكتبة أصلية بلا main\n  lisan --native ملف.lsn --link مكتبة.o  ربط ملف أصلي أو غلاف C\n  lisan --emit-llvm ملف.lsn --output الملف.ll  إخراج LLVM IR\n  lisan --build ملف.lsn [--output الناتج]  الطريقة القديمة (تضم المفسّر)\n  lisan --tokens ملف.lsn  عرض الرموز\n  lisan --ast ملف.lsn  عرض شجرة بناء التعليمات\n  lisan -e 'اطبع(\"مرحبا\")؛'\n  lisan                 عرض تعليمات المترجم", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.len() == 1 && (args[0] == "-V" || args[0] == "--version") {
        println!("لسان {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.len() >= 2
        && (args[0] == "--native" || args[0] == "--emit-llvm" || args[0] == "--emit-object")
    {
        let input = std::path::Path::new(&args[1]);
        let emit_ir = args[0] == "--emit-llvm";
        let mut options = lisan::native::NativeBuildOptions::default();
        if args[0] == "--emit-object" {
            options.emit = lisan::native::NativeEmit::Object;
        }
        let default_name = if emit_ir {
            "program.ll"
        } else if args[0] == "--emit-object" {
            if cfg!(windows) {
                "program.obj"
            } else {
                "program.o"
            }
        } else if cfg!(windows) {
            "program.exe"
        } else {
            "program"
        };
        let mut output = None;
        let mut index = 2;
        while index < args.len() {
            if index + 1 == args.len() {
                native_usage();
            }
            let value = &args[index + 1];
            if args[index] == "--output" {
                if output.is_some() {
                    native_usage();
                }
                output = Some(std::path::PathBuf::from(value));
            } else if args[index] == "--link"
                && !emit_ir
                && options.emit == lisan::native::NativeEmit::Executable
            {
                options.link_inputs.push(value.into());
            } else if args[index] == "--library-path"
                && !emit_ir
                && options.emit == lisan::native::NativeEmit::Executable
            {
                options.library_paths.push(value.into());
            } else if args[index] == "--library"
                && !emit_ir
                && options.emit == lisan::native::NativeEmit::Executable
            {
                let name = value
                    .to_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| native_usage());
                options.libraries.push(name.to_string());
            } else {
                native_usage();
            }
            index += 2;
        }
        let output = output.unwrap_or_else(|| default_name.into());
        let result = if emit_ir {
            lisan::native::emit_ir_file(input, &output)
        } else {
            lisan::native::build_native_with_options(input, &output, &options)
        };
        match result {
            Ok(path) => println!("نجحت الترجمة الأصلية: {}", path.display()),
            Err(error) => {
                eprintln!("خطأ في الترجمة الأصلية: {}", error);
                std::process::exit(1);
            }
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
        let target = if args.len() == 4 {
            Some(std::path::Path::new(&args[3]))
        } else {
            None
        };
        match lisan::bundle::build_executable(std::path::Path::new(&args[1]), target) {
            Ok(path) => println!("أُنشئ البرنامج المستقل: {}", path.display()),
            Err(err) => {
                eprintln!("خطأ في البناء: {}", err);
                std::process::exit(1);
            }
        }
        return;
    }
    if args.len() == 2 && (args[0] == "--tokens" || args[0] == "--ast") {
        let source = match std::fs::read_to_string(&args[1]) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("تعذّرت قراءة الملف: {}", err);
                std::process::exit(1);
            }
        };
        let tokens = match lisan::lexer::tokenize(&source) {
            Ok(tokens) => tokens,
            Err(err) => {
                eprintln!("خطأ: {}", err);
                std::process::exit(1);
            }
        };
        if args[0] == "--tokens" {
            for token in tokens {
                println!("{}:{} {:?}", token.line, token.column, token.kind);
            }
        } else {
            match lisan::parser::parse(tokens) {
                Ok(ast) => println!("{:#?}", ast),
                Err(err) => {
                    eprintln!("خطأ: {}", err);
                    std::process::exit(1);
                }
            }
        }
        return;
    }
    if args.len() == 2 && args[0] == "--test" {
        match test_native_file(std::path::Path::new(&args[1])) {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => {
                eprintln!("فشل الاختبار الأصلي: {}", error);
                std::process::exit(1);
            }
        }
    }
    if args.len() == 2 && args[0] == "--test-legacy" {
        match Interpreter::default().run_file(&args[1]) {
            Ok(_) => println!("نجحت اختبارات لسان"),
            Err(err) => {
                eprintln!("فشلت اختبارات لسان: {}", err);
                std::process::exit(1);
            }
        }
        return;
    }
    if args.len() == 2 && (args[0] == "--check" || args[0] == "--check-legacy") {
        let result = if args[0] == "--check" {
            check_native_file(&args[1])
        } else {
            check_file(&args[1])
        };
        match result {
            Ok(()) => println!(
                "نجح فحص ملفات لسان ({})",
                if args[0] == "--check" {
                    "native LLVM"
                } else {
                    "legacy"
                }
            ),
            Err(err) => {
                eprintln!("خطأ: {}", err);
                std::process::exit(1);
            }
        }
        return;
    }
    if args.len() == 2 && args[0] == "-e" {
        let source = match args[1].to_str() {
            Some(source) => source,
            None => {
                eprintln!("يجب أن يكون نص -e صالحًا بصيغة UTF-8");
                std::process::exit(2);
            }
        };
        execute(&mut Interpreter::default(), source);
        return;
    }
    if args.len() == 2 && args[0] == "--interpret" {
        match Interpreter::default().run_file(&args[1]) {
            Ok(lines) => {
                for output in lines {
                    println!("{}", output);
                }
            }
            Err(err) => {
                eprintln!("خطأ: {}", err);
                std::process::exit(1);
            }
        }
        return;
    }
    if args.len() == 1
        && std::path::Path::new(&args[0]).extension() == Some(std::ffi::OsStr::new("lsn"))
    {
        let source = std::path::Path::new(&args[0]);
        let mut name = source.file_stem().unwrap().to_os_string();
        if cfg!(windows) {
            name.push(".exe");
        }
        match lisan::native::build_native(source, std::path::Path::new(&name)) {
            Ok(path) => println!("أُنشئ الملف التنفيذي الأصلي: {}", path.display()),
            Err(error) => {
                eprintln!("خطأ في الترجمة الأصلية: {}", error);
                std::process::exit(1);
            }
        }
        return;
    }
    if !args.is_empty() {
        eprintln!("وسائط غير صحيحة. اكتب lisan --help");
        std::process::exit(2);
    }
    println!("لسان: مترجم عربي أصلي. الاستخدام: lisan ملف.lsn أو lisan --native ملف.lsn --output البرنامج");
}

fn native_usage() -> ! {
    eprintln!("الاستخدام: lisan --native ملف.lsn [--output الناتج] [--link ملف.o] [--library اسم] [--library-path مجلد]؛ أو --emit-llvm/--emit-object ملف.lsn [--output الناتج]");
    std::process::exit(2)
}

fn execute(interpreter: &mut Interpreter, source: &str) {
    match interpreter.run_source(source) {
        Ok(lines) => {
            for output in lines {
                println!("{}", output);
            }
        }
        Err(err) => {
            eprintln!("خطأ: {}", err);
            std::process::exit(1);
        }
    }
}

/// An exclusively created directory. Unix permissions keep generated executables private.
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn create() -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("تعذر تحديد وقت إنشاء مجلد مؤقت: {}", error))?
            .as_nanos();
        for _ in 0..128 {
            let path = env::temp_dir().join(format!(
                "lisan-native-test-{}-{}-{}",
                std::process::id(),
                timestamp,
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("تعذر إنشاء مجلد اختبار مؤقت: {}", error)),
            }
        }
        Err("تعذر إنشاء مجلد اختبار مؤقت فريد".into())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_native_file(source: &std::path::Path) -> Result<std::process::ExitStatus, String> {
    let directory = TestDirectory::create()?;
    // Resolve the private directory so even a relative TMPDIR never invokes PATH lookup.
    let base = directory
        .0
        .canonicalize()
        .map_err(|error| format!("تعذر تحديد مسار مجلد الاختبار: {}", error))?;
    let executable = base.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    lisan::native::build_native(source, &executable)?;
    std::process::Command::new(&executable)
        .status()
        .map_err(|error| format!("تعذر تشغيل الاختبار الأصلي: {}", error))
}
