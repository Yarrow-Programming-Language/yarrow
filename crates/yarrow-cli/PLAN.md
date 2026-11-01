# Yarrow CLI Implementation Plan

Based on schema [SCHEMA.md](SCHEMA.md).

## Overview

- **Name:** Yarrow CLI
- **Description:** User-facing compiler driver. Talks to **`yarrow-core` only**. Owns clap, stdout/stderr, color, and process exit codes.
- **Last Updated:** 2026-11-01

## Sources of Truth

- **Path:** `crates/yarrow-core/PLAN.md`
  **Type:** Compiler API
- **Path:** `docs/GRAMMAR.md`
  **Type:** Language Spec
- **Path:** `docs/examples/`
  **Type:** Corpus
- **Path:** `AGENTS.md`
  **Type:** Agent Rules

**Rule:** Do not reimplement checking or codegen here. If a command needs a missing core feature, add it to `yarrow-core` first (or mark the command blocked).

---

## Current State

### Landed Features

- **Name:** Architecture
  **Description:** Thin wrapper structure: `src/main.rs` -> `yarrow_cli::run` -> clap (commands + global flags) -> `yarrow_core::Session`
  **Status:** landed

- **Name:** Commands
  **Description:**
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
    **Status:** landed

- **Name:** Defaults
  **Description:** `yarrow <file.yar>` -> `run --target object` (matches `CompileOptions` / `ExecutionMode::Object`). Entry name `main` unless `--main` is set. Use `--target jit` for in-process run.
  **Status:** landed

- **Name:** Global Flags
  **Description:**
  - `--color` (`auto` honors `NO_COLOR` / `CLICOLOR_FORCE` / `FORCE_COLOR`; explicit wins)
  - `--error-limit`
  - `-L` / `--search-path`
  - `-q` (suppress driver chatter, never diagnostics / explain / dump payload)
  - `-v` (stderr progress; no-op under `-q`)
    **Status:** landed

- **Name:** Exit Codes
  **Description:**
  - `0` ok
  - `1` program diagnostics (incl. link)
  - `2` usage / I/O / signal
  - `101` internal compiler error (caught panic or `E999` / `SessionFailureKind::Ice`)
    **Status:** landed
    **Notes:** Native `run --target object` propagates the child exit status when in `0..=255`

### Targets

- **jit:** JIT + execute entry (run) or JIT lower only (compile)
- **object:** Link executable + exec (run) or write relocatable `.o` or linked `exe` via `--emit` (compile)

**Note:** `interpret` is not a `--target`

---

## Phases

### Phase 1-17

- **Name:** Core CLI Development
- **Description:** Initial CLI implementation with all major commands and features
- **Status:** landed
- **Stages:**
  - **Number:** 1-17
    **Name:** Command Implementation
    **Description:** Implementation of all CLI commands and their integration with yarrow-core
    **Status:** landed
    **Gate:** All commands functional, Stages 1-17 complete

---

## Backlog

### Stage 18 - JIT and Interpret argv

- **Number:** 18
- **Name:** JIT and Interpret argv
- **Description:** Wire when core exposes language argv; Stage 10 already forwards OS argv
- **Status:** pending
- **Priority:** medium
- **Dependencies:**
  - **Crate:** yarrow-core
    **Stage:** 42
    **Description:** Core argv API
- **Tasks:**
  - **Description:** Wait for core argv API (see `yarrow-core` Stage 42)
  - **Description:** Wire argv through Session to JIT execution
  - **Description:** Wire argv through Session to interpret execution
  - **Description:** Test with simple programs that use arguments
- **Gate:** Programs using command-line arguments work with both `run --target jit` and `interpret`. `cargo clippy` green.

---

### Stage 19 - Multi-root Compile and Run

- **Number:** 19
- **Name:** Multi-root Compile and Run
- **Description:** After Stage 13 check-only; needs a defined multi-entry product story
- **Status:** pending
- **Priority:** medium
- **Dependencies:**
  - **Crate:** yarrow-core
    **Stage:** 43
    **Description:** Core compile_project / run_project API
- **Tasks:**
  - **Description:** Wait for core `compile_project` / `run_project` API (see `yarrow-core` Stage 43)
  - **Description:** Add `yarrow compile FILE [FILE...]` for multi-root projects
  - **Description:** Add `yarrow run FILE [FILE...]` for multi-root projects
  - **Description:** Handle `--main` selection across multiple roots
- **Gate:** `yarrow run docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar --main root_a` compiles and runs the project. `cargo clippy` green.

---

### Stage 20 - Language-level Tests

- **Number:** 20
- **Name:** Language-level Tests
- **Description:** After Stage 17 corpus driver; needs GRAMMAR / core, not CLI invention
- **Status:** pending
- **Priority:** medium
- **Tasks:**
  - **Description:** Design language-level test framework in GRAMMAR (if not already done)
  - **Description:** Implement test discovery and execution in core
  - **Description:** Add CLI command `yarrow test`
  - **Description:** Support test filtering and reporting
- **Gate:** `yarrow test` discovers and runs tests in a project. `cargo clippy` green.

---

### Stage 21 - LSP Project Workspace

- **Number:** 21
- **Name:** LSP Project Workspace
- **Description:** Depends on Stage 13 UX + core graph; see `yarrow-lsp` Stage 21
- **Status:** pending
- **Priority:** medium
- **Dependencies:**
  - **Crate:** yarrow-lsp
    **Stage:** 21
    **Description:** LSP project workspace support
- **Tasks:**
  - **Description:** Coordinate with `yarrow-lsp` on project workspace support
  - **Description:** Ensure CLI passes necessary project information to LSP
  - **Description:** Test multi-root project workflow with LSP
- **Gate:** LSP project workspace works correctly with multi-root projects. `cargo clippy` green.

---

## Working Rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- No tokenizer/parser/compiler logic beyond calling `yarrow-core`.
- Do not invent language features or diagnostic codes in the CLI.
