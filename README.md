<p align="center">
  <img src="assets/logo.jpg" alt="Yarrow" width="480">
</p>

<p align="center">
  <b>A typed, stack-based language with ownership, regions, and explicit unsafe</b>
</p>

<p align="center">
  Safe by default · No lifetime annotations · Interpreter-only with LSP
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
yarrow docs/examples/valid/01_hello.yar
# Hello, Yarrow!
```

## Quick start

Requires a Rust toolchain (edition 2024).

```bash
cargo build --release
./target/release/yarrow docs/examples/valid/01_hello.yar
```

Common commands:

```bash
yarrow interpret <file.yar>    # Parse and execute
yarrow repl                    # Interactive REPL
yarrow fmt [--check] [path]    # Format source code
yarrow lsp                     # Start language server
yarrow explain E360            # Look up error code
```

## Repository layout

- `crates/yarrow-core` - tokenizer, parser, and interpreter
- `crates/yarrow-runtime` - host heap and `HOST_FNS` (interpreter runtime)
- `crates/yarrow-cli` - Clap-based CLI driver for the interpreter
- `crates/yarrow-fmt` - code formatter
- `crates/yarrow-lsp` - language server
- `docs/` - [language reference, style guide, and examples](docs/README.md)
- `crates/yarrow-core/lib/std` - standard library written in Yarrow

Implementation roadmaps: `crates/**/PLAN.md`

## Project status

Yarrow is now interpreter-only. The interpreter provides execution with LSP support for development. Language design is documented in [`docs/`](docs/README.md); prefer the grammar specification when examples and implementation disagree. Note that without a type checker, the LSP provides AST-based analysis only.

## Documentation

- [Documentation index](docs/README.md)
- [Grammar tour](docs/GRAMMAR.md)
- [Examples](docs/examples/README.md)
- [Style guide](docs/STYLE_GUIDE.md)

## AI contribution

The Yarrow language syntax and rules were designed by the project author. The implementation - including the tokenizer, parser, type checker, and interpreter - was developed with AI assistance.
