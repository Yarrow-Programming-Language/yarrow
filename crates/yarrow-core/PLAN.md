# Yarrow Core Implementation Plan

Compiler library: tokenize → parse → check → `{ jit | object | executable | interpret }`.

`yarrow-core` is the **API**. Drivers (`yarrow-cli`, `yarrow-fmt`, `yarrow-lsp`) call it; they do not reimplement the pipeline. CLI UX lives in `crates/yarrow-cli/PLAN.md`.

## Source of truth

- Language tour: `docs/GRAMMAR.md`
- Formal syntax: `docs/SYNTAX.md`
- AST / types / memory: `docs/AST.md`, `docs/TYPE_SYSTEM.md`, `docs/MEMORY_MODEL.md`
- Runtime / modules: `docs/RUNTIME.md`
- Corpus: `docs/examples/`
- Agent rules: `AGENTS.md`

Prefer the docs when code and docs disagree. Do not invent language features absent from the docs.

---

## Status

### Landed

- Frontend: Tokenizer + parser (flat postfix `Apply*`); rustc-style diagnostics; `Comment` tokens
- Checking: Types, ownership, borrow, regions, unsafe; stack-effect notes; `LowerKind::Check` (no JIT install)
- Warnings: `W401`–`W410` (unused / dead stack / never-written mutable / redundant copy / require ambiguity / unreachable / empty match arm / empty if then / empty unsafe); `CheckedProgram::warnings`
- Session API: `check` / `compile` (JIT, explicit mode) / `compile_object` / `compile_executable` / `interpret`; default `ExecutionMode::Object`; `type_at` (30) + `definition_at` (35); ICE `E999` / `SessionFailureKind` (40)
- AOT: Runtime archive + Cranelift process `main` + `ld`/`lld` link (linux-gnu + static musl + Stage 39 Windows-gnu host to host); DWARF + `OptLevel`; cross object emit (linux-gnu / linux-musl + Stage 34 COFF / Mach-O)
- Projects: `ProjectOptions` / `check_project` / `ModuleGraph`; `E382` cycles; `E383` missing roots (`docs/guides/project_examples/`)
- Runtime/std: Host heap, regions, lists/maps/strings; `std.io` / `std.string` / `std.fs` host wrappers
- Interpret: Stage 37 gate of `docs/examples/valid/**` (stdout matches JIT): Stage 36 plus unsafe/`pointer<T>`/`move`/runes/`std.fs`; `00_grammar_tour.yar` remains out of scope (E393)
- Probes: `TypeIndex` / `type_at`; `DefIndex` / `definition_at` (bindings + requires, root-only; Stage 35)

**Gates:** `docs/examples/valid/**` compile and run (JIT); `invalid/**` fail for the stated reason; `valid/16_*`–`21_*` check with `Ok` + warnings; `cargo fmt && cargo check && cargo clippy` green.

Phases A–E (Stages 0–24) and Phase F–G (Stages 25–26, 28–34) are complete. Stage 27 (bundled linker) stays deferred. Historical stage write-ups were removed; git history keeps them.

---

### Known gaps

- AOT: Cross link needs matching archive + CRT; Mach-O executable link still needs a Darwin host / CI; Windows-gnu host to host exe is Stage 39 done
- Interpret: `00_grammar_tour.yar` stays out of scope (mixed surface / `loop.break` and further tour forms); Stage 37 landed unsafe / pointers / move / fs
- Linker: System `ld`/`lld` only; Stage 27 bundled linker deferred (discovery remains reliable)
- LSP assist: Typed-at-span + definition / require probes landed (Stage 35); LSP consumption is `yarrow-lsp` Stage 22
- Formatter: Whitespace rebuilt by printer (`yarrow-fmt`); incomplete parse → hygiene + selective top-level reprint; ignore regions Stage 19; parallel multi-file Stage 20; widened fmt-check corpus Stage 21

---

## Multi-platform compilation status

**Supported targets:**

- **linux-gnu** (x86_64, aarch64): Full executable link on linux-gnu host
- **linux-musl** (x86_64, aarch64): Static-friendly executable link on linux-gnu host (Stage 33)
- **windows-gnu** (x86_64): Executable link on Windows-gnu host only (Stage 39); uses MinGW `ld`/`lld` + CRT (crt2.o / crtbegin.o / crtend.o)
- **apple-darwin** (x86_64, aarch64): Object emit only (Stage 34); executable link needs Darwin host (backlog)

**Limitations:**

- No cross-compilation from Linux to Windows (requires native Windows-gnu host with MSYS2 UCRT64)
- No MSVC support (only Windows-gnu / MinGW toolchain)
- No macOS executable link yet (Darwin host path not wired; needs native linker + CRT discovery)
- bundled linker (Stage 27) remains deferred

**Conclusion:** Multi-platform compilation is feasible and working for the supported targets. The current approach of host-only linking with native toolchains is sound and maintainable. Cross-compilation from Linux to Windows is not feasible without significant complexity (would require PE linkers on Linux and proper CRT handling). This is an acceptable limitation, not a language design constraint.

For reference: CI workflow at `.github/workflows/stage-39-windows-gnu-exe.yml` demonstrates Windows-gnu host to host compilation using MSYS2 UCRT64 environment.

---

## Next (Phase H)

Focus: Darwin Mach-O executable link when a native macOS host / CI agent is available (Stage 39 Windows-gnu path done). Stage 40 ICE tagging is done. Keep Stage 27 deferred unless PATH linkers become fragile. Do not invent language features. Project CLI is `yarrow-cli` Stage 13 done. LSP Stage 22 consumes Stage 35 probes. CLI Stage 16 can consume `SessionFailureKind::Ice` → exit `101`.

All current stage gates (35–40) are complete.

---

## Backlog (new tasks from backlog)

### Stage 41 - Darwin Mach-O executable link

Implement `compile_executable_source` for `x86_64-apple-darwin` and `aarch64-apple-darwin` on native macOS host.

1. Use `ld64` or `lld` for Mach-O linking (no `cc` compile step)
2. Discover CRT / import libs explicitly (env / documented paths)
3. Prefer host to host path first
4. Update `docs/RUNTIME.md` Cross-compile matrix and Known gaps

**Gate:** on a documented macOS host (or documented CI), `compile_executable_source` for that triple produces a runnable binary for `docs/examples/valid/01_hello.yar`. Existing linux-gnu / windows-gnu paths unchanged. Object-only path for Stage 34 triples still works. `cargo clippy` green.

---

### Stage 42 - Program argv API

Needs GRAMMAR / std design first; CLI object path already forwards OS argv.

1. Design argv access in GRAMMAR (language-level, not host-specific)
2. Implement in std library
3. Wire through Session API
4. Expose in JIT / object / interpret modes

**Gate:** A valid Yarrow program can access command-line arguments. `cargo clippy` green.

---

### Stage 43 - Multi-root compile and run

Check-only graph exists via `check_project`; now enable compile and run for multi-root projects.

1. Define multi-entry product story (which root is main? how to link multiple objects?)
2. Implement `compile_project` / `run_project` in Session API
3. Wire through CLI
4. Update `docs/RUNTIME.md` Projects section

**Gate:** `yarrow run docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar --main root_a` compiles and runs the project. `cargo clippy` green.

---

### Stage 44 - MSVC AOT triple support

After Stage 39 Windows-gnu path done. MSVC later if needed.

1. Add `x86_64-pc-windows-msvc` triple support
2. Implement CRT discovery for MSVC toolchain
3. Link via `link.exe` (no `cl.exe` compile step)
4. Update target.rs and link.rs

**Gate:** MSVC host to host executable link works on native Windows-MSVC host. `cargo clippy` green.

---

### Stage 45 - Bundled linker (revisit Stage 27)

Only if PATH `ld`/`lld` discovery becomes fragile in practice.

1. Evaluate if system linker discovery is reliable across supported platforms
2. If needed, bundle minimal linker (e.g., `lld`) for supported targets
3. Fall back to system linker when available
4. Document the approach in `docs/RUNTIME.md`

**Gate:** Compilation works with and without system linker installed. `cargo clippy` green.

---

## Working rules

- Prefer minimal diffs that pass the **current** stage gate.
- When renaming std or syntax, update compiler and `lib/std` together.
- Do not add tests unless explicitly asked; use `docs/examples/**` as gates.
- Update this file when a stage gate lands (mark done, short notes; do not re-expand history).
- Do not invent lifetime syntax; ownership and regions are the model.
- `unsafe` never disables type, stack, ownership, or borrow checking.
- No CLI parsing (`clap`, argv, exit codes) here; that is `yarrow-cli`.
- May invoke a system **linker** from a library helper; must not depend on a C compiler for CRT or codegen.
