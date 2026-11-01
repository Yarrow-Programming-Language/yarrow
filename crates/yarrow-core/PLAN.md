# Yarrow Core Implementation Plan

Based on schema [SCHEMA.md](SCHEMA.md).

## Overview

- **Name:** Yarrow Core
- **Description:** Compiler library: tokenize -> parse -> check -> `{ jit | object | executable | interpret }`. `yarrow-core` is the API. Drivers (`yarrow-cli`, `yarrow-fmt`, `yarrow-lsp`) call it; they do not reimplement the pipeline.
- **Last Updated:** 2026-11-01

## Sources of Truth

- **Path:** `docs/GRAMMAR.md`
  **Type:** Language Spec
- **Path:** `docs/SYNTAX.md`
  **Type:** Formal Syntax
- **Path:** `docs/AST.md`
  **Type:** AST Definition
- **Path:** `docs/TYPE_SYSTEM.md`
  **Type:** Type System
- **Path:** `docs/MEMORY_MODEL.md`
  **Type:** Memory Model
- **Path:** `docs/RUNTIME.md`
  **Type:** Runtime and Modules
- **Path:** `docs/examples/`
  **Type:** Corpus
- **Path:** `AGENTS.md`
  **Type:** Agent Rules

**Rule:** Prefer the docs when code and docs disagree. Do not invent language features absent from the docs.

---

## Current State

### Landed Features

- **Name:** Frontend
  **Description:** Tokenizer + parser (flat postfix `Apply*`) with rustc-style diagnostics and `Comment` tokens
  **Status:** landed

- **Name:** Type Checking
  **Description:** Types, ownership, borrow, regions, unsafe checking with stack-effect notes
  **Status:** landed

- **Name:** Warnings
  **Description:** `W401`–`W410` (unused, dead stack, never-written mutable, redundant copy, require ambiguity, unreachable, empty match arm, empty if then, empty unsafe); available via `CheckedProgram::warnings`
  **Status:** landed

- **Name:** Session API
  **Description:** `check`, `compile` (JIT, explicit mode), `compile_object`, `compile_executable`, `interpret`; default `ExecutionMode::Object`; `type_at` (30), `definition_at` (35); ICE `E999` / `SessionFailureKind` (40)
  **Status:** landed

- **Name:** AOT Compilation
  **Description:** Runtime archive + Cranelift process `main` + `ld`/`lld` link (linux-gnu + static musl + Stage 39 Windows-gnu host to host); DWARF + `OptLevel`; cross object emit (linux-gnu / linux-musl + Stage 34 COFF / Mach-O)
  **Status:** landed

- **Name:** Projects
  **Description:** `ProjectOptions`, `check_project`, `ModuleGraph`; `E382` cycles detection; `E383` missing roots (`docs/guides/project_examples/`)
  **Status:** landed

- **Name:** Runtime and Standard Library
  **Description:** Host heap, regions, lists/maps/strings; `std.io`, `std.string`, `std.fs` host wrappers
  **Status:** landed

- **Name:** Interpret
  **Description:** Stage 37 gate of `docs/examples/valid/**` (stdout matches JIT): Stage 36 plus unsafe/`pointer<T>`/`move`/runes/`std.fs`; `00_grammar_tour.yar` remains out of scope (E393)
  **Status:** landed

- **Name:** Probes
  **Description:** `TypeIndex` / `type_at`; `DefIndex` / `definition_at` (bindings + requires, root-only; Stage 35)
  **Status:** landed

### Known Gaps

- **Name:** AOT Cross Link
  **Description:** Cross link needs matching archive + CRT; Mach-O executable link still needs a Darwin host / CI; Windows-gnu host to host exe is Stage 39 done
  **Status:** pending

- **Name:** Interpret Full Coverage
  **Description:** `00_grammar_tour.yar` stays out of scope (mixed surface / `loop.break` and further tour forms); Stage 37 landed unsafe / pointers / move / fs
  **Status:** pending

- **Name:** Linker Support
  **Description:** System `ld`/`lld` only; Stage 27 bundled linker deferred (discovery remains reliable)
  **Status:** deferred

- **Name:** LSP Assist
  **Description:** Typed-at-span + definition / require probes landed (Stage 35); LSP consumption is `yarrow-lsp` Stage 22
  **Status:** landed

- **Name:** Formatter Integration
  **Description:** Whitespace rebuilt by printer (`yarrow-fmt`); incomplete parse -> hygiene + selective top-level reprint; ignore regions Stage 19; parallel multi-file Stage 20; widened fmt-check corpus Stage 21
  **Status:** landed

---

## Multi-platform Compilation Status

### Platform Targets

- **Name:** linux-gnu
  **Architectures:** x86_64, aarch64
  **Status:** landed
  **Notes:** Full executable link on linux-gnu host

- **Name:** linux-musl
  **Architectures:** x86_64, aarch64
  **Status:** landed
  **Notes:** Static-friendly executable link on linux-gnu host (Stage 33)

- **Name:** windows-gnu
  **Architectures:** x86_64
  **Status:** landed
  **Notes:** Executable link on Windows-gnu host only (Stage 39); uses MinGW `ld`/`lld` + CRT (crt2.o / crtbegin.o / crtend.o)

- **Name:** apple-darwin
  **Architectures:** x86_64, aarch64
  **Status:** pending
  **Notes:** Object emit only (Stage 34); executable link needs Darwin host (backlog)

**Limitations:**

- No cross-compilation from Linux to Windows (requires native Windows-gnu host with MSYS2 UCRT64)
- No MSVC support (only Windows-gnu / MinGW toolchain)
- No macOS executable link yet (Darwin host path not wired; needs native linker + CRT discovery)
- bundled linker (Stage 27) remains deferred

**Conclusion:** Multi-platform compilation is feasible and working for the supported targets. The current approach of host-only linking with native toolchains is sound and maintainable. Cross-compilation from Linux to Windows is not feasible without significant complexity (would require PE linkers on Linux and proper CRT handling). This is an acceptable limitation, not a language design constraint.

**Reference:** CI workflow at `.github/workflows/stage-39-windows-gnu-exe.yml` demonstrates Windows-gnu host to host compilation using MSYS2 UCRT64 environment.

---

## Phases

### Phase A-E

- **Name:** A-E
- **Description:** Frontend, type checking, and basic compilation pipeline
- **Status:** landed
- **Stages:**
  - **Number:** 0-24
    **Name:** Core Pipeline
    **Description:** Tokenizer, parser, type checker, ownership/borrow/regions, unsafe checking, stack effects
    **Status:** landed
    **Gate:** Basic compilation pipeline functional

### Phase F-G

- **Name:** F-G
- **Description:** Extended features and object emission
- **Status:** landed
- **Stages:**
  - **Number:** 25-26
    **Name:** Object Emission
    **Description:** Object file generation
    **Status:** landed
    **Gate:** Object emission working
  - **Number:** 28-34
    **Name:** Cross-platform Object Support
    **Description:** COFF, Mach-O object emit, cross-compilation support
    **Status:** landed
    **Gate:** Object emit for linux-gnu, linux-musl, windows-gnu, apple-darwin

### Phase H

- **Name:** H
- **Description:** Multi-platform executable linking
- **Status:** in_progress
- **Stages:**
  - **Number:** 35-40
    **Name:** Type/Definition Probes and ICE Handling
    **Description:** `TypeIndex`/`type_at`, `DefIndex`/`definition_at`, `SessionFailureKind::Ice`
    **Status:** landed
    **Gate:** All current stage gates (35-40) are complete

---

## Next

Focus: Darwin Mach-O executable link when a native macOS host / CI agent is available (Stage 39 Windows-gnu path done). Stage 40 ICE tagging is done. Keep Stage 27 deferred unless PATH linkers become fragile. Do not invent language features. Project CLI is `yarrow-cli` Stage 13 done. LSP Stage 22 consumes Stage 35 probes. CLI Stage 16 can consume `SessionFailureKind::Ice` -> exit `101`.

All current stage gates (35-40) are complete.

---

## Backlog

### Stage 41 - Darwin Mach-O Executable Link

- **Number:** 41
- **Name:** Darwin Mach-O Executable Link
- **Description:** Implement `compile_executable_source` for `x86_64-apple-darwin` and `aarch64-apple-darwin` on native macOS host
- **Status:** pending
- **Priority:** high
- **Tasks:**
  - **Description:** Use `ld64` or `lld` for Mach-O linking (no `cc` compile step)
  - **Description:** Discover CRT / import libs explicitly (env / documented paths)
  - **Description:** Prefer host to host path first
  - **Description:** Update `docs/RUNTIME.md` Cross-compile matrix and Known gaps
- **Gate:** on a documented macOS host (or documented CI), `compile_executable_source` for that triple produces a runnable binary for `docs/examples/valid/01_hello.yar`. Existing linux-gnu / windows-gnu paths unchanged. Object-only path for Stage 34 triples still works. `cargo clippy` green.

---

### Stage 42 - Program argv API

- **Number:** 42
- **Name:** Program argv API
- **Description:** Needs GRAMMAR / std design first; CLI object path already forwards OS argv
- **Status:** pending
- **Priority:** medium
- **Tasks:**
  - **Description:** Design argv access in GRAMMAR (language-level, not host-specific)
  - **Description:** Implement in std library
  - **Description:** Wire through Session API
  - **Description:** Expose in JIT / object / interpret modes
- **Gate:** A valid Yarrow program can access command-line arguments. `cargo clippy` green.

---

### Stage 43 - Multi-root Compile and Run

- **Number:** 43
- **Name:** Multi-root Compile and Run
- **Description:** Check-only graph exists via `check_project`; now enable compile and run for multi-root projects
- **Status:** pending
- **Priority:** medium
- **Tasks:**
  - **Description:** Define multi-entry product story (which root is main? how to link multiple objects?)
  - **Description:** Implement `compile_project` / `run_project` in Session API
  - **Description:** Wire through CLI
  - **Description:** Update `docs/RUNTIME.md` Projects section
- **Gate:** `yarrow run docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar --main root_a` compiles and runs the project. `cargo clippy` green.

---

### Stage 44 - MSVC AOT Triple Support

- **Number:** 44
- **Name:** MSVC AOT Triple Support
- **Description:** After Stage 39 Windows-gnu path done. MSVC later if needed
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Add `x86_64-pc-windows-msvc` triple support
  - **Description:** Implement CRT discovery for MSVC toolchain
  - **Description:** Link via `link.exe` (no `cl.exe` compile step)
  - **Description:** Update target.rs and link.rs
- **Gate:** MSVC host to host executable link works on native Windows-MSVC host. `cargo clippy` green.

---

### Stage 45 - Bundled Linker (Revisit Stage 27)

- **Number:** 45
- **Name:** Bundled Linker
- **Description:** Only if PATH `ld`/`lld` discovery becomes fragile in practice
- **Status:** deferred
- **Priority:** low
- **Tasks:**
  - **Description:** Evaluate if system linker discovery is reliable across supported platforms
  - **Description:** If needed, bundle minimal linker (e.g., `lld`) for supported targets
  - **Description:** Fall back to system linker when available
  - **Description:** Document the approach in `docs/RUNTIME.md`
- **Gate:** Compilation works with and without system linker installed. `cargo clippy` green.

---

## Working Rules

- Prefer minimal diffs that pass the **current** stage gate.
- When renaming std or syntax, update compiler and `lib/std` together.
- Do not add tests unless explicitly asked; use `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- Do not invent lifetime syntax; ownership and regions are the model.
- `unsafe` never disables type, stack, ownership, or borrow checking.
- No CLI parsing (`clap`, argv, exit codes) here; that is `yarrow-cli`.
- May invoke a system **linker** from a library helper; must not depend on a C compiler for CRT or codegen.
