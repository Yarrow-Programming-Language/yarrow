# Runtime

How a Yarrow program executes: evaluation stack, calls, errors, and modules. Complements [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md) and [`MEMORY_MODEL.md`](MEMORY_MODEL.md). Surface forms come from [`GRAMMAR.md`](GRAMMAR.md) and [`SYNTAX.md`](SYNTAX.md).

Contents: [Execution model](#execution-model), [Stack](#stack), [Functions](#functions), [Errors](#errors), [Internal compiler errors](#internal-compiler-errors-stage-40), [Modules](#modules), [Projects](#projects), [CLI compile artifacts](#cli-compile-artifacts), [Session probes](#session-probes-stages-30--35), [Warnings](#warnings-stage-20--38).

## Execution model

Pipeline: source `.yar` is tokenized, parsed to an AST, checked, then run or emitted via one of `check`, `jit`, `object`, `executable`, or `interpret`.

**Trivia:** The tokenizer emits `Comment` tokens for `#` ... end of line (lexeme includes `#` and comment text; the terminating newline is not part of the lexeme). Whitespace and newlines are not tokens. The parser skips `Comment` tokens the same way it ignores whitespace; comments are not AST nodes. Tools that need comment text (formatters) should read the token stream before parse.

| Backend      | Role                                                                              |
| ------------ | --------------------------------------------------------------------------------- |
| `check`      | Type / ownership / stack / region analysis; CLIF lower without JIT/object product |
| `jit`        | Cranelift in-process machine code; driver may run `main`                          |
| `object`     | Relocatable native object (ELF / Mach-O / COFF); link stays outside               |
| `executable` | Object emit + system `ld`/`lld` link with the runtime archive                     |
| `interpret`  | Tree-walk interpreter over the checked AST (file / future REPL)                   |

`Session::interpret_source` covers Stage 21-36 plus Stage 37 (`08` ownership/`move`, `11` unsafe/`pointer<T>`/`std.mem`, `14` io/string, `15` fs) with stdout matching JIT. `00_grammar_tour.yar` stays interpret-out-of-scope (`E393`). Gate: `cargo run -p yarrow_core --example check_interpret`.

### Default backend (Stage 29)

Product default is **object** (AOT), not JIT:

| Surface                                                                                                                       | Default                 | Opt in to JIT                   |
| ----------------------------------------------------------------------------------------------------------------------------- | ----------------------- | ------------------------------- |
| [`CompileOptions::new`](../../crates/yarrow-core/src/session.rs) / [`ExecutionMode`](../../crates/yarrow-core/src/session.rs) | `ExecutionMode::Object` | Set `mode = ExecutionMode::Jit` |
| CLI `run` / `compile` / bare `yarrow <file.yar>`                                                                              | `--target object`       | `--target jit`                  |

`Session::compile_source` and `run_main` stay JIT-only: they require an explicit `ExecutionMode::Jit` (default `Object` yields `E391` pointing at `compile_object_source`). `check_source` / `interpret_source` are unchanged and do not follow the object default for their pipelines.

- User modules resolve relative to the source file's directory (`"a.b"` -> `a/b.yar`).
- The standard library is embedded and imported the same way as user code (`"std.io"`, ...).
- Compiled and interpreted code talks to a small **host runtime** for heap headers (strings, lists, maps, regions, free) and raw `alloc` / `free`. Object emit leaves those symbols as imports for a later link.
- Heap values are opaque handles; scalars and addresses are machine words. Kind codes describe how to free nested heap data.

### AOT link surface (host runtime)

Object emit (`Session::compile_object_source`) lowers `@name` / host calls to **`Linkage::Import`** symbols. Names and C ABIs come from the [`HOST_FNS`](../../crates/yarrow-runtime/src/lib.rs) table in `yarrow_runtime` (single source of truth with JIT `install_runtime`).

| Layer          | Crate / API                                              | Role                                                                                   |
| -------------- | -------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Implementation | `yarrow_runtime` (rlib)                                  | Heap, `@print_*`, regions, `HOST_FNS`                                                  |
| JIT            | `runtime::install_runtime`                               | Registers `HOST_FNS` names -> addresses in the in-process JIT linker                   |
| AOT archive    | `yarrow_runtime_aot` (`staticlib`)                       | Same code, `aot-exports` feature adds linker-visible names (`alloc`, `print_str`, ...) |
| Library access | `yarrow_core::linkable_archive` / `linkable_archive_for` | Reads `libyarrow_runtime_aot.a` (host or per-triple; see Stage 26) for AOT link        |
| Executable     | `Session::compile_executable_source`                     | Object emit + `link::link_executable` via system `ld`/`lld` (not `cc`)                 |

Build the archive: `cargo build -p yarrow_runtime_aot`. Program `.o` (with Cranelift process `main`) + runtime `.a` are linked by `compile_executable_source`. No C CRT source and no `cc` compile step; CRT object paths may be discovered with `cc -print-file-name` when present.

**Imports in a typical program object** (all defined by the runtime archive):

| Symbol                                                        | Role                                        |
| ------------------------------------------------------------- | ------------------------------------------- |
| `alloc`, `free`                                               | Raw heap (`@alloc` / `@free`, unsafe)       |
| `str_new`, `str_len`, `str_join`, `str_cmp`                   | String heap helpers (`std.string`)          |
| `fs_open`, `fs_close`, `fs_read`, `fs_write`, `fs_last_error` | File open / close / read / write (`std.fs`) |
| `list_*`, `map_*`                                             | List / hashmap helpers                      |
| `print_str`, `print_int`, `print_float`, `print_newline`      | `std.io` write / write_line / newline       |
| `print_array`, `print_list`, `print_hashmap`                  | Container debug print                       |
| `free_value`, `register_struct_descs`, `register_union_descs` | Drop / layout registration                  |
| `region_new`, `region_register`, `region_free`                | Region lifetime                             |

**Std wrappers (safe Yarrow):**

| Module       | API                                                                | Host / builtin                                                         |
| ------------ | ------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| `std.io`     | `write`, `write_line`, `write_int`, `write_float`, `newline`       | `@print` / `@print_*` -> `print_str` / `print_*`                       |
| `std.string` | `len`, `concat`, `join` (left, right, sep), `compare` (-1 / 0 / 1) | `@string_len`, `~`, `@string_join`, `@str_cmp`                         |
| `std.fs`     | `open_file`, `close_file`, `read_file`, `write_file`               | `@fs_open` / `@fs_close` / `@fs_read` / `@fs_write` / `@fs_last_error` |

Prefer alias `str` for `"std.string"` (`string` is a type keyword and cannot be a require scope name).

`std.fs` modes: `'r'` read, `'w'` write+create+truncate, `'a'` append+create. Host status codes: `0` ok, `1` IO, `2` not found, `3` invalid argument (`fs_open` returns the negated code on failure). Fallible wrappers map those to `error.IO_ERROR` / `NOT_FOUND` / `INVALID_ARGUMENT`.

`free` is exported under that linker name for object imports; on glibc the implementation forwards to `__libc_free` so it does not recurse into itself when other translation units call `free`.

Do not export those names from the JIT driver binary (`aot-exports` is AOT-only); a global `alloc` symbol would clash with the host allocator.

### Program entry / process `main`

A Yarrow program entry (default name `main`, override via `CompileOptions::entry_name`) is **not** the same symbol as the host process entry.

| Piece         | API / symbol                                                 | Role                                                                                                       |
| ------------- | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------- |
| Entry name    | `CompileOptions::entry_name` (default `main`)                | Which top-level Yarrow function is the program entry. CLI `--main` maps here (CLI plan).                   |
| Require / run | Session require-entry, JIT `run_main`, interpret `run_entry` | All honor `entry_name`. Missing entry is `E360` naming that function.                                      |
| Object export | `PROCESS_MAIN_SYMBOL` (`main`)                               | Cranelift-emitted process entry (`() -> i32`): calls the Yarrow entry and maps the return to an exit code. |

Exit mapping (process `main` trampoline):

- void / non-integer single return -> `0`
- integer (including bool / enum) -> value as exit status
- fallible envelope -> `1` on error tag, else `0`

**Link:** `Session::compile_executable_source` links program `.o` + `libyarrow_runtime_aot.a` with `ld`/`lld` (not `cc`). Diagnostics: `E394` linker/CRT missing, `E395` link failed, `E396` runtime archive unavailable, `E397` unsupported target. Default target is the host linux-gnu triple; see [Cross-compile triples](#cross-compile-triples-stage-26--33--34).

### AOT debug info and optimization (Stage 25)

Object and executable products honor two [`CompileOptions`](../../crates/yarrow-core/src/session.rs) knobs (CLI wiring comes later):

| Option       | Default          | Effect                                                                        |
| ------------ | ---------------- | ----------------------------------------------------------------------------- |
| `opt_level`  | `OptLevel::None` | Cranelift `opt_level`: `none` / `speed` / `speed_and_size` (`OptLevel::Size`) |
| `debug_info` | `true`           | Emit DWARF (`.debug_info` / `.debug_line` / ...) into the program object      |

DWARF includes a compilation unit for the source path, `DW_TAG_subprogram` entries for defined functions (Yarrow names plus process `main`), and coarse line mappings at function entries when spans exist. Inspect with `llvm-dwarfdump` or `readelf --debug-dump=info`.

JIT uses the same `opt_level` (default stays debug-friendly `None`). JIT does not emit DWARF.

### Cross-compile triples (Stage 26 / 33 / 34 / 39)

Object emit and executable link take an optional [`CompileOptions::target`](../../crates/yarrow-core/src/session.rs) ([`TargetTriple`](../../crates/yarrow-core/src/target.rs)). `None` means the host.

| Triple                                                           | Role                                                                                            |
| ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Host (`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`) | Default object / executable path                                                                |
| The other of those two                                           | Stage 26 cross: object emit always; link when archive + CRT are available                       |
| `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl`       | Stage 33 musl: object emit always; static executable link when musl CRT + archive are available |
| `x86_64-pc-windows-gnu`                                          | Stage 34 COFF object emit; Stage 39 host->host PE exe on Windows-gnu (`E397` on other hosts)    |
| `x86_64-apple-darwin` / `aarch64-apple-darwin`                   | Stage 34 Mach-O object emit (executable link `E397` until a Darwin host path lands)             |

Unsupported triples (MSVC, WASM, other arches, ...) fail with `E397` (no panic). JIT rejects a non-host `target` with `E397`. Object emit for aarch64 requires the `arm64` feature on `cranelift-codegen` (enabled by `yarrow_core`). DWARF debug info is emitted for ELF only; Mach-O / COFF objects skip DWARF for now.

**Session example** (non-host COFF object; `CompileOptions::new` already defaults to `Object`):

```rust
use yarrow_core::{CompileOptions, Session, TargetTriple};

let mut opts = CompileOptions::new("hello.yar");
opts.target = Some(TargetTriple::parse("x86_64-pc-windows-gnu").expect("supported"));
let session = Session::new(opts);
let artifact = session.compile_object_source(source)?;
// artifact.target is x86_64-pc-windows-gnu; bytes are x86_64 COFF
```

Inspect: COFF objects start with machine `0x8664`; Mach-O 64-bit LE starts with magic `CF FA ED FE`. Gates: `cargo run -p yarrow_core --example check_cross` (linux) and `cargo run -p yarrow_core --example check_macho_coff` (Stage 34).

**Runtime archive layout** (same ABI / `HOST_FNS` as the host runtime; do not invent a second runtime):

| How                     | Path / env                                                                                                                                                                                                                                                                 |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host (always)           | Built by `yarrow-core`'s `build.rs` -> `YARROW_RUNTIME_AOT_ARCHIVE`                                                                                                                                                                                                        |
| Optional cross at build | Set `YARROW_BUILD_CROSS_AOT=1` when building `yarrow-core` after installing the Rust target (`rustup target add ...` or Nix equivalent). Successful builds appear in `YARROW_RUNTIME_AOT_ARCHIVE_TABLE` as `triple=path;...` (linux cross + optional Windows-gnu / Darwin) |
| Manual / CI override    | `YARROW_RUNTIME_AOT_ARCHIVE_<triple_with_underscores>` -> `libyarrow_runtime_aot.a` (or `yarrow_runtime_aot.lib` on Windows) from `cargo build -p yarrow_runtime_aot --target <triple>`                                                                                    |

Example override for musl:

```bash
cargo build -p yarrow_runtime_aot --target x86_64-unknown-linux-musl
export YARROW_RUNTIME_AOT_ARCHIVE_x86_64_unknown_linux_musl=$PWD/target/x86_64-unknown-linux-musl/debug/libyarrow_runtime_aot.a
```

Example override for aarch64 gnu (Stage 26):

```bash
cargo build -p yarrow_runtime_aot --target aarch64-unknown-linux-gnu
export YARROW_RUNTIME_AOT_ARCHIVE_aarch64_unknown_linux_gnu=$PWD/target/aarch64-unknown-linux-gnu/debug/libyarrow_runtime_aot.a
```

Example override for Windows-gnu (Stage 34 object / Stage 39 host exe):

```bash
cargo build -p yarrow_runtime_aot --target x86_64-pc-windows-gnu
export YARROW_RUNTIME_AOT_ARCHIVE_x86_64_pc_windows_gnu=$PWD/target/x86_64-pc-windows-gnu/debug/libyarrow_runtime_aot.a
```

**CRT / linker for cross executables:** host `ld` must support the target's `-m` emulation (`elf_x86_64` / `aarch64linux`). CRT objects come from the usual `cc -print-file-name` path on the host, or for a non-host triple:

| Env                  | Meaning                                                                      |
| -------------------- | ---------------------------------------------------------------------------- |
| `YARROW_AOT_CRT_DIR` | Directory containing `Scrt1.o` / `crt1.o` / `crti.o` / ... for the target    |
| `YARROW_AOT_SYSROOT` | Sysroot / musl prefix searched under `usr/lib`, `lib`, and multiarch subdirs |

linux-gnu cross links stay dynamic (PIE + glibc dynamic linker). linux-musl links use **`-static`** with musl `crt1.o` / `crti.o` / `crtn.o` and `libc.a` (no gcc `crtbegin` / `crtend`). Point `YARROW_AOT_SYSROOT` at a musl prefix whose `lib/` holds those files (for example a Nix `musl-static-*` store path).

**Windows-gnu host->host (Stage 39):** on a Windows-gnu toolchain (MSYS2 UCRT64 / rustc `x86_64-pc-windows-gnu`), `compile_executable_source` links with MinGW `ld`/`lld` (`-m i386pep`), CRT `crt2.o` / `crtbegin.o` / `crtend.o`, and the usual MinGW + Rust import libs (`mingw32`, `msvcrt`, `ws2_32`, ...). Discovery uses `gcc -print-file-name` or `YARROW_AOT_CRT_DIR` / `YARROW_AOT_SYSROOT`. Gate: `cargo run -p yarrow_core --example check_windows_exe` (CI: `.github/workflows/stage-39-windows-gnu-exe.yml`). linux hosts still return `E397` for Windows/Darwin **executable** link and keep object emit; Mach-O Darwin host->host is not wired yet.

Missing archive -> `E396`. Missing CRT / linker -> `E394`. Unsupported triple / object-only executable -> `E397`.

The language model is stack-based regardless of backend. JIT and object backends lower each function to Cranelift IR with an explicit compile-time operand stack that becomes SSA values. The interpreter keeps an explicit runtime operand stack instead.

Entry: every runnable program has a top-level entry (default `main`; override via `CompileOptions::entry_name` / CLI `--main`). The driver runs it after JIT or interpret (object emit does not execute). Optional numeric return from the entry is the process exit code for native binaries; the current CLI also prints supported single return values (`void`, integer, float, bool, string).

---

## Stack

The **evaluation stack** is the primary place values live between words.

### Effects

Stack effects by operation kind:
- **Literal, container, name, type value**: push
- **Binary op, `store`, `move`**: pop 2 (+ push result or nothing)
- **Unary op, `typeof`, `borrow`, `load`, `dup`, `pop`**: pop 1 (+ push)
- **`call` / `unwrap`**: pop callee (+ use preceding args); push returns / project envelope
- **`drop`**: clear stack; release borrows
- **`swap` / `rot` / `unrot`**: rearrange

Declarations and `set` pop the value being stored. Control keywords (`if`, `for`, `match`) expect their subject or condition already on top.

### Ownership on the stack

- Stack **owns** temporary non-copy values it creates.
- `pop` / consume / `drop` free owned heap slots (see [`MEMORY_MODEL.md`](MEMORY_MODEL.md)).
- Variable reads of non-copy types push **borrows**, not second owners.
- `return` takes the return payload(s) and drops the rest of the frame's stack.

### Control flow and the stack

- **`if`**: condition bool consumed; then/else must leave compatible stacks at join.
- **`match`**: subject borrowed for the duration; original stack restored after `end`.
- **`for`**: condition form rechecks a bool; iterable form walks a container (helpers from `std.loop`).
- **`defer`**: bodies run at scope exit in reverse order; they see the exiting scope's bindings.
- **`unsafe`**: does not change stack discipline; only permits unsafe ops inside the block.

Stack height and types are checked at compile time; a runtime stack underflow in generated code indicates a compiler bug, not a user-recoverable error.

---

## Functions

### Definition

```text
name [visibility] [unsafe] function { parameter } do { statement } end [with type]
```

- Parameters are the **input stack**: moved onto the local stack in declaration order (first = deepest). Body bindings pop from the top, so the last parameter binds first.
- `with T` is the return type; omit for `void`.
- Nested functions are allowed; they are only callable from the enclosing body.
- Methods live in `Type implement ... end` and usually take `reference<T>` (optionally `mutable`) as the first parameter.

### Call convention (language)

```yarrow
arg1 arg2 callee call
```

1. Push arguments (deepest first, matching parameter declaration order).
2. Push the callee (name or qualified name).
3. `call` pops the callee, consumes arguments per the signature, transfers or borrows per parameter rules, runs the body, pushes return value(s).

Methods:

```yarrow
point borrow
point.distance call
```

The receiver is a `reference<T>` on the stack before `call` (often from `borrow` or a non-copy read).

### `unsafe` functions

- Marked `unsafe function`: may contain unsafe operations; **call sites** must be inside `unsafe ... end`.
- Even inside an unsafe function, unsafe ops are wrapped in `unsafe ... end` so the site is visible.
- Borrow, ownership, and stack checks still apply.

### Host and builtins

A thin host surface backs heap ops and raw memory (`alloc`, `free`, string/list/map helpers, region register/free, ...). Host entries are marked **Safe** or **Unsafe**; unsafe host calls require an unsafe context.

Std modules (`std.mem`, `std.io`, ...) wrap host behavior in Yarrow where possible so user code stays in the safe model.

### `main` (program entry)

- Required for runnable sessions (`require_main`); public by default; no parameter list in surface syntax.
- Default name is `main`; `CompileOptions::entry_name` (CLI `--main`) may select another top-level function.
- Return optional; numeric return may set the process exit code (native process `main` / grammar).
- Fallible entry (`with |T Err|`) is not part of the supported driver print surface yet; AOT trampoline maps error tags to exit `1`.

---

## Errors

Errors are first-class values, declared like specialized enums:

```yarrow
MyCustomErrors error
	MY_CUSTOM_ERROR
end
```

Optional injection copies members from another error type:

```yarrow
MyCustomErrors error.Error error
	# ...
end
```

### Fallible returns

A function that may fail returns a **union literal** of success and error types:

```yarrow
end with |i32 MyCustomErrors|
```

At the ABI level this is an **error envelope** with two slots:
- **env**: `0` for success, error tag (program-unique) for failure
- **payload**: success value (or `0` if void) for success, unused/zero for failure

Returning an error member sets the env tag. Returning a success value sets env to `0` and places the value in the payload (heap ownership transfers to the caller).

### `unwrap`

```yarrow
fallible call unwrap
```

- Success: push the payload (`T`).
- Failure: if the **caller** can error, propagate (return the envelope); otherwise trap / rejected at compile time when the caller cannot error.
- On a non-envelope value, `unwrap` is a no-op (identity).

### `handle`

```yarrow
fallible call handle
	match
		error.MY_CUSTOM_ERROR case
			# ...
		end
		else
			# ...
		end
	end
	0 fallback
end
```

- Success: keep the payload; skip the handler body.
- Failure: run the handler (often a `match` on the error); then push the **fallback** word as the result of the whole `handle`.
- Short form: `call handle 0 fallback end`.

Fallback must be usable at the success type (coercion allowed).

### Error `match`

Inside `handle`, `match` with no prior subject dispatches on the error the same way union `match` dispatches on member types (grammar: cases compare or name error members). Elsewhere, ordinary value `match` uses bool conditions.

Built-in and std error members (e.g. `error.OUT_OF_MEMORY`) are comparable tags across the program.

---

## Internal compiler errors (Stage 40)

Session failures are ordinarily `SessionDiagnostics` batches (user / toolchain codes such as `E2xx`+`E3xx`). Drivers map those to exit `1`.

An **internal compiler error** is tagged as diagnostic code **`E999`** (`ICE_CODE`). It means an invariant or API-boundary failure inside the compiler, not a mistake in the user's program.

| API                                                                       | Role                                  |
| ------------------------------------------------------------------------- | ------------------------------------- |
| `Diagnostic::ice` / `CompileError::ice`                                   | Build an `E999` diagnostic            |
| `SessionDiagnostics::is_ice` / `DiagnosticBatch::is_ice`                  | Detect ICE in a failed session        |
| `SessionDiagnostics::failure_kind` -> `SessionFailureKind::Ice` \| `User` | Driver exit mapping (`101` vs `1`)    |
| `Session::debug_trigger_ice`                                              | Documented gate hook (does not panic) |

Selected API-boundary sites (for example JIT-only `get_finalized_function` on an object backend) return `E999` instead of panicking. The library does **not** blanket-catch panics; the CLI (`yarrow-cli` Stage 16) uses `catch_unwind` around command dispatch and maps both caught panics and `SessionFailureKind::Ice` to exit **`101`**.

CLI exit codes: `0` ok, `1` user / toolchain diagnostics, `2` usage / I/O, `101` ICE. Documented gate hooks (not for normal use): `YARROW_DEBUG_ICE=panic` (deliberate panic -> banner + `101`) and `YARROW_DEBUG_ICE=session` (`debug_trigger_ice` -> `E999` + `101`).

Gate: `cargo run -p yarrow_core --example check_ice`. Explain: `yarrow explain E999`.

---

## Modules

### `require`

```text
"path" [alias] require
```

| Form                      | Meaning                                                    |
| ------------------------- | ---------------------------------------------------------- |
| `"std.io" io require`     | Import module into scope `io` (`io.write_line`)            |
| `"std.math" require`      | Import module bindings into the **current** scope          |
| `"std.math.sqrt" require` | **Item import**: only that function into the current scope |

- Keyword last.
- `require` is allowed at top level and inside function / method bodies (scoped imports).
- Private entities in a module file are not exported.

### Resolution

1. **Std**: dotted path matches an embedded `std.*` module (from `lib/std/**/*.yar` at build time).
2. **User**: under each search path, `"a.b.c"` -> `a/b/c.yar`. The CLI adds the source file's directory.
3. **Item import**: parent-first: `"a.b.c" require` may mean function `c` in module `a.b` rather than a nested module file; function wins over module when ambiguous (`W406`).

Imported modules are parsed and compiled into the **same** JIT module as the program, so `require` imports code, not only symbols.

### Visibility and names

- Default visibility for declarations and fields is private; `public` exports.
- `main` is public by default.
- Qualified names: `alias.entity`, `Type.MEMBER`, `error.TAG`, `module.fn`.

### Compilation unit

A complete runnable unit is:

1. The entry `.yar` with `main`
2. Every module reached by `require` (transitively)
3. Linked host imports declared by the runtime registry

A require cycle (A loads B which loads A again while A is still loading) is rejected with `E382` at the `require` site that closes the cycle.

## Projects

Stage 28 product shape: an **explicit set of root sources** that share module search paths. There is no project manifest or package-manager syntax in the language.

| Piece        | Role                                                                                                |
| ------------ | --------------------------------------------------------------------------------------------------- |
| Roots        | One or more `.yar` files, each a compilation unit with its own `require` closure and optional entry |
| Search paths | Shared `module_search_paths` plus each root's directory (same rule as single-file sessions)         |
| Graph        | Union of require edges across roots; shared modules appear once in graph metadata                   |

Library API (`yarrow_core`):

- `ProjectOptions` / `ProjectRoot` / `ProjectOptions::from_root_paths`
- `check_project` / `Session::check_project` -> `CheckedProject` (`roots` + `ModuleGraph`)

Diagnostics:

| Code   | Meaning                         |
| ------ | ------------------------------- |
| `E380` | Unknown module (`require` path) |
| `E382` | Module dependency cycle         |
| `E383` | Missing / empty project root    |

Single-file `Session::check_source` and nested `require` are unchanged. Drivers: `yarrow check root_a.yar root_b.yar` ([`yarrow-cli` Stage 13](../crates/yarrow-cli/PLAN.md)); LSP `initializationOptions.projectRoots` ([`yarrow-lsp` Stage 21](../crates/yarrow-lsp/PLAN.md)). Fixtures: [`docs/guides/project_examples/`](../guides/project_examples/).

## CLI compile artifacts

Driver-only hygiene ([`yarrow-cli` Stage 15](../crates/yarrow-cli/PLAN.md)). Not a package manifest.

| Output                                       | Default path                    | Notes                                               |
| -------------------------------------------- | ------------------------------- | --------------------------------------------------- |
| Relocatable object (`compile --emit object`) | `./<stem>.o` in the process cwd | `-o PATH` overrides; only that path is the artifact |
| Linked executable (`compile --emit exe`)     | `./<stem>` in the process cwd   | Same `-o` rule                                      |
| JIT (`--target jit`)                         | (none)                          | In-process; nothing recorded                        |

Successful object/exe writes append the path to `.yarrow-build/artifacts` (created under the cwd). `yarrow clean` deletes **only** paths listed there, then removes the empty manifest / directory when possible. It never globs `*.o` across the tree. Objects that predate recording, or were written outside this CLI, stay until listed (or removed by hand).

## Session probes (Stages 30 / 35)

After a successful `Session::check_source`, [`CheckedProgram`](../crates/yarrow-core/src/session.rs) retains a [`TypeIndex`](../crates/yarrow-core/src/analysis.rs) and a [`DefIndex`](../crates/yarrow-core/src/analysis.rs) of root-file sites collected during check-only lowering (no JIT / object product).

| API                                     | Role                                                                  |
| --------------------------------------- | --------------------------------------------------------------------- |
| `CheckedProgram::type_at(offset)`       | Innermost typed site whose span contains the byte offset              |
| `TypeProbe::ty`                         | Resolved binding type string (`i32`, `list<i32>`, ...)                |
| `TypeProbe::signature`                  | Function summary plus `stack: [...] -> [...]` when on a function name |
| `CheckedProgram::definition_at(offset)` | Innermost definition / `require` site at the offset (Stage 35)        |
| `DefProbe::def_span`                    | Definition name span (use sites point back to the binding)            |
| `DefProbe::path`                        | Root source path, or resolved module dotted path for requires         |
| `DefProbe::file_path`                   | On-disk `.yar` when the loader found one; else `None`                 |
| `DefProbe::kind`                        | `Definition` or `Require`                                             |

Misses (whitespace, comments, unindexed code) return `None`. Never fabricates module paths. Required-module _bodies_ are not indexed into the root probe (root-only); LSP may still walk `require` / project graphs for multi-file navigation.

Examples:

- On `docs/examples/valid/03_variables_and_typeof.yar`, `type_at` / `definition_at` on `answer` yield `ty = Some("i32")` and a non-empty `def_span` in that file.
- On `docs/examples/valid/12_modules.yar`, `definition_at` on the `greet` alias (or the `"helpers.greet"` path string) yields `kind = Require`, `path = "helpers.greet"`, and `file_path` pointing at `helpers/greet.yar` when that file is on a search path.

## Warnings (Stage 20 / 31 / 38)

Successful `check_source` / `compile` may still populate [`CheckedProgram::warnings`](../crates/yarrow-core/src/session.rs). Warnings never fail the Session `Result`. Codes are explained via `explain_code` / CLI `yarrow explain`.

| Code   | Meaning                                                                                         |
| ------ | ----------------------------------------------------------------------------------------------- |
| `W401` | Unused `const` / `mutable` / `static` binding                                                   |
| `W402` | Unused `require`                                                                                |
| `W403` | Value left on the stack and discarded at scope exit                                             |
| `W404` | Scalar / enum `mutable` read but never `set` / `move`d into                                     |
| `W405` | Redundant parameter `copy` on a non-heap type                                                   |
| `W406` | `require` path is both a nested module and a parent-module function (function wins)             |
| `W407` | Statement after divergent control flow (`return`, both-`if` returns, `loop.break` / `continue`) |
| `W408` | Empty `match` case arm (`case ... end` with no statements)                                      |
| `W409` | Empty `if` then branch                                                                          |
| `W410` | Empty `unsafe ... end` block                                                                    |

Fixtures: [`docs/examples/valid/16_*`+`21_*`](examples/valid/). Gate: `cargo run -p yarrow_core --example check_warnings`.

## Interaction with memory and types

| Concern                                           | Doc                                  |
| ------------------------------------------------- | ------------------------------------ |
| What may sit on the stack / coerce                | [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md)   |
| Who frees handles; borrows; regions; `pointer<T>` | [`MEMORY_MODEL.md`](MEMORY_MODEL.md) |
| AST shape of `call`, `handle`, `require`          | [`AST.md`](AST.md)                   |

Runtime invariants the implementation must keep:

- Kind codes and free recursion stay in sync between compiler and host.
- Error tags are interned per program so `==` and envelope propagation agree.
- Double free of a handle (region free then variable drop) is a no-op at the host.
- Unsafe host functions are unreachable from safe contexts.

When spec and code disagree, follow the grammar and these docs; close gaps via the crate `PLAN.md` stages.
