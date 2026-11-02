# Yarrow documentation

Language reference for Yarrow: a typed, stack-based language with ownership, borrowing, regions, and an explicit `unsafe` boundary.

Start with the [grammar tour](GRAMMAR.md) for a readable walkthrough, or jump to a topic below. Implementation status and gates live in `crates/**/PLAN.md`, not here.

## Language

- [Grammar](GRAMMAR.md) - illustrative program that introduces the language by example
- [Syntax](SYNTAX.md) - formal EBNF derived from the grammar
- [AST](AST.md) - intended abstract syntax (expressions, statements, types, patterns)
- [Type system](TYPE_SYSTEM.md) - primitives, coercions, conversions, checking
- [Memory model](MEMORY_MODEL.md) - ownership, borrowing, regions, unsafe
- [Runtime](RUNTIME.md) - stack, host runtime, errors, modules and `require`
- [Style guide](STYLE_GUIDE.md) - layout and idioms for formatters and human code

## Examples

Focused `.yar` programs under [`examples/`](examples/README.md):

- `valid/` - idiomatic, well-formed programs
- `invalid/` - cases a conforming checker should reject (see `# ERROR:` comments)

Prefer the grammar when an example and the unfinished compiler disagree.

## Suggested reading order

1. [Grammar](GRAMMAR.md) or [examples/valid/01_hello.yar](examples/valid/01_hello.yar)
2. [Syntax](SYNTAX.md) when you need exact forms
3. [Type system](TYPE_SYSTEM.md) and [Memory model](MEMORY_MODEL.md) for checking rules
4. [Runtime](RUNTIME.md) for modules, host ABI, and execution shape
5. [Style guide](STYLE_GUIDE.md) before writing or formatting larger programs
