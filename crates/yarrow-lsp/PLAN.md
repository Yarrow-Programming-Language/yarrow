# Yarrow LSP Implementation Plan

Based on schema [SCHEMA.md](SCHEMA.md).

## Overview

- **Name:** Yarrow LSP
- **Description:** Language server for `.yar` editors. Talks to **`yarrow-core`** for tokenize / parse / check / diagnostics, and **`yarrow-fmt`** for document formatting. Owns LSP protocol, document sync, and editor UX. Does **not** reimplement the compiler.
- **Last Updated:** 2026-11-01

## Sources of Truth

- **Path:** `docs/GRAMMAR.md`
  **Type:** Language Syntax
- **Path:** `docs/SYNTAX.md`
  **Type:** Formal Syntax
- **Path:** `docs/AST.md`
  **Type:** AST Definition
- **Path:** `docs/TYPE_SYSTEM.md`
  **Type:** Type System
- **Path:** `docs/RUNTIME.md`
  **Type:** Runtime and Modules
- **Path:** `docs/STYLE_GUIDE.md`
  **Type:** Style (format) via `yarrow-fmt`
- **Path:** `crates/yarrow-core/PLAN.md`
  **Type:** Compiler API
- **Path:** `crates/yarrow-fmt/PLAN.md`
  **Type:** Formatter API
- **Path:** `docs/examples/`
  **Type:** Corpus (smoke)
- **Path:** `AGENTS.md`
  **Type:** Agent Rules

**Rule:** Prefer core diagnostics and spans over inventing LSP-only error messages. When protocol and language disagree, follow the language docs and map carefully into LSP.

---

## Current State

### Landed Features

- **Name:** Protocol Support
  **Description:** stdio Language Server Protocol (LSP 3.17-shaped) + TCP `--listen host:port` transport (one client) + in-repo protocol harness
  **Status:** landed

- **Name:** Text Document Sync
  **Description:** Text document sync for `file://` `.yar` buffers
  **Status:** landed

- **Name:** Diagnostics
  **Description:** Publish diagnostics from `Session::check_source` (and parse failures) with optional multi-root `Session::check_project` via init `projectRoots`
  **Status:** landed

- **Name:** Pull Diagnostics
  **Description:** Pull diagnostics (`textDocument/diagnostic`) with uri+version cache shared with push; workspace pull diagnostics (`workspace/diagnostic`) for open buffers + project roots
  **Status:** landed

- **Name:** Navigation
  **Description:** Go-to-definition, find references (same file + `require` cross-file)
  **Status:** landed

- **Name:** Hover
  **Description:** Hover (AST + typed via `CheckedProgram::type_at`)
  **Status:** landed

- **Name:** Document Symbols
  **Description:** Document symbols
  **Status:** landed

- **Name:** Completions
  **Description:** Completions: keywords + in-scope / imported names + `std.*` require paths
  **Status:** landed

- **Name:** Document Formatting
  **Description:** Document formatting via `yarrow-fmt` (full document + range + safe on-type after `end`)
  **Status:** landed

- **Name:** Code Actions
  **Description:** Code actions / hover that surface `explain_code` for diagnostic codes
  **Status:** landed

- **Name:** Configuration
  **Description:** `LspConfig`, init options, `yarrow lsp` CLI wrapper
  **Status:** landed

- **Name:** Signature Help
  **Description:** Signature help at postfix `name call` sites
  **Status:** landed

- **Name:** Inlay Hints
  **Description:** Inlay hints from core type probes (`--no-inlay` / init `inlayHints`)
  **Status:** landed

- **Name:** Semantic Tokens
  **Description:** Semantic tokens (full document) from tokenizer + AST decls
  **Status:** landed

- **Name:** Rename
  **Description:** File-local rename (`prepareRename` + `rename`; refuse unsafe cross-module edits)
  **Status:** landed

- **Name:** Workspace Symbols
  **Description:** Workspace symbols (`workspace/symbol` over open buffers + resolved requires)
  **Status:** landed

- **Name:** Core Probes
  **Description:** Core `definition_at` probes for goto / hover (Stage 22; AST fallback on miss)
  **Status:** landed

- **Name:** On-type Formatting
  **Description:** On-type formatting: newline after `end` aligns indent only (Stage 23)
  **Status:** landed

- **Name:** Analysis Cancelation
  **Description:** Analysis cancelation: version + generation tickets; checks on `spawn_blocking` (Stage 25)
  **Status:** landed

- **Name:** Virtual URIs
  **Description:** Virtual `yarrow-std:` URIs when on-disk `lib/std` is missing (Stage 26)
  **Status:** landed

### Known Gaps

- **Name:** Package Manager
  **Description:** Language has no project file; roots are explicit paths only
  **Status:** out_of_scope

- **Name:** Cross-module Rename
  **Description:** Silent semantic rename across modules: Stage 15 stays conservative; never guess
  **Status:** out_of_scope

- **Name:** Debug Adapter Protocol
  **Description:** Separate product; AOT/JIT debug is not an LSP feature
  **Status:** out_of_scope

- **Name:** Snippet/AI Rewrite
  **Description:** Not mechanical language support
  **Status:** out_of_scope

- **Name:** Embedded Yarrow
  **Description:** Non-`.yar` / markdown embedded: Skip until requested
  **Status:** out_of_scope

- **Name:** Editor Extensions
  **Description:** Editor extensions (VS Code / Zed / ...): Live in separate repos later; not in the main compiler tree
  **Status:** out_of_scope

## Phases

### Phase v1

- **Name:** v1 LSP
- **Description:** Core LSP functionality implementation
- **Status:** landed
- **Stages:**
  - **Number:** 0-19
    **Name:** Core Features
    **Description:** All core LSP features including diagnostics, navigation, formatting
    **Status:** landed
    **Gate:** All v1 features complete

- **Name:** v1 Extended
- **Description:** Extended features and multi-root support
- **Status:** landed
- **Stages:**
  - **Number:** 21-26
    **Name:** Extended Features
    **Description:** Multi-root project check, analysis cancelation, virtual URIs
    **Status:** landed
    **Gate:** All extended features complete

---

## Backlog

### Stage 27 - Salsa / Full Incremental IR

- **Number:** 27
- **Name:** Salsa / Full Incremental IR
- **Description:** Only if Stage 25 still too slow
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Profile analysis performance on large codebases
  - **Description:** Evaluate if Salsa or similar incremental compilation is needed
  - **Description:** If needed, design and implement incremental IR
  - **Description:** Ensure correctness (output must be identical)
- **Gate:** Performance evaluation complete; if implemented, output unchanged. `cargo clippy` green.

### Stage 28 - Call / Type Hierarchy

- **Number:** 28
- **Name:** Call / Type Hierarchy
- **Description:** Needs richer core index; do not fake from names alone
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Coordinate with `yarrow-core` on richer index needs
  - **Description:** Implement call hierarchy based on core index
  - **Description:** Implement type hierarchy based on core index
  - **Description:** Ensure it does not fake from names alone
- **Gate:** Call and type hierarchy work correctly with core index. `cargo clippy` green.

## Working Rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use scripted LSP messages + `docs/examples/**` as gates (prefer `scripts/harness.mjs` from Stage 19).
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history). When a whole phase is done, collapse finished stages into **Landed** the same way Stages 0-19 were.
- No tokenizer / parser / typechecker logic here beyond calling `yarrow-core`.
- Format only through `yarrow-fmt`, never a second pretty-printer.
- In comments and documentation, never use em dash; use ASCII hyphen or rephrase.
- If core needs position helpers, richer probes, or require-path APIs, land them in `yarrow-core` and note the dependency here and in the core plan Known gaps / Next.
- Keep the safe vs unsafe boundary visible in hovers / inlays when relevant; do not imply `unsafe` turns off checking.
