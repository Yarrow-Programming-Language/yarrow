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

## Scope

### Landed Features (v1, Stages 0-19, 21-26)

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

**Note:** Stage 20 (editor extension packaging) was **canceled**; clients live in separate repos.

### Out of Scope

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

**Transport:** stdio is the default. TCP (`--listen`) is for tests / remote clients.

---

## Architecture

```text
editor  <--stdio JSON-RPC-->  yarrow-lsp
                            -> DocumentStore (uri -> text + version)
                            -> Analysis (Session::parse_source / check_source
                                         / check_project when multi-root)
                            -> PositionMap (LSP <-> core Span / SourceFile)
                            -> Features (diag, hover, def, refs, symbols, complete,
                                         format, codeAction, signature, inlay, ...)
                            -> yarrow_core::Session
                            -> yarrow_fmt::format_source / format_range
```

### Public / Binary Surface

```rust
pub async fn run_stdio() -> Result<(), LspError>;
pub async fn run_stdio_with(config: LspConfig) -> Result<(), LspError>;
pub fn run_stdio_blocking() -> Result<(), LspError>;
pub async fn run_tcp_with(addr, config, on_listen) -> Result<(), LspError>;
pub fn run_tcp_blocking(addr, config, on_listen) -> Result<(), LspError>;
// Shared by stdio and TCP.
pub async fn run_with_streams(/* ... */) -> Result<(), LspError>;
```

**Protocol Stack:**

- LSP types: via `tower-lsp-server` (community fork of `tower-lsp`)
- Runtime: `tokio`
- Binary: `crates/yarrow-lsp` + `yarrow lsp`

**Rule:** Do **not** shell out to `yarrow check`; call `Session` in-process.

### Position Mapping

LSP positions are UTF-16 code units by default (or UTF-8 if negotiated). Core `Span` / `SourceFile::location` use **byte offsets** and Unicode scalar columns.

1. `PositionMap` converts `Span` <-> `lsp::Range`
2. Prefer negotiating `positionEncoding = utf-8` when the client supports it; still support UTF-16 for VS Code-class clients.

### Analysis Model

On `didOpen` / `didChange` (debounced):

1. Update buffer text
2. Build `CompileOptions` with `source_path` from URI, search paths from init options / workspace folders
3. Call `check_source` (or `check_project` when Stage 21 multi-root mode applies)
4. Map `DiagnosticBatch` -> `PublishDiagnosticsParams` (and answer pull requests)
5. Cache last successful parse / check artifact for hover, navigation, inlays, tokens

Open documents + transitive `require` resolution cover the single-file case. Multi-root project check is Stage 21 (core graph already exists).

---

## Current State

All v1 LSP stages (0-19, 21-26) are complete. Stage 20 (editor extensions) was canceled.

### Landed Dependencies

- **Crate:** yarrow-lsp
  **Features:** Stages 0-19 + 21-26 landed

- **Crate:** yarrow-core
  **Features:** Session API: `parse_source` / `check_source` / `check_project` + spans

- **Crate:** yarrow-core
  **Features:** Core diagnostics: `Diagnostic` / `Severity` / codes / explain table

- **Crate:** yarrow-core
  **Features:** Typed hover data: `CheckedProgram::type_at` (core Stage 30)

- **Crate:** yarrow-core
  **Features:** Def / require probe: Core Stage 35 + LSP Stage 22 consume `definition_at`

- **Crate:** yarrow-core
  **Features:** Cross-file resolve: Probe + AST / `require`; project multi-root in Stage 21

- **Crate:** yarrow-fmt
  **Features:** Full-doc best-effort + `format_range`; on-type is local indent

- **Crate:** yarrow-cli
  **Features:** CLI `yarrow lsp`: In-process `run_stdio_blocking` / `--listen`

---

## Feature Mapping

LSP capability to stages:

| Capability                         | Stage                                                                 |
| ---------------------------------- | --------------------------------------------------------------------- |
| initialize / shutdown              | Stage 0                                                               |
| textDocument sync                  | Stage 1                                                               |
| publishDiagnostics                 | Stage 2; cancelation Stage 25                                         |
| documentSymbol                     | Stage 3                                                               |
| definition                         | Stage 4, 7, 22, 26 (probes; virtual std fallback)                     |
| hover                              | Stage 5, 9, 22, 26 (AST; type_at; def/require; virtual std)           |
| completion                         | Stage 6 (grammar keywords + AST names)                                |
| references                         | Stage 7 (binding / name index)                                        |
| formatting                         | Stage 8 (`yarrow-fmt`)                                                |
| codeAction / explain               | Stage 11 (`explain_code`)                                             |
| signatureHelp                      | Stage 12 (AST + type_at)                                              |
| inlayHint                          | Stage 13 (TypeIndex probes)                                           |
| semanticTokens                     | Stage 14 (tokens + AST)                                               |
| rename                             | Stage 15 (references / resolve; file-local)                           |
| workspaceSymbol                    | Stage 16; Stage 21 (open + requires; project roots via check_project) |
| rangeFormatting / onTypeFormatting | Stage 17; Stage 23 (format_range; on-type = local indent after end)   |
| textDocument/diagnostic (pull)     | Stage 18 (same as publish + uri/version cache)                        |
| workspace/diagnostic               | Stage 24 (open / project roots only)                                  |
| TCP + test harness                 | Stage 19 (transport only)                                             |
| multi-root project check           | Stage 21 (check_project; core Stage 28)                               |
| analysis cancelation / latency     | Stage 25 (gen tickets + spawn_blocking)                               |
| virtual yarrow-std URIs            | Stage 26 (embedded std when on-disk missing)                          |
| editor extensions                  | Stage 20 (canceled; separate repos)                                   |

---

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

- **Name:** Cross-module Silent Rename
  **Description:** Explicitly refused; Stage 15 stays conservative
  **Status:** out_of_scope

- **Name:** Salsa / Full Incremental IR
  **Description:** Only if Stage 25 still too slow
  **Status:** pending
  **Priority:** low

- **Name:** DAP / Debug Adapter
  **Description:** Separate product
  **Status:** out_of_scope

- **Name:** Markdown / Embedded Yarrow
  **Description:** Skip until requested
  **Status:** out_of_scope

- **Name:** Editor Extensions
  **Description:** Separate repos (Stage 20 canceled here)
  **Status:** out_of_scope

- **Name:** Call / Type Hierarchy
  **Description:** Needs richer core index; do not fake from names alone
  **Status:** pending
  **Priority:** low

---

## Working Rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use scripted LSP messages + `docs/examples/**` as gates (prefer `scripts/harness.mjs` from Stage 19).
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history). When a whole phase is done, collapse finished stages into **Landed** the same way Stages 0-19 were.
- No tokenizer / parser / typechecker logic here beyond calling `yarrow-core`.
- Format only through `yarrow-fmt`, never a second pretty-printer.
- In comments and documentation, never use em dash; use ASCII hyphen or rephrase.
- If core needs position helpers, richer probes, or require-path APIs, land them in `yarrow-core` and note the dependency here and in the core plan Known gaps / Next.
- Keep the safe vs unsafe boundary visible in hovers / inlays when relevant; do not imply `unsafe` turns off checking.
