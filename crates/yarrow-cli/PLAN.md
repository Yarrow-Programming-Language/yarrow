# Yarrow CLI Implementation Plan

User-facing compiler driver. Talks to **`yarrow-core` only**. Owns clap, stdout/stderr, color, and process exit codes.

Root binary `src/main.rs` stays a thin `yarrow_cli::run(args) -> ExitCode` wrapper.

Language and compiler work: `crates/yarrow-core/PLAN.md`. Formatter / LSP stay separate crates; CLI wires them as thin in-process subcommands (`fmt`, `lsp`).

## Source of truth

- Compiler API: `crates/yarrow-core/PLAN.md`
- Language docs: `docs/GRAMMAR.md`
- Corpus: `docs/examples/`
- Agent rules: `AGENTS.md`

Do not reimplement checking or codegen here. If a command needs a missing core feature, add it to `yarrow-core` first (or mark the command blocked).

---

## Status

### Landed

Architecture:
```text
src/main.rs  ->  yarrow_cli::run
                  -> clap (commands + global flags)
                  -> yarrow_core::Session
```

Commands:
- `run`: `--target object` (default, link + exec) or `jit`; `--main`; args after `--`
- `compile`: Codegen only; default `./<stem>.o` / `./<stem>`; `-o` overrides; records `.yarrow-build/artifacts`
- `check`: Semantic check; one file -> `check_source`; two+ roots -> `check_project`; `--corpus DIR` -> non-recursive `*.yar` batch
- `interpret`: Stack VM via `interpret_source`; `--main`; args after `--` (rejected until core argv)
- `repl`: Line-oriented `EvalContext` loop; wraps snippets as `main`; EOF/`exit`/`quit`
- `lsp`: Language server stdio / `--listen` (`yarrow_lsp`)
- `fmt`: Format `.yar` in-process via `yarrow_fmt::run_fmt`
- `dump`: `--emit tokens|ast|ir`
- `explain`: Long form for a diagnostic code
- `clean`: Delete only paths listed in `.yarrow-build/artifacts` (idempotent; no `*.o` glob)
- `version`: Crate version (`-V` too)

**Defaults:** `yarrow <file.yar>` -> `run --target object` (matches `CompileOptions` / `ExecutionMode::Object`). Entry name `main` unless `--main` is set. Use `--target jit` for in-process run.

**Global flags:**
- `--color` (`auto` honors `NO_COLOR` / `CLICOLOR_FORCE` / `FORCE_COLOR`; explicit wins)
- `--error-limit`
- `-L` / `--search-path`
- `-q` (suppress driver chatter, never diagnostics / explain / dump payload)
- `-v` (stderr progress; no-op under `-q`)

**Exit codes:**
- `0` ok
- `1` program diagnostics (incl. link)
- `2` usage / I/O / signal
- `101` internal compiler error (caught panic or `E999` / `SessionFailureKind::Ice`)

Native `run --target object` propagates the child exit status when in `0..=255`.

Stages 1-17 are complete. Historical stage write-ups were removed; git history keeps them.

---

## Targets (reference)

- `jit`: JIT + execute entry (run) or JIT lower only (compile)
- `object`: Link executable + exec (run) or write relocatable `.o` or linked `exe` via `--emit` (compile)

`interpret` is not a `--target`.

---

## Backlog (new tasks from backlog)

### Stage 18 - JIT and interpret argv

Wire when core exposes language argv; Stage 10 already forwards OS argv.

1. Wait for core argv API (see `yarrow-core` Stage 42)
2. Wire argv through Session to JIT execution
3. Wire argv through Session to interpret execution
4. Test with simple programs that use arguments

**Gate:** Programs using command-line arguments work with both `run --target jit` and `interpret`. `cargo clippy` green.

---

### Stage 19 - Multi-root compile and run

After Stage 13 check-only; needs a defined multi-entry product story.

1. Wait for core `compile_project` / `run_project` API (see `yarrow-core` Stage 43)
2. Add `yarrow compile FILE [FILE...]` for multi-root projects
3. Add `yarrow run FILE [FILE...]` for multi-root projects
4. Handle `--main` selection across multiple roots

**Gate:** `yarrow run docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar --main root_a` compiles and runs the project. `cargo clippy` green.

---

### Stage 20 - Language-level tests

After Stage 17 corpus driver; needs GRAMMAR / core, not CLI invention.

1. Design language-level test framework in GRAMMAR (if not already done)
2. Implement test discovery and execution in core
3. Add CLI command `yarrow test`
4. Support test filtering and reporting

**Gate:** `yarrow test` discovers and runs tests in a project. `cargo clippy` green.

---

### Stage 21 - LSP project workspace

Depends on Stage 13 UX + core graph; see `yarrow-lsp` Stage 21.

1. Coordinate with `yarrow-lsp` on project workspace support
2. Ensure CLI passes necessary project information to LSP
3. Test multi-root project workflow with LSP

**Gate:** LSP project workspace works correctly with multi-root projects. `cargo clippy` green.

---

## Working rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- No tokenizer/parser/compiler logic beyond calling `yarrow-core`.
- Do not invent language features or diagnostic codes in the CLI.
