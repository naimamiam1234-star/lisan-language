# Standard library compatibility inventory

Snapshot: 2026-10-23. This is a source audit, not a claim that every test was run on every target. `docs/SYNTAX.md` describes the dynamic language; its standard-library list is **not** a promise of parity in either native backend. Plans under `docs/superpowers/` are proposals, not implementations.

## Execution paths

| Path | Implementation | Actual scope |
| --- | --- | --- |
| Interpreter (including interpreter bundles) | `src/runtime.rs`, `src/bundle.rs` | Dynamic numbers (`f64`), booleans, text, null, heterogeneous lists and maps; builtins below. A bundled interpreter is not direct native compilation. |
| Legacy native | `src/native.rs::generate_llvm` | LLVM for numbers (`double`), booleans, C-string text and numeric lists; user functions take and return numbers. |
| Typed native | `src/checker.rs`, `src/native/typed_codegen.rs`, `src/native/managed.rs` | Fixed-width numbers, booleans, void, restricted pointers, scalar C ABI, and hosted managed text plus homogeneous scalar/text lists. It does not inherit legacy native builtins. |

`src/native.rs::emit_loaded` selects typed native if `syntax::requires_typed` detects typed constructs, or for library output; otherwise it converts to the legacy AST. Adding a type annotation can therefore change the available builtin set. This is a compatibility boundary, not an automatic migration to a superset.

## Complete language builtin matrix

Notation: `N` = finite dynamic number, `T` = text, `B` = boolean, `V` = any dynamic value, `L<V>` = list, `M` = text-keyed map. `—` means no builtin lowering in that path. Aliases shown together have the same implementation. Every interpreter entry is implemented in `src/runtime.rs::call_builtin` and recognized by `is_builtin`. Legacy entries are in `src/native.rs::generate_expr`, except the explicitly identified range loop.

| Builtin and interpreter signature | Interpreter behavior | Legacy native | Typed native |
| --- | --- | --- | --- |
| `طول(T / L<V> / M) -> N` | Unicode scalar count / element count / key count | Text or numeric list, returns `double` | `طول(نص / قائمة<T>) -> ط64` (unsigned 64-bit), managed preview |
| `نوع(V) -> T` | Returns `رقم`, `نص`, `منطقي`, `لاشيء`, `قائمة`, or `قاموس` | — | — |
| `نص(V) -> T` | Uses value display formatting | Number, boolean, or text only | — |
| `عدد(N / T) -> N` | Parses trimmed Latin numeric text; rejects nonfinite values and Arabic/Persian digits | — | — (numeric casts are separate syntax) |
| `أضف / اضف(L<V>, V) -> L<V>` | Returns copy with appended value | — | `أضف(named_list,value) -> فراغ`: updates named homogeneous list; differs from legacy copy-return behavior |
| `مدى(N) / مدى(N,N) / مدى(N,N,N) -> L<N>` | End exclusive; default start 0, step 1; positive/negative nonzero step | Only directly in `لكل (x في مدى(...))`; **not** a general list-producing expression | — |
| `يحتوي(L<V>,V) / يحتوي(T,T) / يحتوي(M,T) -> B` | Element / substring / key membership | — | — |
| `مفاتيح(M) -> L<T>` | Keys in stored order | — | — |
| `قيم(M) -> L<V>` | Values in stored order | — | — |
| `احذف(M,T) -> M` | Returns copy without key; absent key leaves contents unchanged | — | — |
| `احذف(L<V>,N) -> L<V>` | Returns copy without indexed element; bounds checked | — | — |
| `قسّم / قسم(T,T) -> L<T>` | Rust text split semantics, including empty separator | — | — |
| `ادمج(L<T>,T) -> T` | Joins texts with separator; rejects nontext elements | — | — |
| `استبدل(T,T,T) -> T` | Replaces all matches; empty search text rejected | — | — |
| `اقتطع(T,N,N) -> T` | Unicode scalar slice `[start,end)`; checks order and bounds | — | — |
| `نظّف / نظف(T) -> T` | Unicode whitespace trim | — | — |
| `مطلق(N) -> N` | Absolute value | `double -> double` via LLVM fabs | — |
| `جذر(N) -> N` | Square root; negative input rejected | `double -> double` via LLVM sqrt, negative guard | — |
| `قرّب / قرب(N) -> N` | Nearest integer, ties away from zero | `double -> double` via LLVM round | — |
| `أدنى / ادنى(N,N) -> N` | Minimum | `double,double -> double` | — |
| `أعلى / اعلى(N,N) -> N` | Maximum | `double,double -> double` | — |
| `اقرأ_ملف(T) -> T` | Reads UTF-8 host file; metadata size at most 2,000,000 bytes | — | — |
| `اكتب_ملف(T,T) -> B` | Writes/replaces host file; content at most 2,000,000 bytes; returns true | — | — |
| `تحقق(B) -> B` | Returns true or runtime error | Boolean only; failing guard exits | — |
| `تحقق_يساوي(V,V) -> B` | Dynamic structural equality or runtime error | Same-type number, boolean, or text only; numeric lists rejected | — |

Interpreter collections/ranges are capped at 10,000 items; range inputs must be exact integers within ±9,007,199,254,740,991. Indices must be nonnegative finite integers. Interpreter execution also limits steps to 250,000 and call depth to 128. These are interpreter policies, not a common native resource contract.

`اطبع(value)` is a statement, not a callable builtin. The interpreter displays all its value kinds. Legacy native prints numbers, booleans, and text; printing a numeric list directly is rejected (iterate to print its elements). Typed native prints supported scalar values and preview text; this does not provide formatting, console input, or file APIs.

## Text, collections, and modules outside the builtin table

- Interpreter operators provide text concatenation, list concatenation, text/list/map indexing, and assignment to a named list/map index (`src/runtime.rs::eval`, `binary`). Append/delete return copies; indexed assignment updates the variable.
- Legacy native supports text concatenation (including number/boolean conversion), text equality, Unicode-aware length (text indexing is rejected), numeric-list literals/indexing/index assignment and iteration. Its text representation uses NUL-terminated C strings (`strlen`, `strcmp`, `strcpy`, `strcat`) and heap allocations without the managed-text ownership contract. Do not assume embedded-NUL parity or managed cleanup.
- Typed managed text currently supports literals, local bindings/copies/reassignment, text-plus-text, equality/inequality, scalar indexing, length, and print in top-level scopes and their supported control flow. Unicode scalar indexing is not grapheme indexing. It preserves embedded NUL. Numeric-plus-text is rejected. Internal functions support text parameters, bodies and results, including recursion and early returns. Call arguments are snapshotted left-to-right; each invocation owns separate descriptors. Hosted fatal errors clean every active frame. Foreign managed signatures, pointer-to-text, managed library output and C exports in managed programs remain rejected. Recoverable status propagation, reentry and concurrency are not implemented.
- Typed native supports `قائمة<T>` for fixed-width numbers, booleans and text, with contextual empty literals, length, bounds-checked indexing/set, append and structural equality. Lists cross internal function boundaries with per-call cleanup and value semantics. Mutation always constructs fresh storage (O(n)); aliases keep their original values. Nested lists, maps, collection foreach/delete and whole-list printing remain unsupported. Its scalar casts, address/load/store/null-pointer operations are language syntax, not dynamic conversion or collection APIs. See `docs/NATIVE-TYPES.md` and `docs/MEMORY-POINTERS.md` for exact spelling and restrictions.
- `استورد "relative.lsn"` loads source files through `src/modules.rs`: relative nonempty paths, canonical deduplication, cycle checks, top-level imports, depth limit 32 and per-file limit 2,000,000 bytes. Imported functions share one namespace. This is source composition, not an installed package registry, dependency resolver, version lockfile, or namespaced standard-library distribution.

## Internal managed runtime is not a public language library

`runtime/common/value.h` and `value.c` expose C implementation machinery, used by typed code generation. `runtime/host/allocator.c` supplies the host allocator. Presence here does not make a function callable by its C name from ordinary Lisan.

| Internal API | C signature / role |
| --- | --- |
| `lisan_rt_host_allocator` | `void -> LisanAllocator` |
| `lisan_rt_context_init`, `lisan_rt_context_destroy` | `LisanContext* -> void` |
| `lisan_rt_context_init_with_allocator` | `(LisanContext*, LisanAllocator) -> void` |
| `lisan_rt_context_size` | `void -> size_t` |
| `lisan_rt_size_add`, `lisan_rt_size_mul`, `lisan_rt_size_align_up` | `(size_t,size_t,size_t*) -> bool`, checked arithmetic |
| `lisan_rt_text_header` | `const LisanText* -> LisanValueHeader*` |
| `lisan_rt_text_from_utf8` | `(LisanContext*,const unsigned char*,size_t,LisanText*) -> LisanStatus`, copies validated bytes |
| `lisan_rt_text_clone` | `(LisanContext*,const LisanText*,LisanText*) -> LisanStatus` |
| `lisan_rt_text_concat` | `(LisanContext*,const LisanText*,const LisanText*,LisanText*) -> LisanStatus` |
| `lisan_rt_text_at` | `(LisanContext*,const LisanText*,size_t,LisanText*) -> LisanStatus` |
| `lisan_rt_text_release` | `(LisanContext*,LisanText*) -> void` |
| `lisan_rt_text_scalar_len` | `const LisanText* -> size_t` |
| `lisan_rt_text_equal` | `(const LisanText*,const LisanText*) -> bool` |
| `lisan_rt_text_print` | `const LisanText* -> void` |

Owned output slots must be distinct and empty, inputs belong to the context allocator, and callers release values before destroying the context. The runtime now implements registered element descriptors and checked scalar/text list allocation, clone, release, index, set, append and equality. This does not imply nested collection or map support. The host executable build automatically links this runtime when its managed-text IR marker is present; an object is not a self-contained runtime distribution.

## External library integration and target blockers

Typed native implements declared scalar/pointer C ABI calls within `غير_آمن` and exported functions. This is a real host integration path, not an automatic binding for arbitrary libraries. `src/checker.rs`, `src/types.rs`, `src/native/typed_codegen.rs`, and `src/native/build.rs` are the controlling implementations. No direct C++ class/template ABI, varargs, callbacks, struct-by-value, or implicit text-to-C-string conversion is provided. C++ integrations require an `extern "C"` scalar/pointer wrapper and their own ownership contract.

The build driver accepts object output and explicit link inputs/library paths/library names. Object mode rejects link options. Library generation uses the typed path, rejects executable top-level statements, and does not provide a dynamic shared-library packaging workflow. The build currently requires a 64-bit host and Clang 15+ with opaque pointer support; `LISAN_CLANG` selects an executable, not target flags. It uses host libc and math linkage. Host object success does not establish cross-target ABI correctness.

| Requested ecosystem | Current status | Concrete missing integration |
| --- | --- | --- |
| Arduino / STM32F103C6T6A | No supported board backend or verified library bindings | Explicit ARM target/layout/toolchain; C6 memory map/linker script/startup; matching core; setup/loop wrapper; allocator/I/O policy and hardware tests |
| ESP32 DEVKITC V4 / ESP8266 | No supported target or verified core integration | Exact board/core/toolchain, supported instruction backend (including Xtensa where applicable), startup/link and flash pipeline |
| GPIO, UART, I2C, SPI, SD, WiFi, displays, sensors, GPS, Firebase | No built-in libraries or blanket compatibility | Per-library C wrappers, version/core/board compatibility, buffer ownership, callbacks/async strategy where needed, build and device evidence |
| Browser WebAssembly / DOM | No wasm target or JavaScript bridge | wasm32 layout/toolchain/runtime, imports/exports and memory/text ABI, JS DOM/event/network bindings, browser integration tests |
| WASI | No supported WASI build pipeline | Explicit target/sysroot/import contract and tests; WASI success alone would not establish browser DOM support |

## Next milestones and evidence

1. Extend the hosted internal managed-text/list function increment to the planned recoverable status/context ABI. Exercise argument/return ownership, recursion, nested scopes, early exits, allocation failures, and diagnostics. Keep managed text outside C ABI until an explicit foreign ownership ABI exists.
2. Port standalone text builtins (`نظّف`, `اقتطع`, `استبدل`, `يحتوي`, conversions) with stated Unicode and failure behavior. Split/join require typed text lists first; keys/values/delete require typed maps. Do not substitute legacy C-string behavior for managed runtime semantics.
3. Extend the verified hosted scalar/text lists to nested collections and ordered maps, then add their builtins and independent parity tests. Define whether dynamic `نوع`/`نص` have typed equivalents rather than promising them by name alone.
4. Define a capability-aware I/O layer before porting file calls. Host filesystem operations cannot be reused unchanged on a microcontroller or browser.
5. Implement target descriptions and freestanding/runtime separation before publishing Arduino or wasm claims. Record board, core, compiler, library version and evidence separately as build-only, simulation, browser, or actual hardware.

Existing evidence locations: `tests/native.rs`, `native_extended.rs`, `native_numeric_safety.rs`, `native_typed_codegen.rs`, `native_types.rs`, `native_text_values.rs`, `native_pointers_frontend.rs`, `native_linking.rs`, `native_build.rs`, `runtime_managed.rs`, `modules.rs`, and `tests/fixtures/managed/runtime_harness.c`. They cover different paths; a legacy test does not prove typed support. This inventory's source inspection does not itself assert a fresh passing test run.
