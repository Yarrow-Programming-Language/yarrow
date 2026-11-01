# yarrow-fmt

Code formatter for the [Yarrow](https://github.com/Yarrow-Programming-Language/yarrow) programming language.

## Overview

`yarrow-fmt` provides **opinionated, automatic formatting** for Yarrow source code. It parses the source, applies formatting rules based on the language's [style guide](../../docs/STYLE_GUIDE.md), and outputs consistently formatted code.

## Features

- **Consistent indentation** - Tab-based indentation (configurable)
- **Automatic line breaking** - Intelligent line wrapping
- **Expression alignment** - Vertical alignment of related constructs
- **Comment preservation** - Maintains existing comments
- **Import ordering** - Sorts and groups imports
- **Whitespace normalization** - Removes unnecessary whitespace
- **On-type formatting** - Auto-formatting on specific characters (e.g., `\n`)

## Installation

Build the formatter:

```bash
cargo build -p yarrow_fmt
```

The binary will be available at `./target/debug/yarrow-fmt`.

Or install to your system:

```bash
cargo install --path crates/yarrow-fmt
```

## Usage

### Command Line

```bash
yarrow-fmt [OPTIONS] [FILES...]
```

#### Options

| Option            | Description                                       |
| ----------------- | ------------------------------------------------- |
| `--check`         | Check if files are formatted (exit code 1 if not) |
| `--write`         | Write formatted output back to files (default)    |
| `--stdin`         | Read from stdin                                   |
| `--config <FILE>` | Use a specific configuration file                 |
| `-v`, `--verbose` | Verbose output                                    |
| `-h`, `--help`    | Show help message                                 |

#### Examples

```bash
# Format a single file
yarrow-fmt my_file.yar

# Format multiple files
yarrow-fmt src/*.yar

# Check formatting without modifying files
yarrow-fmt --check src/

# Format from stdin
echo "main function 1 add print end" | yarrow-fmt --stdin

# Format and write to stdout (use with --stdin)
echo "..." | yarrow-fmt --stdin -
```

### As a Library

```rust
use yarrow_fmt::{format_code, FormatConfig, FormatResult};

let source = "main function 1 add print end";
let config = FormatConfig::default();

match format_code(source, &config) {
    FormatResult { formatted, diagnostics } => {
        println!("{}", formatted);
        // Handle any formatting diagnostics
    }
}
```

## Formatting Rules

### Indentation

- **Default:** Tab characters (`\t`)
- **Configurable:** Can be set to spaces
- **Width:** Typically 4 characters per indent level

### Line Breaking

- **Maximum line length:** 100 characters (configurable)
- **Break at operators:** Binary operators are placed at line start
- **Break before keywords:** `do`, `then`, `else`, etc. get their own line

### Expression Formatting

```yarrow
# Before (unformatted)
main function a b add c d mul add print end

# After (formatted)
main function
a b add
c d mul add
print
end
```

### Function Formatting

```yarrow
# Stack-based functions
main function
    # indented body
    1 2 add
    print
end

# With type signature
foo a: Int b: Int -> Int function
    a b add
end
```

### Control Structures

```yarrow
# if-then-else
if condition then
    # then branch
    do_something
else
    # else branch
    do_other_thing
end

# while loop
while condition do
    loop_body
end

# case/match
value case
| Pattern1 -> handle1
| Pattern2 -> handle2
end
```

### Module Formatting

```yarrow
"path.to.module" [scope] require

# Local definitions
my_const 42 :=

my_function do
    # implementation
end
```

### Comments

```yarrow
// Single-line comment (preserved on own line)

/*
 * Multi-line comment
 * (preserved as-is)
 */

main function
    // Inline comment (preserved after code)
    1 print
end
```

## Configuration

Create a `yarrow-fmt.toml` or `.yarrow-fmt.toml` file:

```toml
[format]
# Indentation settings
indent_style = "tab"      # "tab" or "space"
indent_width = 4          # Width of one indent level
tab_width = 4             # Tab width for display

# Line wrapping
max_width = 100           # Maximum line length
wrap_operators = true     # Break lines at operators

# Whitespace
spaces_around_operators = true
spaces_after_commas = true

# Import ordering
sort_imports = true       # Sort imports alphabetically
group_imports = true      # Group imports by source
```

## Project Structure

```
crates/yarrow-fmt/
├── src/
│   ├── lib.rs              # Library entry point
│   ├── main.rs             # CLI entry point
│   ├── driver.rs           # Formatting driver
│   ├── config.rs           # Configuration parsing
│   ├── format.rs           # Main formatting logic
│   ├── ir.rs               # Intermediate representation for formatting
│   ├── range.rs            # Text range management
│   ├── layout.rs           # Layout algorithms
│   ├── indent.rs           # Indentation handling
│   ├── blank.rs            # Blank line handling
│   ├── comment.rs          # Comment preservation
│   ├── phrase.rs           # Phrase-level formatting
│   ├── print.rs            # Output printing
│   ├── best_effort.rs      # Best-effort formatting for invalid code
│   ├── ignore.rs           # Ignore directives
│   └── paths.rs            # File path handling
├── Cargo.toml
└── PLAN.md
```

## Formatting Phases

1. **Parsing** - Parse the source code into an AST
2. **Normalization** - Normalize the AST for formatting
3. **Layout** - Determine the visual layout
4. **Printing** - Generate the formatted source

## Integration

### Editor Integration

Most editors can integrate `yarrow-fmt` for on-save formatting:

**VS Code:**

```json
{
	"editor.formatOnSave": true,
	"[yarrow]": {
		"editor.defaultFormatter": "yarrow.fmt"
	}
}
```

**Neovim:**

```lua
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = "*.yar",
  callback = function()
    vim.lsp.buf.format({ async = false })
  end
})
```

### LSP Integration

`yarrow-fmt` is integrated with [`yarrow-lsp`](../yarrow-lsp) for:

- **Document formatting** (`textDocument/formatting`)
- **Range formatting** (`textDocument/rangeFormatting`)
- **On-type formatting** (`textDocument/onTypeFormatting`)

## Dependencies

### Internal

- [`yarrow-core`](../yarrow-core/README.md) - Parser and AST

### External

- `clap` - Command-line argument parsing
- `rayon` - Parallel formatting (for workspace formatting)

## See Also

- [Yarrow Style Guide](../../docs/STYLE_GUIDE.md)
- [Yarrow Language Specification](../../docs/GRAMMAR.md)
- [`yarrow-cli`](../yarrow-cli/README.md) - Command-line interface
- [`yarrow-core`](../yarrow-core/README.md) - Core compiler
- [`yarrow-lsp`](../yarrow-lsp/README.md) - Language Server Protocol
