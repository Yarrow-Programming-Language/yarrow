# Yarrow Formatter Implementation Plan

Library and binary that rewrite `.yar` source to match `docs/STYLE_GUIDE.md`.

`yarrow-fmt` is a **tooling crate**. It may depend on `yarrow-core` for tokenize / parse / diagnostics. It does **not** type-check, borrow-check, or codegen. CLI wiring (`yarrow fmt`) lives in `crates/yarrow-cli/PLAN.md`. Editors format via `yarrow-lsp` calling the same `format_source` API.

## Source of truth

- **Layout / idiomatic form:** `docs/STYLE_GUIDE.md`
- Language syntax: `docs/GRAMMAR.md`, `docs/SYNTAX.md`
- Intended AST: `docs/AST.md`
- Corpus (format gates): `docs/examples/{valid,invalid}/**`, `docs/guides/project_examples/**`, `crates/yarrow-core/lib/std/**`, fixtures
- Compiler API: `crates/yarrow-core/PLAN.md`
- Agent rules: `AGENTS.md`

When the style guide and the formatter disagree, **change the formatter** (or amend the guide deliberately). Do not invent layout rules absent from the style guide.

---

## Scope

### In scope

Mechanical rewrite of parseable source:

- Source file hygiene: UTF-8, LF, no trailing whitespace, final newline
- Indent: one tab per nesting level; no space indent
- Soft wrap target: 100 columns (break before consuming words); `FormatOptions::max_width` / `--max-width`
- Blank-line rules between top-level items and inside bodies
- Construct layout: `require`, types, `implement`, functions, `if`/`match`/`for`/`defer`/`unsafe`, calls, containers
- Comment text preserved; spacing around `#` normalized where the guide is explicit (`# ` after hash; one space before trailing `#`)
- Opt-in file-layout reorder; require sorting on by default (`--no-sort-requires` to disable)
- Best-effort hygiene on incomplete parse; selective construct reprint when recovery spans are trustworthy
- Diff-friendly ignore regions via `# yarrow-fmt-ignore-begin` / `# yarrow-fmt-ignore-end`
- Parallel multi-file driver
- CLI range mode: `--range START:END` prints `format_range` edit encoding

### Out of scope

- Renaming (`PascalCase`, etc.): Naming is style/lint, not rewrite; core warnings / future lint
- Idiom rewrites (`+` to `~`, etc.): Semantic / teachable; not silent format
- Type-check / borrow fixes: Compiler / `yarrow check`
- Tabs vs spaces as a config knob: Style guide fixes tabs; formatter always emits tabs
- Editor format-on-save wiring: Editor / LSP client; server already exposes full-doc format

**Idempotence:** `format(format(src)) == format(src)` for accepted inputs (fully parsed subset only when best-effort).

---

## Architecture

```text
source (.yar)
  -> comment tokens (yarrow-core; whitespace rebuilt in printer)
  -> parse / parse_recovering (yarrow-core)
  -> format IR (AST + TriviaMap)
  -> printer (STYLE_GUIDE rules: construct -> phrase wrap -> indent -> blanks -> hygiene)
    or hygiene-only / selective reprint when parse incomplete (format_source_best_effort);
      ignore regions restored from hygiened original after layout
  -> UTF-8 string / write back

multi-file run_fmt: independent paths formatted in parallel (rayon);
  reporting / writes stay sorted-path order; stdin and single-file stay sequential

--range START:END: format_range -> yarrow-fmt-range-v1 stdout encoding
  (no file write); editors keep using LSP rangeFormatting
```

Public surface:

```rust
pub struct FormatOptions {
    pub max_width: usize,       // default 100; CLI rejects below MIN_MAX_WIDTH (20); library clamps
    pub sort_requires: bool,    // default true; --no-sort-requires to preserve order
    pub reorder_layout: bool,   // default false (opt-in)
}

pub const MIN_MAX_WIDTH: usize = 20;
pub const DEFAULT_MAX_WIDTH: usize = 100;

pub enum FormatError { Io, NotUtf8, Parse }

pub struct FormattedSource { pub text: String, pub best_effort: bool }

pub fn format_source(source: &str, options: &FormatOptions) -> Result<String, FormatError>;
pub fn format_source_best_effort(source: &str, options: &FormatOptions) -> Result<FormattedSource, FormatError>;
pub fn format_file(path: &Path, options: &FormatOptions) -> Result<String, FormatError>;

pub struct FmtInput { /* options, check, stdin, best_effort, range, paths */ }
pub fn run_fmt(program: &str, input: FmtInput) -> ExitCode;
```

CLI (`yarrow-fmt` and `yarrow fmt`):

- default: Format files in place
- `--check`: Exit non-zero if any file would change
- `--stdin`: Read stdin, write formatted stdout
- `--max-width N`: Soft wrap width (default 100; min 20)
- `--sort-requires`: Force-on require sorting (default already on)
- `--no-sort-requires`: Keep top-level require source order
- `--reorder-layout`: Opt-in top-level file-layout reorder
- `--best-effort`: On parse failure, hygiene (and selective reprint)
- `--range START:END`: Byte-span edit encoding to stdout
- paths / dirs: `.yar` files; recurse directories

Exit codes: `0` ok / already formatted (`--check`), `1` would reformat or parse/format failure, `2` usage / I/O.

---

## Status

Stages 0-22 are complete. Historical stage write-ups were removed; git history keeps them.

Landed features:

- Core comment tokens: `TokenKind::Comment`; parser skips; printer rebuilds whitespace
- Format IR: `FormatIr` + `TriviaMap` (leading / trailing / file trailing)
- Hygiene: LF, strip trailing WS, final newline; `NotUtf8` on bad files
- Indent / blanks: Tab nesting; aligned `end`; top-level blanks; collapse doubles
- Construct + control layout: Requires, types, functions, if/match/for/defer/unsafe/handle
- Phrase wrap: Soft wrap before consuming words; continuation +1 tab
- Comments: Preserve text; `# ` / ` #` spacing
- Require sort (default on): `sort_requires` / `--no-sort-requires`
- Library + binary: `format_source` / `format_file`; `yarrow-fmt` `--check` / `--stdin`
- Shared driver: `run_fmt` / `FmtInput` for binary and CLI
- `yarrow fmt`: In-process wrapper (`yarrow-cli` Stage 12)
- Corpus gate: `valid` / `warnings` / `project` + `lib/std` + stage19 fixture; CI `fmt-check`
- LSP full-document format: `yarrow-lsp` uses `format_source_best_effort`
- File layout reorder (opt-in): `reorder_layout` / `--reorder-layout`
- Defaults polish: sort on by default; `MIN_MAX_WIDTH`; no spaces-indent
- Range / span format API: `format_range` / `FormatRangeEdit` (LSP Stage 17)
- Stdlib + CI `--check`: `scripts/fmt-check.sh` / CI workflow
- Best-effort incomplete parse: hygiene subset + `Parser::parse_recovering`
- Selective construct reprint: recovered top-level decls reprint when spans clear errors
- Ignore regions: `# yarrow-fmt-ignore-begin` / `# yarrow-fmt-ignore-end`
- Parallel multi-file driver: rayon over independent paths; sorted reporting
- Widened fmt-check corpus: `valid/16_*`–`21_*`, `invalid/**`, `docs/guides/project_examples/**`, stage19 fixture on the CI gate
- CLI range mode: `--range START:END`; `yarrow-fmt-range-v1` stdout encoding

**Gates:** `./scripts/fmt-check.sh` (or `yarrow fmt --check` on the Stage 21 path set) exits `0`; `cargo fmt && cargo check && cargo clippy` green for `yarrow_fmt` / `yarrow_cli`.

---

## Backlog (new tasks from backlog)

### Stage 23 - Naming lints

Belong in core warnings or a future `yarrow lint`, not silent format.

1. Define naming conventions in style guide (if not already done)
2. Implement naming lints in core (not in formatter)
3. Add CLI command `yarrow lint` for naming and style checks
4. Keep formatter focused on mechanical layout

**Gate:** New lint command works; formatter does not silently rename. `cargo clippy` green.

---

### Stage 24 - On-type format helpers

LSP Stage 23: local indent after `end`+newline (no fmt API yet).

1. Coordinate with `yarrow-lsp` on on-type formatting needs
2. Implement local indent adjustment for specific patterns
3. Ensure it does not conflict with full document formatting
4. Test with editor workflows

**Gate:** On-type formatting works in LSP; full document format unchanged. `cargo clippy` green.

---

### Stage 25 - Format config file

Only if multi-flag defaults become painful; guide must define it.

1. Monitor if users need per-project format configuration
2. If needed, design config file format (TOML or JSON)
3. Implement config file discovery and parsing
4. Allow overrides via CLI flags

**Gate:** Config file works; CLI flags still override. `cargo clippy` green.

---

### Stage 26 - Incremental / cached IR

After Stage 20 if parse dominates wall time (Stage 20 landed parallel files only).

1. Profile formatting performance on large codebases
2. If parse is bottleneck, implement caching of format IR
3. Invalidate cache on source changes
4. Ensure correctness (format output must be identical)

**Gate:** Performance improvement demonstrated; output unchanged. `cargo clippy` green.

---

## Mapping: style guide to stages

- Principles: Design only
- Source files: Landed
- Indentation and line width: Landed
- Blank lines: Landed
- Comments: Landed; ignore markers done
- Naming: Out of scope (core / lint)
- File layout (order): Landed (opt-in)
- Modules and require: Landed
- Visibility: Landed (print as written); order via Stage 13
- Types / Functions / Variables: Landed
- Stack phrases and operators: Landed
- Literals and containers: Landed
- Control flow / Defer / Unsafe / Errors: Landed
- Ownership / Stack hygiene: Out of scope (semantics)
- Checklist: Landed layout rows; naming rows ignored
- Tooling / ignore: Done; CLI `--range` done

---

## Working rules

- Prefer minimal diffs that pass the **current** stage gate.
- Do not add tests unless explicitly asked; use fixtures + `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- Do not reimplement the language grammar in this crate; parse via `yarrow-core`.
- Do not type-check or run programs as part of format.
- In comments and documentation, never use em dash; use ASCII hyphen or rephrase.
- Format only through this crate API from CLI / LSP; never a second pretty-printer.
- If core needs trivia / recovery / API changes, land them in `yarrow-core` with a note here and in the core plan as needed.
