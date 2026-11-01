# Yarrow LSP Implementation Plan

Language server for `.yar` editors. Talks to **`yarrow-core`** for tokenize / parse / check / diagnostics, and **`yarrow-fmt`** for document formatting. Owns LSP protocol, document sync, and editor UX. Does **not** reimplement the compiler.

CLI wiring: `yarrow lsp` delegates in-process via `yarrow-cli` Stage 12.

## Source of truth

- Language syntax: `docs/GRAMMAR.md`, `docs/SYNTAX.md`
- AST / types: `docs/AST.md`, `docs/TYPE_SYSTEM.md`
- Modules / runtime: `docs/RUNTIME.md`
- Style (format): `docs/STYLE_GUIDE.md` via `yarrow-fmt`
- Compiler API: `crates/yarrow-core/PLAN.md`
- Formatter API: `crates/yarrow-fmt/PLAN.md`
- Corpus (smoke): `docs/examples/`
- Agent rules: `AGENTS.md`

Prefer core diagnostics and spans over inventing LSP-only error messages. When protocol and language disagree, follow the language docs and map carefully into LSP.

---

## Scope

### Landed (v1, Stages 0-19, 21-26)

- stdio Language Server Protocol (LSP 3.17-shaped)
- TCP `--listen host:port` transport (one client) + in-repo protocol harness
- Text document sync for `file://` `.yar` buffers
- Publish diagnostics from `Session::check_source` (and parse failures)
- Optional multi-root `Session::check_project` via init `projectRoots`
- Pull diagnostics (`textDocument/diagnostic`) with uri+version cache shared with push
- Workspace pull diagnostics (`workspace/diagnostic`) for open buffers + project roots
- Navigation: go-to-definition, find references (same file + `require` cross-file)
- Hover (AST + typed via `CheckedProgram::type_at`) and document symbols
- Completions: keywords + in-scope / imported names + `std.*` require paths
- Document formatting via `yarrow-fmt` (full document + range + safe on-type after `end`)
- Code actions / hover that surface `explain_code` for diagnostic codes
- `LspConfig`, init options, `yarrow lsp` CLI wrapper
- Signature help at postfix `name call` sites
- Inlay hints from core type probes (`--no-inlay` / init `inlayHints`)
- Semantic tokens (full document) from tokenizer + AST decls
- File-local rename (`prepareRename` + `rename`; refuse unsafe cross-module edits)
- Workspace symbols (`workspace/symbol` over open buffers + resolved requires)
- Core `definition_at` probes for goto / hover (Stage 22; AST fallback on miss)
- On-type formatting: newline after `end` aligns indent only (Stage 23)
- Analysis cancelation: version + generation tickets; checks on `spawn_blocking` (Stage 25)
- Virtual `yarrow-std:` URIs when on-disk `lib/std` is missing (Stage 26)

Stage 20 (editor extension packaging) was **canceled**; clients live in separate repos.

### Out of scope

- Package-manager / manifest invent: Language has no project file; roots are explicit paths only
- Silent semantic rename across modules: Stage 15 stays conservative; never guess
- Debug Adapter Protocol: Separate product; AOT/JIT debug is not an LSP feature
- Snippet / AI rewrite actions: Not mechanical language support
- Non-`.yar` / markdown embedded: Skip until requested
- Editor extensions (VS Code / Zed / ...): Live in separate repos later; not in the main compiler tree

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

Public / binary surface:

```rust
pub async fn run_stdio() -> Result<(), LspError>;
pub async fn run_stdio_with(config: LspConfig) -> Result<(), LspError>;
pub fn run_stdio_blocking() -> Result<(), LspError>;
pub async fn run_tcp_with(addr, config, on_listen) -> Result<(), LspError>;
pub fn run_tcp_blocking(addr, config, on_listen) -> Result<(), LspError>;
// Shared by stdio and TCP.
pub async fn run_with_streams(/* ... */) -> Result<(), LspError>;
```

Protocol stack:

- LSP types: via `tower-lsp-server` (community fork of `tower-lsp`)
- Runtime: `tokio`
- Binary: `crates/yarrow-lsp` + `yarrow lsp`

Do **not** shell out to `yarrow check`; call `Session` in-process.

### Position mapping

LSP positions are UTF-16 code units by default (or UTF-8 if negotiated). Core `Span` / `SourceFile::location` use **byte offsets** and Unicode scalar columns.

1. `PositionMap` converts `Span` <-> `lsp::Range`
2. Prefer negotiating `positionEncoding = utf-8` when the client supports it; still support UTF-16 for VS Code-class clients.

### Analysis model

On `didOpen` / `didChange` (debounced):

1. Update buffer text
2. Build `CompileOptions` with `source_path` from URI, search paths from init options / workspace folders
3. Call `check_source` (or `check_project` when Stage 21 multi-root mode applies)
4. Map `DiagnosticBatch` -> `PublishDiagnosticsParams` (and answer pull requests)
5. Cache last successful parse / check artifact for hover, navigation, inlays, tokens

Open documents + transitive `require` resolution cover the single-file case. Multi-root project check is Stage 21 (core graph already exists).

---

## Current state

All v1 LSP stages (0-19, 21-26) are complete. Stage 20 (editor extensions) was canceled.

Landed features:

- `yarrow-lsp` crate: Stages 0-19 + 21-26 landed
- Core Session API: `parse_source` / `check_source` / `check_project` + spans
- Core diagnostics: `Diagnostic` / `Severity` / codes / explain table
- Typed hover data: `CheckedProgram::type_at` (core Stage 30)
- Def / require probe: Core Stage 35 + LSP Stage 22 consume `definition_at`
- Cross-file resolve: Probe + AST / `require`; project multi-root in Stage 21
- `yarrow-fmt`: Full-doc best-effort + `format_range`; on-type is local indent
- CLI `yarrow lsp`: In-process `run_stdio_blocking` / `--listen`

---

## Feature mapping

LSP capability to stages:

- initialize / shutdown: Stage 0
- textDocument sync: Stage 1
- publishDiagnostics: Stage 2; cancelation Stage 25
- documentSymbol: Stage 3
- definition: Stage 4, 7, 22, 26 (probes; virtual std fallback)
- hover: Stage 5, 9, 22, 26 (AST; type_at; def/require; virtual std)
- completion: Stage 6 (grammar keywords + AST names)
- references: Stage 7 (binding / name index)
- formatting: Stage 8 (`yarrow-fmt`)
- codeAction / explain: Stage 11 (`explain_code`)
- signatureHelp: Stage 12 (AST + type_at)
- inlayHint: Stage 13 (TypeIndex probes)
- semanticTokens: Stage 14 (tokens + AST)
- rename: Stage 15 (references / resolve; file-local)
- workspaceSymbol: Stage 16; Stage 21 (open + requires; project roots via check_project)
- rangeFormatting / onTypeFormatting: Stage 17; Stage 23 (format_range; on-type = local indent after end)
- textDocument/diagnostic (pull): Stage 18 (same as publish + uri/version cache)
- workspace/diagnostic: Stage 24 (open / project roots only)
- TCP + test harness: Stage 19 (transport only)
- multi-root project check: Stage 21 (check_project; core Stage 28)
- analysis cancelation / latency: Stage 25 (gen tickets + spawn_blocking)
- virtual yarrow-std: URIs: Stage 26 (embedded std when on-disk missing)
- editor extensions: Stage 20 (canceled; separate repos)

---

## Backlog

- Cross-module silent rename: Explicitly refused; Stage 15 stays conservative
- Salsa / full incremental IR: Only if Stage 25 still too slow
- DAP / debug adapter: Separate product
- Markdown / embedded .yar: Skip until requested
- Editor extensions: Separate repos (Stage 20 canceled here)
- Call / type hierarchy: Needs richer core index; do not fake from names alone

---

## Working rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use scripted LSP messages + `docs/examples/**` as gates (prefer `scripts/harness.mjs` from Stage 19).
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history). When a whole phase is done, collapse finished stages into **Landed** the same way Stages 0-19 were.
- No tokenizer / parser / typechecker logic here beyond calling `yarrow-core`.
- Format only through `yarrow-fmt`, never a second pretty-printer.
- In comments and documentation, never use em dash; use ASCII hyphen or rephrase.
- If core needs position helpers, richer probes, or require-path APIs, land them in `yarrow-core` and note the dependency here and in the core plan Known gaps / Next.
- Keep the safe vs unsafe boundary visible in hovers / inlays when relevant; do not imply `unsafe` turns off checking.
