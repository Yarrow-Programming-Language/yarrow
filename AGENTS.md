# AGENTS.md

Yarrow is a typed, stack-based programming language with ownership, borrow checking, regions, and an explicit `unsafe` boundary. Implemented in Rust; Cranelift JIT, object emit, and host executable link.

## Role

Build and maintain the Yarrow programming language implementation, following the language specification and project roadmap.

## Goals

- Follow the language spec in `docs/GRAMMAR.md`.
- Implement features according to the roadmap in `crates/**/PLAN.md`.
- Update `PLAN.md` status and gates when a stage is finished.

## Constraints

- Do not write tests unless the user explicitly asks. Tests come later.
- Treat `docs/GRAMMAR.md` as authoritative.
- When spec and code diverge, follow the plan's tasks to close the gap.
- Keep the safe vs unsafe boundary strict:
  - Safe: ownership, borrow, regions, stack effects, types.
  - `unsafe function` / `unsafe ... end` only allow explicitly unsafe operations (raw pointers, etc.). They do not disable borrow or ownership checking.
- Do not invent lifetime syntax (`reference<'a, T>`). Ownership and regions are the model.
- Module imports use the form `"path" [scope] require` (keyword last).
- In comments and documentation, never use `—`.

## Preferences

- Prefer minimal, focused changes that pass the relevant PLAN gate.
- Prefer clear prose, short lists, and linked tables of contents over ASCII tree or box-drawing diagrams. Use a diagram only when it is the simplest accurate explanation.
- Even if a prompt is written in a certain language, generated code, comments, and documentation must be in English; only the agent's response text should be in the prompt's language.
- Avoid using tables unless they are the best way to represent the information; keep information simple to read and understand when possible.

## Workflow

Run the following commands to verify changes:

```bash
cargo fmt --all
cargo check
cargo clippy
cargo run -- <file.yar>
```

## Resources

### Project layout

- `crates/yarrow-core`: Core compiler crate. Contains tokenizer, parser, compiler, and session (Cranelift JIT, object emit, interpret).
- `crates/yarrow-runtime`: Host heap and `HOST_FNS` (rlib).
- `crates/yarrow-runtime/aot`: AOT staticlib for ahead-of-time compilation.
- `crates/yarrow-cli`: CLI driver (Clap). Modules resolve relative to the source file.
- `crates/yarrow-fmt`: Code formatter and style guide.
- `crates/yarrow-lsp`: Language server implementation.
- `src/main.rs`: Project entry point, linked to the CLI.
- `docs/`: Documentation (grammar, syntax, memory model, type system, style guide).

### Source of truth

- Language spec: `docs/GRAMMAR.md`
- Implementation roadmap + gates: `crates/**/PLAN.md`
