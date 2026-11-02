# yarrow-runtime-aot (Deprecated)

**This crate is deprecated.** Since Yarrow has been transitioned to an interpreter-only implementation, AOT (Ahead-of-Time) compilation is no longer supported.

## Historical Context

This directory previously contained a static library (`yarrow_runtime_aot`) that provided runtime support for ahead-of-time compiled Yarrow programs. It exposed stable, linker-visible symbols that compiled Yarrow code could call at runtime.

## Current State

With the removal of the compiler and Cranelift integration, AOT compilation is no longer possible. All execution is now performed by the interpreter in `yarrow-core`.

## Migration

If you were using AOT compilation, you should now use the interpreter:

```rust
// Instead of compiling to object code:
// let obj = compiler.compile_to_object("my_program.yar")?;

// Use interpretation:
use yarrow_core::{Session, SessionOptions};

let opts = SessionOptions::new("my_program.yar");
let session = Session::new(opts);
let result = session.interpret_source(source)?;
```

## See Also

- [`yarrow-core`](../README.md) - Core interpreter implementation
- [Yarrow Language Specification](../../../docs/GRAMMAR.md)
