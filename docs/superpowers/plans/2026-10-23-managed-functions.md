# Hosted managed function increment

- [ ] Replace global descriptors with zero-initialized per-invocation stack slots.
- [ ] Register every active invocation in a linked cleanup stack; fatal guards release all frames before destroying the context.
- [ ] Borrow text parameters, snapshot call arguments left-to-right, and clone text results into hidden caller-owned output slots before frame cleanup.
- [ ] Enable internal text signatures and bodies; reject foreign managed ABI and managed object exports.
- [ ] Exercise recursion, early return, argument mutation, failpoints and live allocation accounting; run full Rust suite.

This hosted increment retains fatal located errors and a single process context. It deliberately does not implement recoverable hidden-status/context propagation: scalar ABI remains unchanged; text returns use hidden output pointers. Cleanup frame registration makes fatal errors unwind ownership without unwinding the machine stack. Reentrant foreign callbacks and concurrent execution remain unsupported.
