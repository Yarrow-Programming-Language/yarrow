# yarrow-cli

Command-line interface for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-cli` provides the main entry point for interacting with the Yarrow interpreter and toolchain from the command line. It delegates to:

- [`yarrow-core`](../yarrow-core) for parsing and interpretation
- [`yarrow-fmt`](../yarrow-fmt) for code formatting
- [`yarrow-lsp`](../yarrow-lsp) for Language Server Protocol support

## Installation

Build the CLI from source:

```bash
cargo build -p yarrow_cli
```

The binary will be available at `./target/debug/yarrow`.

Or install to your system:

```bash
cargo install --path crates/yarrow-cli
```

## Usage

```bash
yarrow [OPTIONS] [SUBCOMMAND]
```

### Commands

| Command | Description                               |
| ------- | ----------------------------------------- |
| `interpret` | Parse and execute a Yarrow file           |
| `repl`   | Start an interactive interpreter session   |
| `fmt`   | Format Yarrow source files                |
| `lsp`   | Start the Language Server Protocol server |
| `explain` | Print the long form of a diagnostic code   |
| `version` | Print the CLI version                     |

### Examples

```bash
# Run a Yarrow file
yarrow interpret hello.yar

# Start the REPL
yarrow repl

# Format a file in-place
yarrow fmt my_file.yar

# Start the LSP server
yarrow lsp

# Format all Yarrow files in a directory
yarrow fmt ./src/

# Print version
yarrow version

# Explain a diagnostic code
yarrow explain E360
```

## Subcommands

### `interpret`

Parse and execute a Yarrow program:

```bash
yarrow interpret [OPTIONS] <FILE>
```

Options:

- `--main <NAME>` - Entry function name (default: `main`)
- `-L <DIR>` - Add module search path
- `--error-limit <N>` - Maximum number of errors to report

### `repl`

Start an interactive interpreter session:

```bash
yarrow repl [OPTIONS]
```

The REPL provides an interactive environment for testing Yarrow code snippets.

### `fmt`

Format Yarrow source code:

```bash
yarrow fmt [OPTIONS] [FILES...]
```

Options:

- `--check` - Check formatting without modifying files
- `--write` - Write formatted output back to files (default)
- `--stdin` - Read from stdin

### `lsp`

Start the Language Server:

```bash
yarrow lsp [OPTIONS]
```

Options:

- `--listen <ADDR>` - Listen on TCP address (e.g., `127.0.0.1:8080`)
- `--log-level <LEVEL>` - Set log level: `off`, `error`, `warn`, `info`, `debug`
- `--stdio` - Use stdin/stdout (default when `--listen` is omitted)
- `-L <DIR>` - Add module search path
- `--main <NAME>` - Entry function name
- `--no-format` - Disable `textDocument/formatting`
- `--no-inlay` - Disable `textDocument/inlayHint`

See [`yarrow-lsp`](../yarrow-lsp/README.md) for more details.

### `explain`

Print the long form of a diagnostic code:

```bash
yarrow explain [OPTIONS] <CODE>
```

### `version`

Print the CLI version:

```bash
yarrow version
```

## Environment Variables

| Variable       | Description   | Default |
| -------------- | ------------- | ------- |
| `YARROW_COLOR` | Color output  | `auto`  |

## Exit Codes

| Code | Meaning                     |
| ---- | --------------------------- |
| `0`  | Success                     |
| `1`  | User errors (parsing, execution) |
| `2`  | Usage / I/O errors           |
| `101`| Internal errors              |

## Project Structure

```
crates/yarrow-cli/
├── src/
│   ├── lib.rs          # CLI library
│   ├── args.rs         # Command-line argument parsing
│   ├── commands/       # Subcommand implementations
│   │   ├── interpret.rs
│   │   ├── repl.rs
│   │   ├── fmt.rs
│   │   ├── lsp.rs
│   │   ├── explain.rs
│   │   └── version.rs
│   └── main.rs         # Binary entry point
└── Cargo.toml
```

## See Also

- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [`yarrow-core`](../yarrow-core/README.md) - Core interpreter implementation
- [`yarrow-fmt`](../yarrow-fmt/README.md) - Code formatter
- [`yarrow-lsp`](../yarrow-lsp/README.md) - Language Server Protocol
