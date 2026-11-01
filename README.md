<p align="center">
  <img src="assets/logo.jpg" alt="Yarrow" width="480">
</p>

<p align="center">
  <b>A typed, stack-based language with ownership, regions, and explicit unsafe</b>
</p>

<p align="center">
  Safe by default · No lifetime annotations · Cranelift JIT, object emit, and host link
</p>

---

Yarrow is a statically typed stack-based language. It combines postfix syntax with explicit stack effects, ownership, and regions to provide memory safety without lifetime annotations. Unsafe operations are explicitly marked and still respect ownership and borrow checking.

## Example

```yarrow
"std.io" io require

greet function
	string
do
	"Hello, " swap ~ io.write_line call
end

main function do
	"Yarrow!" greet call
end
```

```bash
cargo run -- docs/examples/valid/01_hello.yar
# Hello, Yarrow!
```

## Quick start

Requires a Rust toolchain (edition 2024) and, for native linking on Linux, a system linker (`ld` or `lld`).

```bash
cargo build --release
cargo run -- docs/examples/valid/01_hello.yar
```

Common commands:

```bash
cargo run -- check <file.yar>          # Type-check only
cargo run -- compile -o out.o <file>   # Compile to object file
cargo run -- run <file.yar>             # Compile and run (native)
cargo run -- run --target jit <file>   # JIT execution
cargo run -- interpret <file.yar>      # Interpreted execution
cargo run -- dump --emit ast <file>    # AST dump
cargo run -- explain E360               # Look up error code
```

`cargo run -- <file.yar>` defaults to `run --target object` with `main` as entry point. Use `--target jit` for in-process execution or `--main` to specify a different entry.

## Repository layout

- `crates/yarrow-core` - tokenizer, parser, type checker, JIT/object/interpret execution (`Session` API)
- `crates/yarrow-runtime` - host heap and `HOST_FNS`; `aot/` contains staticlib for linking
- `crates/yarrow-cli` - Clap-based CLI driver
- `crates/yarrow-fmt` - code formatter (planned)
- `crates/yarrow-lsp` - language server (planned)
- `docs/` - [language reference, style guide, and examples](docs/README.md)
- `crates/yarrow-core/lib/std` - standard library written in Yarrow

Implementation roadmaps: `crates/**/PLAN.md`

## Project status

Core compiler and CLI support compile, check, interpret, and native execution. Language design is documented in [`docs/`](docs/README.md); prefer the grammar specification when examples and implementation disagree. Formatter and LSP are next on the roadmap.

## Documentation

- [Documentation index](docs/README.md)
- [Grammar tour](docs/GRAMMAR.md)
- [Examples](docs/examples/README.md)
- [Style guide](docs/STYLE_GUIDE.md)

## AI contribution

The Yarrow language syntax and rules were designed by the project author. The compiler implementation - including the tokenizer, parser, type checker, and execution backends (JIT, object emit, interpret) - was developed with AI assistance.
