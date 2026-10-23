//! Host build driver. Artifacts are linked privately and published only on success.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEmit {
    Executable,
    Object,
}

#[derive(Debug, Clone)]
pub struct NativeBuildOptions {
    pub emit: NativeEmit,
    pub link_inputs: Vec<PathBuf>,
    pub library_paths: Vec<PathBuf>,
    pub libraries: Vec<String>,
}
impl Default for NativeBuildOptions {
    fn default() -> Self {
        Self {
            emit: NativeEmit::Executable,
            link_inputs: Vec::new(),
            library_paths: Vec::new(),
            libraries: Vec::new(),
        }
    }
}

struct BuildDirectory(PathBuf);
impl BuildDirectory {
    fn create(parent: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        for _ in 0..128 {
            let path = parent.join(format!(
                ".lisan-build-{}-{}-{}",
                std::process::id(),
                timestamp,
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("تعذر إنشاء مجلد بناء خاص: {}", e)),
            }
        }
        Err("تعذر إنشاء مجلد بناء فريد".into())
    }
}
impl Drop for BuildDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compile generated LLVM for the current 64-bit host. No interpreter is linked.
/// `LISAN_CLANG` selects one executable path, never shell command text.
pub fn compile_ir(ir: &str, source: &Path, output: &Path) -> Result<PathBuf, String> {
    compile_ir_with_options(ir, source, output, &NativeBuildOptions::default())
}

/// Compile LLVM to an executable or a host object, publishing only after success.
pub fn compile_ir_with_options(
    ir: &str,
    source: &Path,
    output: &Path,
    options: &NativeBuildOptions,
) -> Result<PathBuf, String> {
    if !cfg!(target_pointer_width = "64") {
        return Err("البناء الأصلي الحالي يتطلب مضيفًا 64 بت؛ هدف 32 بت غير مدعوم بعد".into());
    }
    if options.emit == NativeEmit::Object
        && (!options.link_inputs.is_empty()
            || !options.library_paths.is_empty()
            || !options.libraries.is_empty())
    {
        return Err("وضع الملف الكائني لا يقبل خيارات الربط".into());
    }
    if options
        .libraries
        .iter()
        .any(|name| name.is_empty() || name.starts_with('-') || name.contains(['/', '\\', '\0']))
    {
        return Err("اسم مكتبة الربط غير صالح".into());
    }
    let link_inputs = options
        .link_inputs
        .iter()
        .map(|input| {
            if !input.is_file() {
                return Err(format!("ملف الربط غير موجود: {}", input.display()));
            }
            input
                .canonicalize()
                .map_err(|e| format!("تعذر تحديد ملف الربط: {}", e))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for path in &options.library_paths {
        if !path.is_dir() {
            return Err(format!("مجلد مكتبات الربط غير موجود: {}", path.display()));
        }
    }
    let destination = prepare_destination(source, output)?;
    for input in &link_inputs {
        if &destination == input || destination.canonicalize().ok().as_ref() == Some(input) {
            return Err("لا يجوز أن يكون ملف الناتج أحد ملفات الربط".into());
        }
    }
    let parent = destination.parent().ok_or("مسار الناتج غير صالح")?;
    let compiler = std::env::var_os("LISAN_CLANG").unwrap_or_else(|| "clang".into());
    let version = Command::new(&compiler)
        .arg("--version")
        .output()
        .map_err(|e| format!("تعذر تشغيل Clang؛ ثبّت clang أو حدد LISAN_CLANG: {}", e))?;
    if !version.status.success() {
        return Err("فشل فحص أداة Clang".into());
    }
    let version_text = String::from_utf8_lossy(&version.stdout);
    let mut words = version_text.split_whitespace();
    let major = words
        .find(|word| *word == "version")
        .and_then(|_| words.next())
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse::<u32>().ok());
    if !matches!(major, Some(15..)) {
        return Err("يتطلب LLVM الأصلي Clang 15 أو أحدث يدعم opaque pointers".into());
    }
    let directory = BuildDirectory::create(parent)?;
    let input = directory.0.join("module.ll");
    let linked = directory.0.join(match options.emit {
        NativeEmit::Executable => format!("program{}", std::env::consts::EXE_SUFFIX),
        NativeEmit::Object => if cfg!(windows) {
            "module.obj"
        } else {
            "module.o"
        }
        .into(),
    });
    fs::write(&input, ir).map_err(|e| format!("تعذر كتابة LLVM IR: {}", e))?;
    let runtime = ir
        .lines()
        .any(|line| line == "; LISAN_MANAGED_TEXT_RUNTIME_V1")
        && options.emit == NativeEmit::Executable;
    if runtime {
        fs::write(
            directory.0.join("value.h"),
            include_str!("../../runtime/common/value.h"),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            directory.0.join("value.c"),
            include_str!("../../runtime/common/value.c"),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            directory.0.join("allocator.c"),
            include_str!("../../runtime/host/allocator.c"),
        )
        .map_err(|e| e.to_string())?;
    }
    let mut command = Command::new(&compiler);
    command
        .arg("-x")
        .arg("ir")
        .arg(&input)
        .arg("-x")
        .arg("none")
        .arg("-O2")
        .arg("-o")
        .arg(&linked);
    if options.emit == NativeEmit::Object {
        command.arg("-c");
    } else {
        if runtime {
            command
                .arg(directory.0.join("value.c"))
                .arg(directory.0.join("allocator.c"));
        }
        command.args(&link_inputs);
        for path in &options.library_paths {
            command.arg("-L").arg(path);
        }
        for library in &options.libraries {
            command.arg("-l").arg(library);
        }
        #[cfg(all(windows, target_env = "msvc"))]
        // Let the MSVC linker resolve this through LIB, bypassing Clang's input-file check.
        command.arg("-Xlinker").arg("legacy_stdio_definitions.lib");
        #[cfg(not(all(windows, target_env = "msvc")))]
        command.arg("-lm");
    }
    let result = command
        .output()
        .map_err(|e| format!("تعذر تشغيل Clang: {}", e))?;
    if !result.status.success() {
        return Err(format!(
            "فشلت الترجمة الأصلية: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    if !linked.is_file() {
        return Err("لم ينتج Clang ملف البناء".into());
    }
    publish(&linked, &destination)
        .map_err(|e| format!("تعذر نشر ملف البناء؛ لم يُستبدل الناتج السابق: {}", e))?;
    Ok(destination)
}

#[cfg(not(windows))]
fn publish(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn publish(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Both paths are same-volume, NUL-terminated and live for the duration of the call.
    let success = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 1 | 8) };
    if success == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn prepare_destination(source: &Path, output: &Path) -> Result<PathBuf, String> {
    let requested = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(output)
    };
    let parent = requested.parent().ok_or("مسار الناتج غير صالح")?;
    fs::create_dir_all(parent).map_err(|e| format!("تعذر إعداد مجلد الناتج: {}", e))?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    let destination = parent.join(requested.file_name().ok_or("اسم الناتج غير صالح")?);
    let source = source
        .canonicalize()
        .map_err(|e| format!("تعذر تحديد مسار المصدر: {}", e))?;
    if destination == source || destination.canonicalize().ok().as_ref() == Some(&source) {
        return Err("لا يجوز أن يكون ملف الناتج هو ملف المصدر".into());
    }
    if destination.is_dir() {
        return Err("مسار الناتج يشير إلى مجلد".into());
    }
    Ok(destination)
}

/// Save LLVM without invoking a compiler, with the same transactional file protection.
pub fn save_ir(ir: &str, source: &Path, output: &Path) -> Result<PathBuf, String> {
    let destination = prepare_destination(source, output)?;
    let directory = BuildDirectory::create(destination.parent().ok_or("مسار الناتج غير صالح")?)?;
    let staged = directory.0.join("module.ll");
    fs::write(&staged, ir).map_err(|e| format!("تعذر حفظ LLVM IR: {}", e))?;
    publish(&staged, &destination).map_err(|e| format!("تعذر نشر LLVM IR: {}", e))?;
    Ok(destination)
}
