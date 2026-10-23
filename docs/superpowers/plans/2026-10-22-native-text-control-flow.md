# Native text control-flow preview

Goal: extend preview.2 managed text to main-level if/while and boolean short-circuit, including loop break/continue cleanup. Keep source function managed text, managed C ABI/pointer and collections unsupported. No interpreter; hosted x86_64 Windows/Linux.

## Global constraints
- Preserve immutable UTF8 text, embeddedNUL, scalar indexing, checked allocation, alias and left-to-right snapshots.
- Text locals live for lexical scope; expression temps no later than statement/condition end. Conditions clean all executed temps before selecting a branch or entering the body. Reused global descriptor slots must be empty before reconstruction on every iteration/path.
- All scalar/text failure guards retain universal cleanup, including recursive scalar callees. Main-only context/global descriptors remain intentional; managed function calls/results and pointers/FFI remain rejected.
- Short-circuit evaluates only selected RHS and does not allocate/trap from skipped RHS; merge cleanup safely sees zero descriptors for unexecuted paths.
- break/continue release current statement temps and lexical text locals down through the current loop body, preserving enclosing outer-loop/main locals. Generate cleanup before branch terminators. Never mutate compile-time enclosing frame lists while emitting path-specific cleanup. Normal fallthrough and nested lexical blocks release once.
- Shared checker/directIR validation and runtime-use discovery traverse If/While/short-circuit bodies/conditions. Pure scalar and legacy behavior stays compatible. No source functions accepting/returning or operating on managed text yet (existing allocation-free literal print stays).

## Task1 Implementation
Own src/native/managed.rs,typed_codegen.rs and tests/native_text_values.rs (new testfile if useful). Tests-first formerly prohibited main controlflow now executable; retain negative function/CABI/directmalformedIR tests. Use perlexical-scope text slot frames, loop record with scope-depth mark, condition/statement temp boundaries. Direct IR loop/shortcircuit acceptance updated; invalid types still rejected. Keep runtime API/allocator unchanged.

Test actual native if bothbranches+skipbranch, loops reassign+bodylocals, nestedloops, break/continue through nestedblocks and ifarms, outertext preservation, whilecondition temps incl terminating false, boolean RHS skipped OOM/bounds and executed failure. Instrumented allocator checks live counts *during* repeated iterations and zero at success/text/numericfailure exits; 100000 bounded-workload generated loop, every-allocation OOM for finite loop workload, nestedmutation snapshots still correct. Exact Unicode/NUL and WindowsCRT newline paths. Source located errors for failure inside loop/condition. Full scalar/native/Rust tests. No tests relying only on process exit for cleanup.

## Task2 Review and release
Independent spec/quality and final scoped correction review; root release docs/version preview.3 and packaged example/CI. Run full Linux+Windows matrix and sanitizer harness, upload same original repository branch feature/native-0.9, update PR2 and download links. Do not infer full1.0/Arduino/Web support.
