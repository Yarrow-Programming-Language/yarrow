# yarrow-core

Core compiler implementation for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-core` is the **heart of the Yarrow compiler**. It provides:

- **Tokenization** - Lexical analysis of Yarrow source code
- **Parsing** - Syntax tree construction from tokens
- **Type checking** - Static type analysis and validation
- **Borrow checking** - Ownership and borrowing validation
- **Region checking** - Region-based memory safety
- **Compilation** - Code generation via Cranelift
- **Interpretation** - Direct execution without code generation
- **JIT compilation** - Just-in-time compilation for fast execution
- **Object emission** - Object file generation for linking
- **Session management** - Compilation session with state management

## Architecture

### Components

```
┌────────────────────────────────────────────────────────────────┐
│                          yarrow-core                           │
├─────────────────┬───────────────┬─────────────────┬────────────┤
│   Tokenizer     │    Parser     │    Analyzer     │   Link     │
│  (tokenizer/)   │   (parser/)   │  (analysis.rs)  │ (link.rs)  │
├─────────────────┴───────────────┴─────────────────┴────────────┤
│                                                                │
│  ┌─────────────┐      ┌───────────────┐      ┌──────────────┐  │
│  │  Compiler   │      │  Interpreter  │      │    Target    │  │
│  │ (compiler/) │      │ (interpreter/)│      │  (target.rs) │  │
│  └─────────────┘      └───────────────┘      └──────────────┘  │
│                                                                │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Session (session.rs)                  │  │
│  │  - Project management                                    │  │
│  │  - Module resolution                                     │  │
│  │  - Diagnostics collection                                │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

### Key Modules

| Module         | Purpose                                          |
| -------------- | ------------------------------------------------ |
| `tokenizer/`   | Lexical analysis (token stream generation)       |
| `parser/`      | Syntax parsing (AST construction)                |
| `compiler/`    | Code generation (Cranelift IR, machine code)     |
| `interpreter/` | Direct execution of Yarrow bytecode              |
| `analysis.rs`  | Type checking, borrow checking, region analysis  |
| `diagnostics/` | Error reporting and diagnostic management        |
| `session.rs`   | Compilation session state and project management |
| `link.rs`      | Module linking and dependency resolution         |
| `target.rs`    | Target platform configuration                    |
| `runtime.rs`   | Runtime integration                              |
| `project.rs`   | Project structure and file management            |
| `entry.rs`     | Compilation entry points                         |

## Features

### Language Features

- **Stack-based syntax** - Explicit stack manipulation
- **Strong typing** - Static type system with type inference
- **Ownership model** - Rust-like ownership with move semantics
- **Borrow checking** - Compile-time memory safety
- **Regions** - Explicit region-based lifetimes
- **Pattern matching** - Powerful pattern matching on types
- **Modules** - Modular code organization with imports/exports
- **Generics** - Parametric polymorphism
- **Unsafe blocks** - Explicit unsafe boundary for low-level operations

### Compilation Backends

| Backend         | Description                                          |
| --------------- | ---------------------------------------------------- |
| **JIT**         | Just-in-time compilation via Cranelift               |
| **Object**      | Object file emission for linking                     |
| **Interpreted** | Direct interpretation without code generation        |
| **AOT**         | Ahead-of-time compilation (via `yarrow-runtime-aot`) |

### Cranelift Integration

`yarrow-core` uses the [Cranelift](https://github.com/bytecodealliance/wasmtime/tree/main/cranelift) code generator for:

- Native code generation (x86_64, ARM64, etc.)
- JIT compilation for fast execution
- Object file emission for persistent artifacts

Dependencies:

- `cranelift-codegen` - Code generation
- `cranelift-frontend` - Frontend interface
- `cranelift-jit` - JIT compilation support
- `cranelift-module` - Module management
- `cranelift-object` - Object file emission
- `cranelift-native` - Native target support

## Usage

As a library:

```rust
use yarrow_core::{Compiler, Session, SessionConfig};

let config = SessionConfig::default();
let mut session = Session::new(config);
let compiler = Compiler::new(&mut session);

// Compile a string
let result = compiler.compile_string("main function 1 add print end");

match result {
    Ok(module) => {
        // Execute or inspect the compiled module
    }
    Err(diagnostics) => {
        // Handle compilation errors
    }
}
```

## Standard Library

The Yarrow standard library (`std`) is embedded in `yarrow-core` and includes:

- `std.io` - Input/output operations
- `std.math` - Mathematical functions
- `std.mem` - Memory allocation and management
- `std.time` - Time-related functions
- `std.fs` - File system operations
- `std.os` - Operating system interfaces

Standard library modules are accessible via the `yarrow-std:` URI scheme in LSP clients.

## Diagnostics

`yarrow-core` provides a rich diagnostic system with error codes:

| Code | Category           | Description                    |
| ---- | ------------------ | ------------------------------ |
| E1xx | Syntax errors      | Lexical/parse errors           |
| E2xx | Type errors        | Type checking failures         |
| E3xx | Semantic errors    | Borrow/region/ownership issues |
| E4xx | Compilation errors | Code generation failures       |
| E5xx | Runtime errors     | Execution failures             |

## Configuration

### Session Configuration

```rust
use yarrow_core::{SessionConfig, OptimizeLevel};

let config = SessionConfig {
    optimize: OptimizeLevel::Release,  // Optimization level
    emit_debug: true,                   // Emit debug information
    enable_jit: true,                   // Enable JIT compilation
    // ...
};
```

### Target Configuration

```rust
use yarrow_core::target::Target;

let target = Target::from_triple("x86_64-unknown-linux-gnu")?;
```

## Project Structure

```
crates/yarrow-core/
├── src/
│   ├── lib.rs              # Main library entry point
│   ├── entry.rs            # Compilation entry points
│   ├── session.rs          # Compilation session
│   ├── project.rs          # Project management
│   ├── target.rs           # Target platform
│   ├── runtime.rs          # Runtime integration
│   ├── link.rs             # Module linking
│   ├── analysis.rs         # Type and semantic analysis
│   ├── tokenizer/          # Lexical analysis
│   │   └── mod.rs
│   ├── parser/             # Syntax parsing
│   │   ├── mod.rs
│   │   ├── expr.rs
│   │   └── ...
│   ├── compiler/           # Code generation
│   │   ├── mod.rs
│   │   ├── cranelift.rs
│   │   └── ...
│   ├── interpreter/        # Interpretation
│   │   └── mod.rs
│   └── diagnostics/        # Error diagnostics
│       ├── mod.rs
│       └── code.rs
├── Cargo.toml
└── PLAN.md
```

## Dependencies

### Internal

- [`yarrow-runtime`](../yarrow-runtime/README.md) - Runtime library

### External

- `cranelift-*` - Code generation and compilation
- `object` - Object file handling
- `target-lexicon` - Target platform support
- `gimli` - Debug information
- `half` - Half-precision floating point

## See Also

- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [Yarrow Grammar](../../docs/GRAMMAR.md) - Complete language specification
- [`yarrow-cli`](../yarrow-cli/README.md) - Command-line interface
- [`yarrow-fmt`](../yarrow-fmt/README.md) - Code formatter
- [`yarrow-lsp`](../yarrow-lsp/README.md) - Language Server Protocol
- [`yarrow-runtime`](../yarrow-runtime/README.md) - Runtime library
