# Managed Values Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Supply native text, homogeneous lists and text-keyed homogeneous maps with automatic cleanup and typed function parameters/results for the 1.0 core.

**Architecture:** Immutable length-bearing UTF-8 text and reference-counted collection storage use value semantics with copy-on-write mutation. A small compiled native runtime implements checked allocation, reference management and collection operations; it contains no evaluator. Compiler-generated cleanup and an explicit internal failure channel release values across every control-flow exit, including recursive calls and allocation failure.

**Tech Stack:** Existing Rust checker/typed LLVM emitter, portable C runtime compiled to the selected target, Clang, ASan/LSan on Linux x86_64; browser allocator integration after host acceptance.

**Spec:** `docs/LANGUAGE-1.0-SCOPE.md`; `docs/superpowers/specs/2026-10-20-native-hardware-web-design.md`; integration contracts in `2026-10-20-compiler-core.md` and `2026-10-20-web.md` in this directory.

## Global Constraints

- Proposed work only: no managed native text/list/map function support or successful memory checks is asserted by this plan.
- “لا يكفي الاعتماد على انتهاء العملية لتحرير كل نص مؤقت.” Repeated operations must release allocations while the program remains alive.
- Existing default numbers stay f64; all 1.0 scalar types, text and finite nested list/map types are valid managed elements. Raw pointers and void are forbidden as collection elements.
- Native release target is Linux x86_64 hosted. Browser and freestanding implementations require their own allocator/runtime evidence; never fall back to libc on browser or silently enable heap features on an unsupported freestanding profile.
- Foreign ABI never receives or returns managed aggregates by value. No implicit conversion to NUL-terminated C strings; embedded NUL is valid text.

## Review Focus

- Refcounts and container capacity arithmetic must detect overflow before allocation or mutation (Tasks 1–3).
- Aliases, nested values and function results must retain value semantics after mutation (Tasks 3–4).
- Break/continue, early return and failing recursive callees must release all owned temporaries exactly once (Task 4).
- Arabic combining marks, emoji, embedded NUL and invalid UTF-8 must follow the published indexing contract (Task 2).
- Browser memory growth, borrowed inputs and OOM must not invalidate live values or leak per event (Task 5).

## Representation and Shared Interfaces

Create `runtime/common/value.h`, `runtime/common/value.c`, `runtime/host/allocator.c`, `src/native/managed.rs`, `docs/MEMORY.md` (extend if created by core), `docs/MANAGED-VALUES.md`. Extend the planned `src/types.rs`, `src/typed.rs`, `src/checker.rs`, located syntax/parser and emitter. Runtime C is compiled object code linked through the shared target driver, never Rust interpreter code.

`Type::Text` LLVM value stays exactly `{ usize, ptr }`: UTF-8 byte length and data pointer, matching the web plan. `Type::List(Box<Type>)` and `Type::Map(Box<Type>)` replace native-only NumberList in checked IR. Their runtime LLVM values are opaque storage pointers with statically known element descriptors. The legacy numeric-list type adapts to `List(f64)` before lowering.

Every dynamic text buffer has an aligned private header immediately before its bytes; literals are emitted with the same header layout and immortal flag. Runtime clone/release recover the header from text's data pointer. Header fields are target-sized `refs`, `capacity_bytes`, `allocation_bytes`, followed by fixed-width `kind`/`flags`; `value.h` defines layout, align-up rules and static assertions per target. Zero-length text is one immortal empty allocation. No reference count is stored inside the public text descriptor. Do not read a header before a borrowed JS/C pointer: borrowed input must be copied first.

`LisanTypeOps { size_t size, align; LisanStatus (*clone)(LisanContext *,const void *,void *); void (*release)(LisanContext *,void *); bool (*equal)(const void *, const void *) }` (forward-declare context/status before descriptors) is a generated, target-local descriptor per concrete element type. Lists contain `refs,len,capacity,ops,data`; maps contain `refs,len,capacity,ops,entries` with insertion-ordered unique text keys and homogeneous values. Initial maps use linear lookup, avoiding an unplanned hashing/security contract. Recursive types and cycles are impossible in this finite static type grammar; runtime destruction is iterative using explicit work storage, with a fallback that does not allocate during teardown.

`LisanAllocator { void *state; void *(*alloc)(void *,size_t,size_t); void (*dealloc)(void *,void *,size_t,size_t) }`; `LisanContext { LisanAllocator allocator; LisanError error; }`; `LisanStatus` values `OK=0, OOM=1, SIZE_OVERFLOW=2, BOUNDS=3, MISSING_KEY=4, INVALID_UTF8=5, REFCOUNT_OVERFLOW=6`. Errors carry source-site ID; an emitted source table resolves it to file/line/column. Failed operations leave their output and all original operands unchanged. Runtime release/deallocation cannot fail or allocate.

Runtime ABI names: `lisan_rt_text_from_utf8(ctx,bytes,byte_len,out_text)`, `lisan_rt_text_concat(ctx,a,b,out_text)`, `lisan_rt_text_at(ctx,text,scalar_index,out_text)`, `lisan_rt_list_new(ctx,ops,len,items,out_list)`, `lisan_rt_list_set(ctx,in_list,index,value,out_list)`, `lisan_rt_map_new(ctx,ops,out_map)`, `lisan_rt_map_set(ctx,in_map,key,value,out_map)`, `lisan_rt_map_get(ctx,map,key,out_value)` all return `LisanStatus` and borrow inputs. `lisan_rt_text_release(ctx,text)`, `lisan_rt_list_release(ctx,list)`, `lisan_rt_map_release(ctx,map)` return void; copying a reference uses checked `lisan_rt_*_clone(ctx,in,out)` returning REFCOUNT_OVERFLOW for a non-immortal maximum refcount. Compiler ownership copies use checked clone, not unchecked retain.

All source functions transitively invoking a fallible managed operation gain an internal checked calling convention: hidden `(ptr context, ptr result_out, ordinary typed arguments...) -> i32 status`; void omits result_out, which remains uninitialized on failure. Borrowed managed parameters remain caller-owned for call duration; result slots contain one owned result on success. Non-fallible scalar functions retain the core's direct-result convention. The checker computes `FunctionEffects { may_fail: bool }` by call-graph fixed point, including recursion. This is an internal ABI change owned jointly with the core emitter, never a C ABI promise. Every failing call branches through generated cleanup and propagates status; top-level hosted entry renders an Arabic located error and exits 1. Export adapters return status and do not expose result_out aggregates publicly.

### Task 1: Runtime Allocation and Ownership Primitives

**Files:** Create `runtime/common/value.h`, `runtime/common/value.c`, `runtime/host/allocator.c`, `tests/runtime_managed.rs`, `tests/fixtures/managed/runtime_harness.c`; extend shared target runtime dependency/link selection.

**Interfaces:** Produce the context, error, header, allocator and checked-clone contracts above; test allocator exposes `attempts`, `live_blocks`, `live_bytes`, `fail_at` through its state. Host allocator implements requested alignment and records original allocation for correct deallocation.

- [ ] Write native C harness cases `allocation_failure_is_transactional`, `size_math_overflow`, `refcount_overflow`, `empty_immortal`, `release_never_allocates`, and `alignment_all_supported_scalars`. Assert zero live blocks after each case and unchanged sentinel outputs on failure; fail every allocation index in a multi-allocation constructor.
- [ ] Run `cargo test --test runtime_managed`; first establish failure, then implement checked `add/mul/align_up`, allocation/header handling, clone and release. Allocate no recursive teardown scratch memory; use nonrecursive intrusive pending links only after storage reaches zero references, preserving shared DAG edges.
- [ ] Run the harness with `-fsanitize=address,undefined` and leak detection enabled. Require zero findings, no overflow before allocation, and no double free. Commit allocator/runtime foundation with its evidence.

### Task 2: Length-Bearing UTF-8 Text

**Files:** Extend `value.c`, `src/native/managed.rs`, `src/checker.rs`, `src/native.rs`, `src/types.rs`; create `tests/native_text_values.rs`; document text contract in `docs/MANAGED-VALUES.md`.

**Interfaces:** Produce text helpers above plus `lisan_rt_text_scalar_len(text)->size_t`, `lisan_rt_text_equal(a,b)->bool`; scalar_len cannot fail for validated text. Printing uses bytes+length, never puts/strlen. Hosted printing may call fwrite through the runtime.

- [ ] Add executable `arabic_scalar_length`, `embedded_nul_roundtrip`, `emoji_indexing`, `text_concat_scope_cleanup`, `invalid_utf8_rejected`, and `text_bounds_located_error`. Assert `طول("سَ😀") == 3`, indexing returns one Unicode scalar as Text, empty length is zero, NUL does not truncate printing/equality, and byte counts remain distinct from scalar length. No normalization or grapheme segmentation occurs.
- [ ] Run `cargo test --test native_text_values`; establish failure, then emit immortal length-bearing UTF-8 literals and implement validated copy, concatenation, comparison and scalar indexing. Index syntax accepts integer scalars or legacy finite/integral/nonnegative f64 after checked conversion; reject fractions, infinity and out-of-range values before addressing memory.
- [ ] Implement straight-line top-level ownership and cleanup first: each constructed text temporary/local owns one reference, assignment clones before releasing, successful scope exit releases in reverse order, and every fallible helper branches to that same cleanup before returning status to hosted entry. Until Task 4, reject managed values crossing source-function boundaries or entering branch/loop bodies with `E_UNSUPPORTED`; do not silently use the old leaking paths. Remove native strlen/strcpy/strcat text paths and process-lifetime concatenation allocation behavior only after this emitter owns cleanup. Run native text regressions and allocation failpoint harness; require no leaks across 100,000 concatenation/index operations. Commit fully cleaned-up native text, not an intermediate leaking representation migration.

### Task 3: Homogeneous Lists and Ordered Text-Key Maps

**Files:** Extend type/parser/checker/typed IR, `value.c`, `src/native/managed.rs`; create `tests/native_managed_collections.rs`, `tests/fixtures/managed/collections.lsn`; extend `docs/MANAGED-VALUES.md` and `docs/NATIVE-TYPES.md`.

**Interfaces:** Types use proposed syntax `قائمة<ص32>` and `قاموس<نص>` (map key type is always Text). Empty literals require an annotated expected type; nonempty literals infer one element type and contextually type numeric literals. Add typed builtins `أضف(list,value): void`, `احذف(map,key): منطقي`, `يحتوي(map,key): منطقي`; preserve existing `طول` and index/assignment syntax. List append and index assignment update only the named variable's value.

- [ ] Write executable `list_alias_copy_on_write`, `nested_alias_copy_on_write`, `map_alias_copy_on_write`, `map_insert_order`, `missing_key_error`, `mixed_literal_rejected`, `empty_requires_context`, and `structural_equality`. Assert `ب=أ; ب[0]=9` leaves أ unchanged, including nested containers copied between top-level local variables; returned nested-container behavior belongs to Task 4. Maps replace existing keys without changing order; deleting and reinserting appends the key. Equality compares list order/content; map equality compares key/value content independent of insertion order; float NaN follows scalar equality.
- [ ] Run `cargo test --test native_managed_collections`; establish failure, then implement heap storage, checked capacity growth, transactional copy-on-write and descriptor-driven nested clone/release/equality. Mutations evaluate target/index/value left-to-right once. Failed mutation preserves both aliases and live-block totals.
- [ ] Extend Task 2’s nonescaping straight-line ownership/cleanup to lists and maps, including transactional nested clone failure; retain the temporary restriction on managed function boundaries and source control-flow bodies until Task 4. Add `nested_container_oom_preserves_aliases` native tests, injecting failure at every allocation point. Define foreach’s later contract now: retained snapshot in insertion order for maps (iterates keys) and list order for lists; mutation detaches and cannot invalidate the snapshot. Implement and execute that control-flow acceptance in Task 4.
- [ ] Run collection, text and runtime suites under sanitizers, including deep nested finite types and repeated map replacement/deletion. Commit checked native collections with documented equality/copy rules.

### Task 4: Typed Calls, Failure Propagation and Complete Cleanup

**Files:** Extend `src/checker.rs`, `src/typed.rs`, `src/native.rs`, `src/native/managed.rs`; create `tests/native_managed_functions.rs`, `tests/native_managed_cleanup.rs`; update `docs/MEMORY.md`.

**Interfaces:** Produce FunctionEffects fixed point and checked calling convention above; typed IR identifies owned temporary IDs and lexical drop scopes. `CleanupScope { owned: Vec<OwnedValueId> }` is emitter bookkeeping, not runtime reflection. `emit_cleanup(scope, except: Option<OwnedValueId>)` releases in reverse creation order, skipping moved return value.

- [ ] Write actual native `return_text_list_map`, `text_concat_function_result`, `nested_collection_function_result`, `mutate_during_iteration_is_snapshot`, `recursive_managed_return`, `callee_retains_parameter_only_on_copy`, `all_return_paths_cleanup`, `break_continue_cleanup`, `short_circuit_temporary_cleanup`, `failing_recursive_callee_unwinds`, and `oom_never_reads_uninitialized_result`. Assert exact results and zero live allocations after the owning top-level result is released; failure caller output sentinel is untouched.
- [ ] Run both test targets; establish failure, then compute transitive effects, lower checked calls and route return/failure/loop control through shared cleanup blocks. Remove Tasks 2–3’s explicit nonescaping/control-flow restrictions only after these tests pass. Implement foreach as a retained owned snapshot released on loop exhaustion, break, early return and failure; continue retains the same snapshot. Parameters are borrowed, locals and temporaries own one reference; assignment clones RHS before releasing old LHS, making `س=س` safe. Returning a local transfers its owned reference; returning a borrowed parameter first clones. Evaluate call arguments left-to-right and release constructed arguments when a later argument fails.
- [ ] Route all runtime bounds/key/UTF-8 errors through this status channel; no libc exit or raw trap may bypass live managed cleanup. Hosted entry renders the stored source error once and exits 1. This runtime failure mechanism does not yet introduce a source-level generic Result type; expected file/number-parse success/failure syntax remains owned by the standard-library/error plan.
- [ ] Run complete `cargo test` and sanitizer suites; require legacy scalar native behavior and typed core regressions. Commit managed functions only with failure-path cleanup evidence.

### Task 5: Browser Bridge and Foreign Boundary Exclusions

**Files:** Extend `runtime/browser/runtime.c`, `runtime/browser/abi.h`, `web/lisan-host.mjs`, `docs/web-abi.md`; create `tests/managed_abi.rs`, extend `web/tests/browser.spec.mjs` (consume files created by web plan).

**Interfaces:** Browser reuses common value.c and supplies LisanAllocator with wasm32 `size_t`, explicit maximum memory and reusable free storage. Keep web exports `lisan_alloc(len:i32)->i32`, `lisan_free(ptr:i32,len:i32)->void` as raw bridge allocations, separate from text-header managed allocations. Add internal `lisan_rt_text_from_utf8` copy on borrowed JS input; never fabricate managed descriptors around bridge allocations. Browser uses the same `{ usize, ptr }` text representation and status codes.

- [ ] Write native checker tests rejecting Text/List/Map by-value foreign imports/exports, pointer-to-managed-type, managed values in raw address/load/store, and implicit C-string conversion. Only explicit byte-pointer+byte-length bridge adapters may copy into/out of managed text; a C callee cannot retain a borrow without its own explicit copy protocol.
- [ ] Add actual browser tests for embedded NUL/Arabic roundtrip, forced memory growth, 10,000 managed callbacks, OOM followed by a valid smaller request, and malformed UTF-8 rejection. Refresh JS views after allocation/calls; assert live allocation count returns to baseline between events and no monotonic growth within stable workload capacity.
- [ ] Implement caller-buffer output/status adapters from the web plan: report required byte length without partial writes, copy synchronously on success, release the temporary result on success and insufficient-capacity paths. Browser errors propagate to adapter status, not a trap that skips cleanup; host can render the error and continue.
- [ ] Run native ABI rejection tests and pinned Chromium integration. Missing browser/toolchain is a blocked browser milestone, not native failure or browser support evidence. Commit shared managed bridge only after executing the real wasm module.

### Task 6: Release Evidence and Compatibility Documentation

**Files:** Extend `scripts/check-linux.sh`, `docs/NATIVE-COMPILER.md`, `docs/MEMORY.md`, `docs/MANAGED-VALUES.md`, `README.md`; create `examples/native_managed_values.lsn`, `tests/native_managed_acceptance.rs`.

- [ ] Add a standalone compiled example returning nested lists/maps/text across recursive functions, then performing snapshots, equality, mutation and Arabic text indexing. Run its machine executable without the compiler/interpreter installed; inspect dependencies and runtime symbols.
- [ ] Run all native suites, C runtime ASan/UBSan/LSan tests, fail-every-allocation tests and sustained live-process stress. Require zero leaked/double-freed/use-after-free blocks and stable high-water memory under repeated fixed-size workload. Report compiler commit, target, tool versions, commands and outcomes; never mark skipped tools as success.
- [ ] Document automatic memory/value semantics, scalar versus byte lengths, map iteration/equality, failure/cleanup behavior and FFI exclusions. Update the 1.0 checklist only with exact test evidence; do not infer full 1.0 completion from this workstream.
- [ ] Commit acceptance and documentation. Keep host-tested and browser-tested statuses separate; freestanding managed support remains unavailable until an allocator and resource-budget tests are supplied for that named target.

## Dependencies and Completion

Tasks 2–3 are independently releasable only for nonescaping straight-line top-level managed operations with complete local/failure cleanup; they explicitly reject managed function calls/results and control-flow bodies until Task 4. Tasks 1–4 close the compiler-core plan's explicit managed-value deferral for the hosted 1.0 target. They require its located syntax, scalar types, typed functions and source diagnostics; representation migration is atomic per delivery so old stack-backed lists and raw NUL text cannot accidentally mix with new values. Task 5 consumes the web plan's runtime/exports rather than duplicating a browser backend. Package management, file handles, source-level recoverable errors and library API inventory remain their own plans; external resources are never released merely by decrementing a text/list/map reference.
