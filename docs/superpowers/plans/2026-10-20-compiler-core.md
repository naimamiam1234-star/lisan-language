# Compiler Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Compile explicitly typed native functions and safe-to-describe C ABI calls while preserving existing untyped programs and producing source-located errors.

**Architecture:** Preserve the public legacy parsing and interpreter entry points through adapters while adding a located syntax tree, semantic checking, and a typed IR before LLVM emission. All native declarations are checked before emission; external signatures and pointer operations share the same type rules. Target configuration and build/link orchestration are separate workstreams and must consume these interfaces rather than duplicate them.

**Tech Stack:** Rust 2021, existing dependency-free crate, textual LLVM IR, Clang for executable acceptance tests.

**Spec:** `docs/superpowers/specs/2026-10-20-native-hardware-web-design.md`

## Global Constraints

- “تحتفظ البرامج القديمة بنوع العدد الافتراضي الحالي.” Existing unannotated numeric programs remain f64, including legacy functions and implicit zero returns.
- “لا تدعم المرحلة الأولى varargs أو struct-by-value أو callbacks أو C++ مباشرة.”
- “تمرير الوسائط إلى Command دون shell.”
- “لا يصدر freestanding main أو استدعاءات malloc/printf ضمنيًا”.
- “يرفض البناء هدفًا غير مدعوم بدل الرجوع الصامت إلى جهاز المضيف.”
- Native acceptance must execute a real C-linked binary and inspect its object symbols and machine instructions; textual LLVM inspection alone is insufficient.
- The syntax below is a proposed grammar to lock in Task 1, not existing support. Package resolution, lockfiles, generic collections, C++ ABI, and hardware volatile operations remain separate later deliverables.

## Current Evidence and Gaps

`src/lexer.rs` stores numeric tokens as f64, losing exact u64 values before checking. `src/ast.rs` contains no type annotations or locations; `Function.params` is `Vec<String>`. `src/native.rs` has private `Type::{Number,Boolean,Text,NumberList}`, functions mapped to `(String, usize)`, double-only arguments/results, and a zero default return. It emits libc declarations, a main function, i64 list lengths, stack-backed numeric lists, and heap-allocated concatenation buffers without reclamation. `src/modules.rs` already canonicalizes relative imports, deduplicates them, detects cycles, and enforces 32-level/2,000,000-byte limits, but flattens statements and loses source provenance. `check_file` currently checks parsing/imports, not native semantics. No FFI declaration, raw pointer, target description, or object-output API exists. Existing Rust/Clang tests must be run during execution; this plan makes no claim that they passed here.

## Review Focus

- Decimal integers above 2^53 and signed minima must retain exact source digits (Task 2).
- Missing returns in typed functions, including nested branches, must fail before LLVM generation (Task 3).
- Duplicate C symbols arriving through distinct imported paths must produce deterministic diagnostics (Tasks 4 and 6).
- Borrowed local pointers escaping through returns or stores must be rejected; foreign ownership must stay explicit (Task 5).
- Errors inside imported Arabic UTF-8 source must identify that file and its human-readable line/column (Tasks 1 and 6).

## File Structure and Shared Interfaces

Create `src/source.rs` for `SourceId(u32)`, `Span { source: SourceId, start: usize, end: usize }` (UTF-8 byte offsets), and `SourceMap` holding source text/path and mapping byte positions to one-based Unicode-scalar line/column. Create `src/diagnostic.rs` for `Diagnostic { code: &'static str, message: String, primary: Span, labels: Vec<(Span, String)> }` and `render(&self, sources: &SourceMap) -> String`.

Create `src/syntax.rs` for a located syntax tree; do not destructively change legacy `ast.rs`. Define `Node<T> { kind: T, span: Span }`, aliases `Expr = Node<ExprKind>` and `Stmt = Node<StmtKind>`, and `Program { statements: Vec<Stmt> }`. Its expression/statement kinds cover the existing AST plus annotated bindings/functions, extern declarations, export definitions, casts, raw-pointer address/load/store and unsafe blocks. Preserve original numeric lexemes in `NumericLiteral { text: String }`. `Param { name: String, ty: Option<TypeSyntax>, span: Span }`; `Function { name: String, params: Vec<Param>, result: Option<TypeSyntax>, body: Vec<Stmt> }`; `ExternFunction { name: String, symbol: String, params: Vec<Param>, result: TypeSyntax }`; `StmtKind::ExportFunction { function: Function, symbol: String }` wraps an explicitly exported definition.

Create `src/types.rs` for `Type::{Int { signed: bool, bits: u8 }, Float { bits: u8 }, Bool, Void, Pointer(Box<Type>), Text, NumberList}` and `FunctionSig { params: Vec<Type>, result: Type, abi: Abi }`, `Abi::{Lisan,C}`. Only widths specified here are constructible by parsing. Create `src/typed.rs` for typed IR, `FunctionId(u32)`, `LocalId(u32)`, `TypedExpr { kind: TypedExprKind, ty: Type, span: Span }`, typed statements with equivalent control flow, `TypedFunction { id, name, symbol, signature, params: Vec<LocalId>, body }`, and `TypedProgram { functions, externs, main }`. `TypedExprKind::Call { callee: FunctionId, args: Vec<TypedExpr> }` resolves names once; constant integer payloads use `i128` to represent the entire u64 domain. Add `src/checker.rs::check(program: &syntax::Program) -> Result<typed::TypedProgram, Vec<Diagnostic>>`.

Public compatibility APIs remain `lexer::tokenize`, `parser::parse`, `modules::load_file`, `native::generate_llvm`, `native::emit_file`, `native::build_native`, `run`, and `run_file`. Their old signatures remain valid. New located APIs are `lexer::tokenize_located(source: &str, source_id: SourceId) -> Result<Vec<LocatedToken>, Diagnostic>`, `parser::parse_located(tokens: Vec<LocatedToken>) -> Result<syntax::Program, Diagnostic>`, and `modules::load_program(path: &Path) -> Result<LoadedProgram, Vec<Diagnostic>>`, where `LoadedProgram { program: syntax::Program, sources: SourceMap }`. Legacy conversion must reject new native-only constructs explicitly when using the interpreter, never erase types or execute foreign calls.

### Task 1: Lock Grammar and Preserve Source Locations

**Files:** Create `docs/NATIVE-TYPES.md`, `src/source.rs`, `src/diagnostic.rs`, `src/syntax.rs`, `tests/native_syntax.rs`; modify `src/lexer.rs`, `src/parser.rs`, `src/lib.rs`.

**Interfaces:** Produce the located APIs above and `syntax::to_legacy(&Program) -> Result<Vec<ast::Stmt>, Diagnostic>` for the legacy interpreter. Existing public token and AST APIs stay adapters, avoiding an unrelated interpreter rewrite.

- [ ] Write `typed_signature_round_trips`, `extern_requires_complete_signature`, `export_definition_has_symbol_and_body`, `legacy_ast_adapter_preserves_program`, and `arabic_span_renders_scalar_column` tests. Assert exact parameter names/types, original digit strings, source ranges, and legacy output. Assert missing type/result/symbol errors identify the offending token.
- [ ] Run `cargo test --test native_syntax`; require failures because the new APIs are absent.
- [ ] Document and implement the grammar: `دالة جمع(أ: ص32، ب: ص32): ص32 { أرجع أ + ب؛ }`; `ليكن س: ص32 = 7؛`; `خارجي دالة جمع_سي(أ: ص32، ب: ص32): ص32 = "c_add"؛`; export definition `صدّر دالة إعداد(): فراغ = "lisan_setup" { أرجع؛ }` places `صدّر` before `دالة` and the mandatory linker name after result type and before body; type names `ص8/ص16/ص32/ص64`, `ط8/ط16/ط32/ط64`, `ع32/ع64`, `منطقي`, `فراغ`, `مؤشر<نوع>`, with `عدد` aliasing `ع64`. Untyped functions require all parameter and result annotations absent; mixed signatures fail. `فراغ` only occurs as a result or pointer pointee. Reserve `خارجي` contextually so unrelated old identifiers remain accepted where unambiguous.
- [ ] Define explicit conversion as `تحويل<ص32>(expr)`; unsafe block `غير_آمن { ... }`; raw pointer operations `عنوان(expr)`, `تحميل(ptr)`, `خزن(ptr، value)` and typed null `مؤشر_فارغ<ص32>()`. Parse these as dedicated syntax nodes, not ordinary overridable builtin calls. Address operands must eventually be locals. Type names remain contextual identifiers; nested pointer closing `>>` is two greater-than tokens.
- [ ] Run `cargo test --test native_syntax` and `cargo test --test language --test control_flow`; require all pass. Commit only this independently reviewable grammar/location change.

### Task 2: Exact Scalar Types and Conversion Rules

**Files:** Create `src/types.rs`, `src/typed.rs`, `src/checker.rs`, `tests/native_types.rs`; modify `src/lib.rs`.

**Interfaces:** Produce `check` and `Type`; typed expression variants retain explicit casts and checked arithmetic operations for the emitter. Symbol/function collection is introduced in Task 3.

- [ ] Write `u64_max_is_exact` asserting decimal `18446744073709551615` checks as ط64 with exact constant payload; `signed_min_is_valid` for -128/-32768/-2147483648/-9223372036854775808; `integer_boundaries_reject` for each max+1/min-1; `legacy_number_is_f64`; and `conversion_rules_are_explicit` asserting implicit sign changes/narrowing fail with `E_TYPE_MISMATCH`.
- [ ] Run `cargo test --test native_types`; require the new checks fail before implementation.
- [ ] Implement integer lexeme parsing directly to i128 under expected types, folding unary minus before range validation. Fraction/scientific notation cannot initialize integers implicitly. Untyped numerals stay f64. Same-type arithmetic/comparison only; the sole contextual conversion is an exact literal into the expected scalar type. No implicit variable conversion, including widening, is needed for this stage. Explicit casts permit checked numeric conversions; reject out-of-range constants at compile time and preserve a checked-cast IR node for dynamic values. No integer/pointer casts.
- [ ] Specify typed integer add/subtract/multiply overflow and division by zero or MIN/-1 as traps; unsigned and signed comparison/division select distinct IR operations. Floats follow existing f64 semantics; f32 operations remain f32. Logical operations require Bool. `**` stays f64-only. This is a deliberate new typed-integer contract, not a change to legacy f64 arithmetic.
- [ ] Run `cargo test --test native_types`; require every width and signedness boundary pass. Commit scalar checking and its contract together.

### Task 3: Typed Native Functions and LLVM Emission

**Files:** Modify `src/checker.rs`, `src/typed.rs`, `src/native.rs`; create `src/native/scalar.rs`, `tests/native_typed_functions.rs`.

**Interfaces:** `native::generate_typed_llvm(program: &typed::TypedProgram) -> Result<String, Vec<Diagnostic>>` is the initial hosted emitter; `generate_llvm(&[ast::Stmt])` converts legacy AST into syntax, checks it, and delegates. `native/scalar.rs` owns scalar LLVM type/operator/cast emission. Later target work adds target input without changing semantic rules.

- [ ] Add executable tests `typed_recursive_calls`, `boolean_and_f32_return`, `void_call_is_not_a_value`, `missing_return_branch_rejected`, `legacy_implicit_zero_return_preserved`, `dynamic_integer_overflow_traps`, and `dynamic_checked_cast_traps`. Assert typed recursion returns 120, bool branches behave correctly, f32 return equals a C-observable representable value, and failing trap programs exit unsuccessfully. Use arguments loaded through locals to prevent constant-only checking from satisfying runtime tests.
- [ ] Run `cargo test --test native_typed_functions`; establish missing feature failures.
- [ ] Collect all declarations before checking bodies, allocate stable FunctionIds in source order, reject duplicate names/parameters, then resolve call arity/types and exact result types. Reject nested functions as before. Require all reachable paths in annotated non-void functions to return; a while-loop is conservatively fallthrough unless proven literal true without reachable break. Annotated void permits only bare return; legacy functions retain bare/fallthrough f64 zero. Main cannot return a language value.
- [ ] Move native code generation to checked typed functions with per-function result types; render `ret void` without constructing a value. Emit LLVM signed/unsigned overflow intrinsics and `llvm.trap` blocks for the contract in Task 2; never use poison-producing unchecked `sdiv`/float-to-int for invalid inputs. Preserve entry allocas and existing short-circuit semantics. Reject unsupported typed constructs with source diagnostics before emitting any artifact.
- [ ] Run `cargo test --test native_typed_functions --test native --test native_extended` and the complete `cargo test` suite. Actual executable tests require Clang; CI acceptance must fail if its toolchain preflight fails rather than count existing skip branches as native success. Commit the working typed native path.

### Task 4: C ABI Signatures and Symbol Safety

**Files:** Create `src/ffi.rs`, `tests/native_ffi.rs`, `tests/fixtures/ffi/scalars.c`; modify `src/checker.rs`, `src/typed.rs`, `src/native.rs`, `docs/NATIVE-TYPES.md`.

**Interfaces:** `ffi::validate_symbol(symbol: &str) -> Result<(), &'static str>` accepts ASCII `[A-Za-z_][A-Za-z0-9_]*`; `ffi::validate_signature(sig: &FunctionSig) -> Result<(), &'static str>` allows fixed scalars, pointers, and void results only. `typed::ExternFunction { id: FunctionId, name: String, symbol: String, signature: FunctionSig, span: Span }`. A target workstream must provide `CAbiLowering` before enabling external calls on a target: `lower_signature(&FunctionSig) -> Result<LoweredSignature, Diagnostic>` with per-value LLVM type and ABI attributes.

- [ ] Write `extern_symbol_validation` rejecting empty strings, quotes, newlines, LLVM punctuation, non-ASCII linker names, and reserved `main`, `lisan_fn_`, `lisan_rt_` and `llvm.` implementation namespaces. Explicit wrapper symbols such as `lisan_setup`, `lisan_loop` and `lisan_gpio_write` are permitted. Write `conflicting_foreign_signatures` rejecting any two declarations of the same linker symbol with differing signatures even under different language names; identical signatures may share a symbol. Reject name collision with language functions.
- [ ] Write C-linked acceptance `c_scalar_roundtrip`: C fixture exposes i8/u8/i16/u16/i32/u32/i64/u64/f32/f64/_Bool echo functions, mixed-argument addition, and void setter/getter. Assert negative narrow integers, unsigned maxima, exact u64 max, and both bool values survive both call/result boundaries. `-128` must return `-128`, never `128`; test actual compiled code.
- [ ] Run `cargo test --test native_ffi`; establish failure, then implement signature validation, exact argument matching, void-call statements, and external `declare`/call generation. Bool uses the target's C `_Bool` ABI lowering, not an assumption that every C boolean has the language's bare i1 ABI. Include target-specific signext/zeroext rules; unsupported target lowerings return a diagnostic.
- [ ] Test arity, float/integer mixing, narrowing, sign changes, varargs syntax, text/list arguments, and unsupported ABI declarations fail before invoking Clang. Implement `StmtKind::ExportFunction` with complete annotated signatures and `typed::ExportFunction { function_id: FunctionId, symbol: String, span: Span }`; export symbols share the same validation/conflict table as imports. Validate exported parameters/results through target-owned CAbiLowering; reject managed aggregates, duplicate exports, and import/definition symbol collisions before lowering. Emit the requested unmangled C ABI definition and apply matching argument/result attributes; internal calls use the checked function identity. Add executable C-to-Lisan tests for scalar results, narrow signed/unsigned values, void `lisan_setup`/`lisan_loop` calls, plus negative tests for incomplete/mixed signatures and duplicate symbols. Do not expose ordinal `lisan_fn_N` names as a stable C API.
- [ ] Run `cargo test --test native_ffi` under the target/driver object-and-link integration harness; inspect C-import symbols using the configured nm and disassemble the resulting object with configured objdump. Assert unresolved imports in the object and a real machine-code call in the linked output; record tool versions and target. Commit working C calls only after executable evidence exists.

### Task 5: Pointer and Memory Contract

**Files:** Create `src/memory.rs`, `tests/native_pointers.rs`, `tests/fixtures/ffi/pointers.c`, `docs/MEMORY.md`; modify `src/checker.rs`, `src/typed.rs`, `src/native.rs`, `docs/NATIVE-TYPES.md`.

**Interfaces:** `memory::Provenance::{Local(LocalId),Foreign,Null}` attaches to pointer expressions; `memory::validate_escape(value: &typed::TypedExpr, destination: EscapeDestination) -> Result<(), Diagnostic>`, `EscapeDestination::{Return,ForeignCall,LocalStore(LocalId)}`. These are conservative escape checks, not a full borrow checker. Pointer types carry pointee type through checking even though LLVM uses opaque `ptr`.

- [ ] Write executable `c_pointer_out_parameter` passing `عنوان(س)` for typed ص32 local to C, mutating it, and observing 42; `foreign_null_roundtrip`; `null_load_traps`; `pointer_alignment_contract`; and rejection tests `unsafe_required`, `local_pointer_cannot_return`, `local_pointer_alias_cannot_escape`, `pointer_pointee_mismatch`.
- [ ] Run `cargo test --test native_pointers`; establish failure.
- [ ] Implement unsafe checking: foreign calls and pointer load/store require `غير_آمن`; taking the address of a local creates a local-provenance pointer, which may be passed to a foreign call only within that block. Foreign callees may not retain that pointer under the documented caller contract. Copying pointers must preserve provenance; branch joins combine provenance conservatively, and reject escape if any incoming value is local. No arithmetic, integer casts, implicit text-to-C-string, callbacks, heap allocator, or general lifetime inference in this stage.
- [ ] Specify that foreign pointers are borrowed and never automatically freed, caller guarantees pointee validity/lifetime/alignment and callee ownership rules; foreign null values may be compared or forwarded but load/store null traps. Loads/stores use conservatively low alignment unless a proven stronger local alignment exists; no promise of validating arbitrary foreign addresses. `مؤشر<فراغ>` can cross FFI but cannot be loaded/stored without a future typed conversion contract.
- [ ] Document existing text literals as immutable process-lifetime UTF-8/NUL data, existing text concatenation allocations as hosted process-lifetime allocations pending a dedicated managed-memory phase, and NumberList as function-lifetime storage that must not escape. Typed text/list parameters and results stay rejected until an ownership-aware representation ships; this task does not invent safe collection support. Emit runtime dependencies only when used and surface them to the target/profile integration; freestanding rejects allocation/printing/text helpers before emission.
- [ ] Run `cargo test --test native_pointers --test native_ffi` plus the existing collection regression suite. Commit pointers with the explicit memory contract and negative lifetime tests.

### Task 6: Source-Preserving Modules and Deterministic Errors

**Files:** Modify `src/modules.rs`, `src/source.rs`, `src/diagnostic.rs`, `src/lib.rs`, `src/main.rs`; create `tests/native_diagnostics.rs`, `tests/fixtures/modules/typed_main.lsn`, `tests/fixtures/modules/typed_math.lsn`; extend `tests/modules.rs`.

**Interfaces:** Produce `load_program`/`LoadedProgram` above. `native::check_file(path: &Path) -> Result<(), Vec<Diagnostic>>` loads and semantically checks native source; keep public legacy `check_file` behavior until CLI compatibility is explicitly versioned. CLI native checks use the new function, not parser-only success.

- [ ] Add `imported_type_error_has_path_line_column`, `diamond_import_deduplicates_symbols`, `cycle_reports_import_chain`, `imported_ffi_conflict_has_two_locations`, and `diagnostic_order_is_stable` tests. Assert actual UTF-8 filenames and scalar columns, both declaration locations, and stable order across repeated runs; retain depth/size/relative-path rejection tests.
- [ ] Run `cargo test --test native_diagnostics --test modules`; establish failures for missing native provenance.
- [ ] Implement module loading into one SourceMap with canonical identity and deterministic depth-first statement order. Retain existing 32/2,000,000 limits, deduplication and cycle checking. Track import-site spans in diagnostics; typed imports remain shared flattened scope for compatibility, with global name collisions rejected explicitly. No namespace or package/version semantics are implied.
- [ ] Use codes `E_PARSE`, `E_TYPE_MISMATCH`, `E_RETURN`, `E_DUPLICATE`, `E_FFI_SYMBOL`, `E_FFI_ABI`, `E_UNSAFE`, `E_POINTER_ESCAPE`, `E_IMPORT`, `E_UNSUPPORTED`; render Arabic messages with file:line:column and secondary labels. Type-check independent functions in source order, returning multiple deterministic errors without generating partial IR. CLI exits nonzero and does not invoke linker after a semantic error.
- [ ] Run `cargo test --test native_diagnostics --test modules` and all legacy tests. Commit module provenance and user-facing diagnostics together.

### Task 7: Integrated Acceptance and Honest Documentation

**Files:** Modify `docs/NATIVE-COMPILER.md`, `docs/SYNTAX.md`, `README.md`, `scripts/check-linux.sh`, `scripts/check-windows.ps1`; create `examples/native_typed_ffi.lsn`, `tests/native_core_acceptance.rs`.

**Interfaces:** Consume the target/driver plan's explicit object emission/link input APIs and configured toolchain. Do not add a second ad hoc build driver to this plan. If those APIs are not delivered, keep the integration acceptance blocked and report that fact; unit-level signature support is not a completed native FFI release.

- [ ] Write `typed_c_ffi_end_to_end` that compiles the C fixture to object, emits a Lisan object, links them, executes the result, checks typed scalar/pointer mutations, and checks expected symbol and disassembly evidence. Use separate Command arguments and temporary directories including spaces; assert invalid declarations fail without an output object. Add the same host ABI coverage on Windows when its configured native C toolchain is present, with explicit per-platform evidence.
- [ ] Add `freestanding_scalar_has_no_host_runtime` through the target integration: a scalar-only exported function has no main/malloc/printf/strlen declarations or references; a text-concatenating/printing source returns `E_UNSUPPORTED`. Assert an unsupported target cannot silently build a host executable.
- [ ] Run `cargo test`, then required native acceptance with Clang, nm, and objdump preflight. Missing tools must yield a clearly blocked acceptance job, never a passing skipped native claim. Capture execution status, stdout, target, tool versions, symbol names, and disassembly artifact paths in CI output.
- [ ] Update docs to distinguish interpreter-supported dynamic features, legacy bundled builds, and native typed support. Include exact supported scalar/C ABI list, unsafe/pointer contract, exclusions, and target-specific evidence. Do not claim Arduino, browser WebAssembly, package manager, or general C++ library compatibility from these host tests.
- [ ] Commit verified documentation and acceptance coverage. Publish a coverage table mapping this plan's seven deliveries to executable test names and remaining external dependencies.

## Execution Dependencies and Completion Rule

Tasks 1–3 are executable without FFI or cross-target drivers and preserve the existing hosted native path. Task 4 validation is independently testable but its executable acceptance waits for target-specific ABI lowering and object/link APIs. Task 5 depends on Task 4; Task 6 can proceed after Task 1 and integrates checker diagnostics after Task 3. Task 7 gates release claims across the compiler and target/driver workstreams. Semantic support, object generation, successful host execution, and actual hardware/browser validation must remain separate statuses.
