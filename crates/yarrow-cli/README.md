# yarrow-cli

Command-line interface for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-cli` provides the main entry point for interacting with the Yarrow compiler and toolchain from the command line. It delegates to:

- [`yarrow-core`](../yarrow-core) for compilation and execution
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
| `run`   | Compile and execute a Yarrow file         |
| `check` | Type-check a file without executing       |
| `fmt`   | Format Yarrow source files                |
| `lsp`   | Start the Language Server Protocol server |
| `build` | Compile to various output formats         |
| `doc`   | Generate documentation                    |

### Examples

```bash
# Run a Yarrow file
yarrow run hello.yar

# Check a file for errors
yarrow check my_file.yar

# Format a file in-place
yarrow fmt my_file.yar

# Start the LSP server
yarrow lsp

# Format all Yarrow files in a directory
yarrow fmt ./src/
```

## Subcommands

### `run`

Compile and execute a Yarrow program:

```bash
yarrow run [OPTIONS] <FILE>
```

Options:

- `--release` - Compile with optimizations
- `--emit <FORMAT>` - Output format: `jit` (default), `obj`, `asm`
- `--output <FILE>` - Write output to file

### `check`

Type-check without executing:

```bash
yarrow check [OPTIONS] <FILE>
```

Options:

- `--json` - Output diagnostics as JSON

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
- `--log-level <LEVEL>` - Set log level: `error`, `warn`, `info`, `debug`, `trace`

See [`yarrow-lsp`](../yarrow-lsp/README.md) for more details.

### `build`

Compile Yarrow code to various formats:

```bash
yarrow build [OPTIONS] <FILE>
```

Options:

- `--emit <FORMAT>` - Output format: `jit`, `obj`, `asm`, `exe`
- `--output <FILE>` - Output file path
- `--release` - Enable optimizations
- `--target <TRIPLE>` - Target platform triple

### `doc`

Generate documentation from Yarrow source:

```bash
yarrow doc [OPTIONS] <FILE>
```

## Environment Variables

| Variable       | Description   | Default |
| -------------- | ------------- | ------- |
| `YARROW_LOG`   | Logging level | `info`  |
| `YARROW_COLOR` | Color output  | `auto`  |

## Exit Codes

| Code | Meaning                     |
| ---- | --------------------------- |
| `0`  | Success                     |
| `1`  | Compilation/Type errors     |
| `2`  | Runtime error               |
| `3`  | Command-line argument error |

## Project Structure

```
crates/yarrow-cli/
├── src/
│   ├── lib.rs          # CLI library
│   ├── args.rs         # Command-line argument parsing
│   ├── commands/       # Subcommand implementations
│   │   ├── run.rs
│   │   ├── check.rs
│   │   ├── fmt.rs
│   │   └── ...
│   └── main.rs         # Binary entry point
└── Cargo.toml
```

## See Also

- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [`yarrow-core`](../yarrow-core/README.md) - Core compiler
- [`yarrow-fmt`](../yarrow-fmt/README.md) - Code formatter
- [`yarrow-lsp`](../yarrow-lsp/README.md) - Language Server Protocol
