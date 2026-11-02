# AGENTS.md

Yarrow is a typed, stack-based programming language with ownership, borrow checking, regions, and an explicit `unsafe` boundary. Implemented in Rust; interpreter-only.

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
- In general, never use em dashs `—`.

## Preferences

- Prefer clear prose, short lists, and linked tables of contents over ASCII tree or box-drawing diagrams. Use a diagram only when it is the simplest accurate explanation.
- Even if a prompt is written in a certain language, generated code, comments, and documentation must be in English; only the agent's response text should be in the prompt's language.
- Avoid using tables unless they are the best way to represent the information; keep information simple to read and understand when possible.
- When a change request can be generalized as a rule, prefer adding it to the relevant .agents/skills/ SKILL.md rather than appending to AGENTS.md. AGENTS.md should stay small and universal; domain-specific conventions belong in skills so they load only when needed. Create a new skill when no existing one fits.

## Workflow

Run the following commands to verify changes:

```bash
cargo fmt --all # To format the file
cargo check # First check
cargo clippy # Second check
cargo run -- <file.yar> # Run the file to test it
```

## Resources

### Project layout

- `crates/yarrow-core`: Core interpreter crate. Contains tokenizer, parser, and session (interpret).
- `crates/yarrow-runtime`: Host heap and `HOST_FNS` (rlib).
- `crates/yarrow-cli`: CLI driver (Clap). Modules resolve relative to the source file.
- `crates/yarrow-fmt`: Code formatter and style guide.
- `crates/yarrow-lsp`: Language server implementation.
- `src/main.rs`: Project entry point, linked to the CLI.
- `docs/`: Documentation (grammar, syntax, memory model, type system, style guide).

### Source of truth

- Language spec: `docs/GRAMMAR.md`
- Implementation roadmap + gates: `crates/**/PLAN.md`
