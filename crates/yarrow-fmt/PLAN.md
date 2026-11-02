# Yarrow Formatter Implementation Plan

Based on schema [SCHEMA.md](SCHEMA.md).

## Overview

- **Name:** Yarrow Formatter
- **Description:** Library and binary that rewrite `.yar` source to match `docs/STYLE_GUIDE.md`. `yarrow-fmt` is a tooling crate. It may depend on `yarrow-core` for tokenize / parse / diagnostics. It does **not** type-check, borrow-check, or codegen. CLI wiring (`yarrow fmt`) lives in `crates/yarrow-cli/PLAN.md`. Editors format via `yarrow-lsp` calling the same `format_source` API.
- **Last Updated:** 2026-11-01

## Sources of Truth

- **Path:** `docs/STYLE_GUIDE.md`
  **Type:** Layout / Idiomatic Form
- **Path:** `docs/GRAMMAR.md`
  **Type:** Language Syntax
- **Path:** `docs/SYNTAX.md`
  **Type:** Formal Syntax
- **Path:** `docs/AST.md`
  **Type:** Intended AST
- **Path:** `docs/examples/{valid,invalid}/**`
  **Type:** Corpus (format gates)
- **Path:** `docs/guides/project_examples/**`
  **Type:** Corpus (format gates)
- **Path:** `crates/yarrow-core/lib/std/**`
  **Type:** Corpus (format gates)
- **Path:** `crates/yarrow-core/PLAN.md`
  **Type:** Compiler API
- **Path:** `AGENTS.md`
  **Type:** Agent Rules

**Rule:** When the style guide and the formatter disagree, **change the formatter** (or amend the guide deliberately). Do not invent layout rules absent from the style guide.

---

## Current State

### Landed Features

- **Name:** Core Comment Tokens
  **Description:** `TokenKind::Comment`; parser skips; printer rebuilds whitespace
  **Status:** landed

- **Name:** Format IR
  **Description:** `FormatIr` + `TriviaMap` (leading / trailing / file trailing)
  **Status:** landed

- **Name:** Hygiene
  **Description:** LF, strip trailing WS, final newline; `NotUtf8` on bad files
  **Status:** landed

- **Name:** Indent / Blanks
  **Description:** Tab nesting; aligned `end`; top-level blanks; collapse doubles
  **Status:** landed

- **Name:** Construct + Control Layout
  **Description:** Requires, types, functions, if/match/for/defer/unsafe/handle
  **Status:** landed

- **Name:** Phrase Wrap
  **Description:** Soft wrap before consuming words; continuation +1 tab
  **Status:** landed

- **Name:** Comments
  **Description:** Preserve text; `# ` / ` #` spacing
  **Status:** landed

- **Name:** Require Sort
  **Description:** `sort_requires` / `--no-sort-requires`
  **Status:** landed

- **Name:** Library + Binary
  **Description:** `format_source` / `format_file`; `yarrow-fmt` `--check` / `--stdin`
  **Status:** landed

- **Name:** Shared Driver
  **Description:** `run_fmt` / `FmtInput` for binary and CLI
  **Status:** landed

- **Name:** CLI Integration
  **Description:** `yarrow fmt`: In-process wrapper (`yarrow-cli` Stage 12)
  **Status:** landed

- **Name:** Corpus Gate
  **Description:** `valid` / `warnings` / `project` + `lib/std` + stage19 fixture; CI `fmt-check`
  **Status:** landed

- **Name:** LSP Integration
  **Description:** Full-document format via `format_source_best_effort`
  **Status:** landed

- **Name:** File Layout Reorder
  **Description:** `reorder_layout` / `--reorder-layout` (opt-in)
  **Status:** landed

- **Name:** Defaults Polish
  **Description:** Sort on by default; `MIN_MAX_WIDTH`; no spaces-indent
  **Status:** landed

- **Name:** Range / Span Format API
  **Description:** `format_range` / `FormatRangeEdit` (LSP Stage 17)
  **Status:** landed

- **Name:** Stdlib + CI
  **Description:** `scripts/fmt-check.sh` / CI workflow
  **Status:** landed

- **Name:** Best-effort Incomplete Parse
  **Description:** Hygiene subset + `Parser::parse_recovering`
  **Status:** landed

- **Name:** Selective Construct Reprint
  **Description:** Recovered top-level decls reprint when spans clear errors
  **Status:** landed

- **Name:** Ignore Regions
  **Description:** `# yarrow-fmt-ignore-begin` / `# yarrow-fmt-ignore-end`
  **Status:** landed

- **Name:** Parallel Multi-file Driver
  **Description:** Rayon over independent paths; sorted reporting
  **Status:** landed

- **Name:** Widened Fmt-check Corpus
  **Description:** `valid/16_*`–`21_*`, `invalid/**`, `docs/guides/project_examples/**`, stage19 fixture on the CI gate
  **Status:** landed

- **Name:** CLI Range Mode
  **Description:** `--range START:END`; `yarrow-fmt-range-v1` stdout encoding
  **Status:** landed

**Gates:** `./scripts/fmt-check.sh` (or `yarrow fmt --check` on the Stage 21 path set) exits `0`; `cargo fmt && cargo check && cargo clippy` green for `yarrow_fmt` / `yarrow_cli`.

## Phases

### Phase 0-22

- **Name:** Core Formatter Development
- **Description:** Initial formatter implementation with all major features
- **Status:** landed
- **Stages:**
  - **Number:** 0-22
    **Name:** Formatter Pipeline
    **Description:** Complete formatter pipeline from parsing to output
    **Status:** landed
    **Gate:** All formatting stages complete and tested

---

## Backlog

### Stage 23 - Naming Lints

- **Number:** 23
- **Name:** Naming Lints
- **Description:** Belong in core warnings or a future `yarrow lint`, not silent format
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Define naming conventions in style guide (if not already done)
  - **Description:** Implement naming lints in core (not in formatter)
  - **Description:** Add CLI command `yarrow lint` for naming and style checks
  - **Description:** Keep formatter focused on mechanical layout
- **Gate:** New lint command works; formatter does not silently rename. `cargo clippy` green.

---

### Stage 24 - On-type Format Helpers

- **Number:** 24
- **Name:** On-type Format Helpers
- **Description:** LSP Stage 23: local indent after `end`+newline (no fmt API yet)
- **Status:** pending
- **Priority:** low
- **Dependencies:**
  - **Crate:** yarrow-lsp
    **Stage:** 23
    **Description:** On-type formatting needs
- **Tasks:**
  - **Description:** Coordinate with `yarrow-lsp` on on-type formatting needs
  - **Description:** Implement local indent adjustment for specific patterns
  - **Description:** Ensure it does not conflict with full document formatting
  - **Description:** Test with editor workflows
- **Gate:** On-type formatting works in LSP; full document format unchanged. `cargo clippy` green.

### Stage 25 - Format Config File

- **Number:** 25
- **Name:** Format Config File
- **Description:** Only if multi-flag defaults become painful; guide must define it
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Monitor if users need per-project format configuration
  - **Description:** If needed, design config file format (TOML or JSON)
  - **Description:** Implement config file discovery and parsing
  - **Description:** Allow overrides via CLI flags
- **Gate:** Config file works; CLI flags still override. `cargo clippy` green.

### Stage 26 - Incremental / Cached IR

- **Number:** 26
- **Name:** Incremental / Cached IR
- **Description:** After Stage 20 if parse dominates wall time (Stage 20 landed parallel files only)
- **Status:** pending
- **Priority:** low
- **Tasks:**
  - **Description:** Profile formatting performance on large codebases
  - **Description:** If parse is bottleneck, implement caching of format IR
  - **Description:** Invalidate cache on source changes
  - **Description:** Ensure correctness (format output must be identical)
- **Gate:** Performance improvement demonstrated; output unchanged. `cargo clippy` green.

---

## Working Rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use fixtures + `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- Do not reimplement the language grammar in this crate; parse via `yarrow-core`.
- Do not type-check or run programs as part of format.
- In comments and documentation, never use em dash; use ASCII hyphen or rephrase.
- Format only through this crate API from CLI / LSP; never a second pretty-printer.
- If core needs trivia / recovery / API changes, land them in `yarrow-core` with a note here and in the core plan as needed.
