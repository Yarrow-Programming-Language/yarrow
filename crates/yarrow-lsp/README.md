# yarrow-lsp

Language Server Protocol (LSP) implementation for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-lsp` provides IDE integration for `.yar` files by implementing the LSP specification. It communicates over **stdio** (default) or **TCP** (`--listen`), delegating:

- **Analysis** (parsing, diagnostics, symbols, definitions, etc.) to [`yarrow-core`](../yarrow-core)
- **Formatting** to [`yarrow-fmt`](../yarrow-fmt)

Note: Since Yarrow is now interpreter-only without a type checker, the LSP provides AST-based analysis only. Type information and inlay hints are not available.

## Features

| LSP Feature                     | Supported | Notes                                           |
| ------------------------------- | --------- | ----------------------------------------------- |
| `textDocument/diagnostic`       | Yes       | Pull diagnostics via request (parsing only)     |
| `workspace/diagnostic`          | Yes       | Full workspace diagnostics (parsing only)       |
| `textDocument/definition`       | Yes       | Go-to-definition for values, types, and modules |
| `textDocument/hover`            | Yes       | AST-based signatures on hover                  |
| `textDocument/completion`       | Yes       | Context-aware completions                       |
| `textDocument/signatureHelp`    | Yes       | Function signature assistance (AST-based)        |
| `textDocument/rename`           | Yes       | Symbol renaming with workspace-wide updates     |
| `textDocument/references`       | Yes       | Find all references to a symbol                 |
| `textDocument/documentSymbol`   | Yes       | Document outline (symbol hierarchy)             |
| `workspace/symbol`              | Yes       | Workspace-wide symbol search                    |
| `textDocument/formatting`       | Yes       | Full document formatting                        |
| `textDocument/onTypeFormatting` | Yes       | Auto-indent on `\n`                             |
| `textDocument/codeAction`       | Yes       | Quick fixes and refactorings                    |
| `textDocument/inlayHint`        | No        | Disabled (requires type checker)                |
| `textDocument/semanticTokens`   | Yes       | Syntax highlighting via semantic tokens         |
| Project roots                   | Yes       | Multi-root workspace support (parsing only)     |
| Virtual `yarrow-std:` URIs      | Partial   | On-disk std library access only                |

## Installation

The LSP server is included in the `yarrow` CLI:

```bash
yarrow lsp
```

Or build from source:

```bash
cargo build -p yarrow_lsp
```

The binary will be available at `./target/debug/yarrow_lsp`.

## Usage

### Stdio Mode (Default)

For editors that communicate over stdin/stdout:

```bash
cargo run -p yarrow_lsp --
```

Most LSP clients (VS Code, Neovim, etc.) will automatically start the server in this mode.

### TCP Mode

For TCP-based communication:

```bash
cargo run -p yarrow_lsp -- --listen 127.0.0.1:8080
```

The server will listen on the specified address and port.

### Configuration Options

```bash
# Log level (error, warn, info, debug, trace)
cargo run -p yarrow_lsp -- --log-level debug

# TCP listen address
cargo run -p yarrow_lsp -- --listen 127.0.0.1:0

# Force virtual std module URIs (useful for packaged binaries)
YARROW_LSP_FORCE_VIRTUAL_STD=1 cargo run -p yarrow_lsp --
```

## Testing

A test harness (`harness.mjs`) is provided to validate LSP functionality:

```bash
# Run default scenario (pull-diagnostics)
node crates/yarrow-lsp/scripts/harness.mjs

# Run a specific scenario
node crates/yarrow-lsp/scripts/harness.mjs workspace-diagnostics
node crates/yarrow-lsp/scripts/harness.mjs virtual-std
node crates/yarrow-lsp/scripts/harness.mjs rapid-edits
```

Available scenarios:

| Scenario                | Description                                          |
| ----------------------- | ---------------------------------------------------- |
| `pull-diagnostics`      | Tests diagnostic retrieval for a file with errors    |
| `workspace-diagnostics` | Tests full workspace diagnostic reporting            |
| `project-roots`         | Tests multi-root project support                     |
| `project-missing-root`  | Tests error handling for missing project roots       |
| `definition-require`    | Tests go-to-definition for module imports            |
| `on-type-format`        | Tests auto-formatting on newline                     |
| `rapid-edits`           | Tests diagnostic consistency with rapid file changes |
| `virtual-std`           | Tests access to virtual standard library modules     |

## Virtual `yarrow-std:` URIs

Standard library modules are loaded from on-disk files in `lib/std/**`. By default, go-to-definition will resolve to these files when available.

The `yarrow-std:` URI scheme is no longer supported since virtual std modules are disabled without the compiler.

## Integration

### VS Code

Add to your `settings.json`:

```json
{
	"yarrow.server.path": "yarrow",
	"yarrow.server.arguments": ["lsp"]
}
```

Or use the [vscode-yarrow](https://github.com/Yarrow-Programming-Language/vscode-yarrow) extension.

### Neovim (nvim-lspconfig)

```lua
require('lspconfig').yarrow_lsp.setup({
  cmd = { 'yarrow', 'lsp' },
  filetypes = { 'yarrow' },
})
```

### Other Editors

Configure your LSP client with:

- **Command:** `yarrow lsp` or path to `yarrow_lsp` binary
- **File types:** `yarrow`, `.yar`
- **Protocol:** stdio (default) or TCP

## See Also

- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [`yarrow-core`](../yarrow-core/README.md) - Core compiler implementation
- [`yarrow-fmt`](../yarrow-fmt/README.md) - Code formatter
- [LSP Specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
