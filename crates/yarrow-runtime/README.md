# yarrow-runtime

Runtime library for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-runtime` provides the **execution environment** for Yarrow programs. It implements:

- **Host heap** - Memory allocation for Yarrow values
- **Host functions** - Standard library and FFI functions
- **Type representations** - Runtime type information
- **Value representations** - How Yarrow values are stored in memory
- **GC integration** - Garbage collection support (future)

## Architecture

### Memory Model

Yarrow uses a **region-based memory model** with explicit ownership:

- **Stack values** - Values owned by the stack (local variables)
- **Heap values** - Values allocated on the host heap (boxed types, arrays)
- **Borrowed values** - References with limited lifetimes
- **Regions** - Explicit memory regions for grouping allocations

### Value Representation

All Yarrow values are represented using a **tagged union**:

```rust
pub enum Value {
    // Primitive types
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),

    // Composite types
    Array(Box<[Value]>),
    Tuple(Box<[Value]>),
    Struct { fields: BTreeMap<String, Value> },

    // Reference types
    Ref(*mut Value),           // Raw pointer (unsafe)
    Borrowed(&'static Value),  // Static reference

    // Function types
    Function(FunctionPtr),
    Closure { func: FunctionPtr, captures: Box<[Value]> },

    // Type representations
    Type(TypeId),

    // Special values
    Unit,
    None,
}
```

### Host Functions

The runtime provides **host functions** (HOST_FNS) that can be called from Yarrow code:

```rust
// Example: std.io.write_line
fn write_line(args: &[Value]) -> Result<Value, RuntimeError> {
    let string = args[0].as_string()?;
    println!("{}", string);
    Ok(Value::Unit)
}

// Registered as:
HOST_FNS.insert("std.io.write_line", write_line);
```

## Features

### Heap Management

```rust
use yarrow_runtime::heap::Heap;

let heap = Heap::new();

// Allocate a value
let value = heap.allocate(Value::Int(42));

// Create a reference
let reference = heap.reference(value);
```

### Host Function Registry

```rust
use yarrow_runtime::host::HostFn;

// Define a host function
let my_func: HostFn = |args, heap| {
    let a = args[0].as_int()?;
    let b = args[1].as_int()?;
    Ok(heap.allocate(Value::Int(a + b)))
};

// Register it
HostFnRegistry::global().register("my.add", my_func);
```

### Type System

```rust
use yarrow_runtime::types::{Type, TypeId};

let int_type = Type::Int;
let bool_type = Type::Bool;
let array_type = Type::Array(Box::new(Type::Int));
```

## Standard Library Support

`yarrow-runtime` provides the implementation for the Yarrow standard library modules:

| Module     | Description  | Host Functions                                   |
| ---------- | ------------ | ------------------------------------------------ |
| `std.io`   | Input/Output | `write_line`, `read_line`, `print`, `eprint`     |
| `std.math` | Mathematics  | `add`, `sub`, `mul`, `div`, `sqrt`, `sin`, `cos` |
| `std.mem`  | Memory       | `alloc`, `dealloc`, `copy`, `move`               |
| `std.time` | Time         | `now`, `since_epoch`, `sleep`                    |
| `std.fs`   | File System  | `read_file`, `write_file`, `delete_file`         |
| `std.os`   | OS           | `args`, `env`, `exit`, `time`                    |

## AOT Compilation

For ahead-of-time compilation, see [`yarrow-runtime-aot`](aot/README.md).

### AOT Static Library

The `aot/` directory contains a static library (`yarrow_runtime_aot`) that:

- Exposes stable, linker-visible symbols for AOT-compiled programs
- Provides the runtime initialization and cleanup
- Includes all host functions for standalone executables

## Configuration

### Features

| Feature       | Description                        |
| ------------- | ---------------------------------- |
| `default`     | No additional features             |
| `aot-exports` | Export symbols for AOT compilation |

Enable features in `Cargo.toml`:

```toml
[dependencies]
yarrow_runtime = { path = "../yarrow-runtime", features = ["aot-exports"] }
```

## Project Structure

```
crates/yarrow-runtime/
├── src/
│   └── lib.rs              # Main runtime library
├── aot/
│   ├── src/
│   │   └── lib.rs          # AOT static library
│   └── Cargo.toml
└── Cargo.toml
```

## Dependencies

### External

- `libc` - C library bindings for system calls

## Usage from Yarrow

In Yarrow code, host functions are called like regular functions:

```yarrow
"std.io" io require

main function
    "Hello, World!" io.write_line
end
```

The runtime automatically resolves `io.write_line` to the host function.

## Error Handling

```rust
use yarrow_runtime::error::RuntimeError;

match some_operation() {
    Ok(value) => { /* success */ }
    Err(RuntimeError::TypeMismatch { expected, actual }) => {
        eprintln!("Type error: expected {}, got {}", expected, actual);
    }
    Err(RuntimeError::NullDereference) => {
        eprintln!("Null dereference");
    }
    Err(RuntimeError::OutOfBounds { index, len }) => {
        eprintln!("Index {} out of bounds for length {}", index, len);
    }
    // ...
}
```

## See Also

- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [Memory Model](../../docs/MEMORY_MODEL.md)
- [`yarrow-core`](../yarrow-core/README.md) - Core compiler
- [`yarrow-lsp`](../yarrow-lsp/README.md) - Language Server Protocol
- [`yarrow-runtime-aot`](aot/README.md) - AOT static library
