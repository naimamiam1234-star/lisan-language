# Hosted homogeneous lists increment

- [x] Establish failing native acceptance tests before implementation.
- [x] Add `Type::List(Box<Type>)`, `قائمة<T>` annotations and typed literal inference. Empty literals require context. Supported T: fixed width integer/float, bool, text. Nested lists, pointers, void and maps remain diagnosed.
- [x] Implement opaque reference-counted runtime storage with descriptor-driven checked clone/release/equality. All constructors and mutations are transactional; append and indexed set produce fresh storage, retaining value semantics. Capacity/refcount arithmetic is checked.
- [x] Add literal, length, index, indexed assignment, `أضف(named_list,value)` and structural equality IR/lowering. Evaluate target snapshot, index, RHS once left-to-right. Snapshot list/text items during literal/call evaluation.
- [x] Generalize active call-frame cleanup using typed release callbacks. Borrowed source function list parameters copy into owned locals; list results use owned output slots. Recursion and fatal located failures release every active frame. This remains hosted fatal-error behavior, not recoverable status propagation.
- [x] Verify aliases, scalar/text lists, all scalar widths, context/inferred/empty literals, bounds, functions/recursion, self mutation and earlier argument snapshots. Instrument allocator failpoints and live counts, execute sanitizer runtime checks and full suite.
- [x] Document exact support and exclusions and commit explicit source/test/doc paths. Preserve legacy NumberList and interpreter semantics; no fallback for typed managed collection programs.

Internal list ABI: caller-owned slot containing one opaque storage pointer; zero is empty/uninitialized and release-safe. Concrete element operations selected from target-local runtime descriptors. Managed list values never cross foreign ABI or raw pointer operations. All call-frame records store slot plus release callback, permitting mixed text/list ownership. List storage destruction is nonrecursive because this increment excludes nested containers.

Evidence: 2026-10-23, Rust 1.75.0, Clang 18.1.3, GCC 13.3.0; `LISAN_SANITIZER_LEAKS=0 cargo test --target-dir /tmp/lisan-collections-tests`: 212 passed, 0 failed across 22 suites, including 10 native collection tests and 4 runtime contracts/sanitizer tests. GCC ASan/UBSan executed for C runtime tests; local LSan unavailable/disabled. No browser, freestanding, map or nested-list claim.
