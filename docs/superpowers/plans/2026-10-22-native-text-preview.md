# Owned native text preview

Implement managed-values Tasks 1–2 on x86_64 Linux/Windows. Native C runtime and LLVM code only; no evaluator. Existing default unannotated route remains legacy. This is preview.2, not1.0.

## Global constraints
- Text descriptor is {size_t byte_len,ptr data}; valid UTF8, embedded NUL valid; Unicode scalar indexing/length, no normalization.
- Checked allocation/refcounts; transactional failure; no allocations during release. Cleanup must run during execution, not rely on process exit.
- Managed text supported only straight-line main and lexical blocks. Reject text operations inside functions/branches/loops/short-circuit, including text-derived scalar subexpressions; existing allocation-free literal print may remain supported.
- Reject managed CABI, pointer-to-text and raw access. No numeric/text coercion. No managed collection claims.
- Preserve source protection, existing scalar backend, legacy bundle, object/linking semantics.

## Task1 Runtime
Use managed-values plan Task1/2 exact text helpers via descriptor pointers, context size/init/destroy. GCC/Clang native harness, custom allocator failpoints/liveblocks, UTF8/NUL/refcount/sizeoverflow,100000 operation stress, sanitizers where executable.

## Task2 Compiler
Parser نص; IR TextLength/TextIndex; checker text +,==,!=,طول returns ط64,index accepts checked integers or finite integral nonnegative f32/f64 below2^64. Structural validation shared checker+direct TypedProgram backend.
LLVM main-only text descriptors initialized empty. Variables borrow/clone; assignment clone-before-release staging, temp release eachstatement, locals lexical cleanup. Program-wide cleanup callable from ALL scalar failing functions while main owns text; module-private descriptor globals and active-context pointer are acceptable for this restricted hosted preview. Never read failed output. Every failure releases allslots. SourceMap-aware runtime errors filename:line:column. Runtime embedded include_str C/header materialized into private staging and linked with selected Clang only when runtime IR marker present; object mode never attempts multi-input -c/-o. No checkout-dependent runtime file lookup.
Tests-first real compiled Unicode/NUL exact bytes (Windows CRLF accounted),aliases,selfassign,nested concat/index,scopes,badindexes,located failure,unsupported contexts+FFI+direct IR. Instrument emittedIR using replacement hostallocator atexit liveblocks counter to verify successful exit,textfailure,scalarcallee failure and eachallocation OOM clean up. Native emitLLVM manual linker dependency documented.

## Task3 Review and delivery
Independent runtime/compiler spec+quality reviews, fix important findings. Full cargo tests/release/sanitizer and Windows/Linux CI. Docs truthful limitations. Upload to authorized existing branch; official account currently read-only on original repo, no available official repo.
