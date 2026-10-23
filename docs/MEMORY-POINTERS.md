# Bounded scalar pointers (0.9 preview)

Native typed programs support taking the address of a local scalar, passing it to a function, and loading or storing its value. This increment deliberately uses immutable pointer bindings instead of general lifetime inference or loop data-flow analysis.

```lisan
خارجي دالة املأ(م: مؤشر<ص32>): فراغ = "fill_i32"؛
ليكن س: ص32 = 0؛
ليكن م = عنوان(س)؛
غير_آمن {
    املأ(م)؛
    خزن(م، 42)؛
    اطبع(تحميل(م))؛
}
```

The matching C declaration is `void fill_i32(int32_t *value)`. Compile the C implementation separately and link it through the native build command's `--link` option. The callee may update the scalar during the call and must not retain its address afterward.

## Operations and types

- `عنوان(name)` accepts a local variable or scalar function parameter whose type is a signed/unsigned integer, float, or boolean. It returns a pointer to that exact type. Taking an address itself does not require an unsafe block.
- `تحميل(pointer)` requires `غير_آمن` and returns the exact scalar pointee type.
- `خزن(pointer، value)` requires `غير_آمن`, requires the exact scalar pointee type (ordinary contextual numeric literal rules apply), and returns `فراغ`.
- `مؤشر_فارغ<type>()` constructs a typed null. Dereferencing null traps at runtime; creating and passing null is permitted. A foreign function's own nullability requirements remain the caller's responsibility.
- Foreign calls require `غير_آمن` even when their arguments contain no pointers. A surrounding unsafe block does not make pointer escapes or invalid types legal.

Bindings that hold pointers, including pointer parameters, cannot be reassigned. A fresh binding can copy an existing pointer. Scalar values remain mutable, including through `خزن`; pointer immutability constrains the binding, not the pointee.

Pointer-to-pointer and void-pointer signatures and null values remain available for opaque FFI use. This increment does not dereference them or take the address of pointer bindings. Taking the address of text, a temporary expression, or a collection is unsupported. Pointer arithmetic, integer/pointer casts, implicit text-to-C-string conversion, and storing pointer values through memory are unsupported.

## Lifetimes and aliases

The checker records the local stack roots of each pointer at initialization. Copies retain those roots. A pointer-valued call result conservatively inherits the roots of **every pointer argument**, for both native and foreign functions. Consequently an identity function or nested foreign call cannot hide the address of a local variable.

Returning a pointer with any local stack root is rejected with `E_POINTER_ESCAPE`, including the address of a scalar parameter's local stack slot. Returning a borrowed pointer parameter, null, or a pointer returned by a foreign function with no local-pointer arguments is permitted. The latter remains subject to the foreign library's ownership and validity contract. The conservative union can reject a call result that is actually independent of one of its arguments; this preview has no per-function alias annotations.

Pointer initialization occurs within lexical scope, pointer reassignment is forbidden everywhere (`E_POINTER_IMMUTABLE`), and pointer-containing memory stores are unsupported. Thus pointers cannot be moved from an inner scope into an existing outer binding, and loop iterations cannot change a pointer's recorded roots. This is the bounded rule that avoids requiring branch joins or loop fixed points in this increment.

## Unsafe caller contract

The programmer must ensure foreign pointers are valid, correctly aligned, initialized for loads, and point to writable storage for stores. The ABI and exact pointee type must match the C declaration. Foreign callees must not retain any pointer to caller-local storage after returning, publish it globally, free it, or return an unrelated pointer to invalid storage. The checker cannot inspect foreign code or enforce these promises. Its local-root tracking prevents the language-level escapes described above; it is not a general memory-safety guarantee for arbitrary C code.

The native backend guards null loads and stores. Non-null dangling, unaligned, or otherwise invalid foreign addresses are outside the supported contract. There is no pointer ownership, allocation, deallocation, callbacks, or general lifetime inference in this preview.
