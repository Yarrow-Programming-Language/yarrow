# yarrow-core PLAN.md

Roadmap for closing the gaps between `docs/GRAMMAR.md` and the interpreter in
this crate. The audit behind this plan compared the spec against
`src/interpreter/mod.rs` and verified behavior by running every file in
`docs/examples/valid/` plus targeted probe programs.

Status legend:

- `[ ]` todo
- `[~]` in progress
- `[x]` done

## Table of contents

- [Stage 1: Missing runtime features](#stage-1-missing-runtime-features)
- [Stage 2: Parsed but ignored semantics](#stage-2-parsed-but-ignored-semantics)
- [Stage 3: Compile-time checks](#stage-3-compile-time-checks)
- [Stage 4: Robustness and documentation](#stage-4-robustness-and-documentation)
- [Verification](#verification)

## Stage 1: Missing runtime features

Spec features that currently fail at runtime with `E393`, `E320`, or `E333`.

### 1.1 `loop.break` and `loop.continue`

- [x] 1.1.1 Add `Break` and `Continue` variants to the `Flow` enum in
  `src/interpreter/mod.rs` (currently only `Next` and `Return`).
- [x] 1.1.2 Handle `break` and `continue` members in the `std.loop` alias branch
  of `Expr::Member` evaluation (the branch that already handles `loop.value`
  and `loop.index`, around `eval_expr`). Return the new `Flow` variants instead
  of pushing a value.
- [x] 1.1.3 Propagate `Flow::Break` and `Flow::Continue` through `eval_body`,
  `eval_stmt`, and the `if` / `match` / `handle` bodies so the words escape
  nested blocks but stop at the innermost `for`.
- [x] 1.1.4 Consume `Flow::Break` (exit the loop) and `Flow::Continue` (jump to
  the next iteration) in both `eval_cond_for` and `eval_iter_for`, including
  the list branch that currently frees the iterable on early return.
- [x] 1.1.5 Reject `loop.break` / `loop.continue` outside a `for` body with a
  clear diagnostic (reuse the existing `E393` message style of
  `loop.value` outside a `for`).

Acceptance: a `for` loop using `loop.break` exits early and leaves the counter
variable correct; `loop.continue` skips the rest of the body; both forms work
in condition-`for` and iterable-`for`; the stack stays balanced.

### 1.2 Missing list and map intrinsics

`is_std_intrinsic` advertises five intrinsics that `eval_std_intrinsic` does
not implement, so they fail with `E393` even though `lib/std/list.yar` and
`lib/std/map.yar` document them.

- [x] 1.2.1 Implement `std.list::get`: pop an index and a list, push the
  element at that index as a borrow (`owned: false`), using
  `runtime::yarrow_list_get`. Out-of-range index must produce a user
  diagnostic, not a panic.
- [x] 1.2.2 Implement `std.list::put`: pop a value and an index and a list,
  write the element in place with the host list runtime, push the list back.
- [x] 1.2.3 Implement `std.map::len`: pop a map, push
  `runtime::yarrow_map_len` as `i64`.
- [x] 1.2.4 Implement `std.map::get`: pop a key and a map, push the value for
  that key. Decide and document the missing-key behavior (error tag versus
  default value) before implementing.
- [x] 1.2.5 Implement `std.map::put`: pop a value and a key and a map, insert
  with `runtime::yarrow_map_insert`, push the map back.
- [x] 1.2.6 Keep ownership handling consistent with `std.list::push_last`:
  heap bits transferred into the container are not freed by the stack slot.

Acceptance: each intrinsic runs on `list<i32>`, `list<string>`, and
`hashmap<string i32>` probes; no `E393` remains for the names listed in
`is_std_intrinsic`.

### 1.3 String equality

- [x] 1.3.1 Add string cases to `eval_bin` for `Eq` and `Ne`: compare through
  `runtime::yarrow_str_cmp` (or a direct byte compare), free owned operands,
  push a `bool`. Mirror the ownership handling of the existing `Concat` arm.

Acceptance: `"a" "a" ==` pushes `true`, `"a" "b" !=` pushes `true`, and neither
leaks nor double-frees when one side is an owned temporary.

### 1.4 `typeof` against named types

- [x] 1.4.1 Extend `Expr::TypeValue` evaluation so a named type resolves
  through the interpreter tables: struct ids, union ids, enum ids, and error
  type ids, reusing the logic of `type_kind_code` for `TypeKind::Named`.
- [x] 1.4.2 Keep the primitive path unchanged so `myVar typeof i32 ==` keeps
  working.

Acceptance: `{x 1} p const Point` then `p typeof Point ==` pushes `true`; the
same holds for a union and an enum type.

## Stage 2: Parsed but ignored semantics

Constructs the parser accepts and records in the AST, but the interpreter
silently ignores.

### 2.1 Enum underlying types

- [ ] 2.1.1 Honor `EnumDecl.underlying` in `register_enum`. Decide the
  runtime story per carrier first: integer carriers keep discriminants of that
  width; `string` carrier needs a representation decision (interned handles
  versus discriminants plus a side table).
- [ ] 2.1.2 Store the carrier in `StructInfo`-style metadata so `typeof` and
  member access agree on the kind code.
- [ ] 2.1.3 If a carrier is deferred, reject the declaration with a clear
  `E393` instead of silently treating it as `i32`.

Acceptance: `Color string enum` either behaves as a string-carried enum or
fails with an explicit unsupported-carrier diagnostic; it never silently
behaves as an `i32` enum.

### 2.2 Error type injection

- [ ] 2.2.1 Honor `ErrorDecl.inject` in `register_error`: copy the members of
  the injected error type into the new type before interning local members.
- [ ] 2.2.2 Keep tags program-unique (the `error_tags` interning already
  guarantees this) so `MyErr.OUT_OF_MEMORY` and `error.OUT_OF_MEMORY` resolve
  to the same tag.
- [ ] 2.2.3 Remove the accidental global-by-member-name fallback in the
  `error.MEMBER` lookup path once injection is real, so a typo cannot resolve
  through an unrelated error type.

Acceptance: after `"std.error" error require`, `MyErr error.Error error MINE
end` makes `MyErr.OUT_OF_MEMORY`, `MyErr.MINE`, and `error.OUT_OF_MEMORY` all
resolve; an unknown member fails.

### 2.3 `T copy` parameter modifier

- [ ] 2.3.1 Act on `ParamModifier::Copy` in `call_function`: deep-copy heap
  arguments (string, list, map, struct, union) before binding them, so the
  callee cannot mutate the caller's value.
- [ ] 2.3.2 Leave the default (no modifier) as the current borrow-on-read
  behavior, matching the grammar's "parameters move onto the local stack".
- [ ] 2.3.3 Decide and document the interaction with `mutable` receivers
  (`reference<T> mutable`), which must keep mutating the pointee.

Acceptance: a function declared with `list<i32> copy` that pushes into its
parameter leaves the caller's list unchanged.

### 2.4 `f16` float literals

- [ ] 2.4.1 Extend `float_literal_kind` to return the `f16` kind code (12)
  when the value is exactly representable as `f16`, then `f32`, then `f64`,
  matching the grammar's "floats take the smallest fitting float".
- [ ] 2.4.2 Check every consumer of the kind code (arithmetic result kinds,
  `typeof`, container element sizing in `kind_elem_size`) still behaves with
  kind 12; `kind_elem_size` already maps 12 to 2 bytes.

Acceptance: `3.14 typeof f16 ==` pushes `true`; `1.5 typeof f16 ==` pushes
`true`; `3.14159265358979 typeof f64 ==` pushes `true`.

### 2.5 `defer` at block scope exit

- [ ] 2.5.1 Run deferred bodies at the exit of the innermost scope that
  registered them: function body, and `if` / `else` / `for` / `match` case /
  `handle` bodies, in reverse registration order.
- [ ] 2.5.2 Move the deferred registry from `Frame` (function-wide) to a
  per-scope stack, or track a scope depth alongside each registered body.
- [ ] 2.5.3 Preserve the current function-exit behavior for defers
  registered directly in the function body, including the fallible-return
  path in `call_function` and the `run_deferred` ordering.

Acceptance: a `defer` inside an `if` body runs when the `if` ends, before
statements after the `if`; a `defer` in the function body still runs last,
after block-scoped defers.

### 2.6 Function-scoped `require`

- [ ] 2.6.1 Make `require` statements inside a function body bind their
  alias only for that function, per the grammar's "require inside a function
  only affects that function's scope".
- [ ] 2.6.2 Keep top-level requires global. Load each module once (the
  loader cache already deduplicates by path).
- [ ] 2.6.3 Store per-function aliases on the function entry or a scope stack
  consulted by `resolve_call_name`, instead of only the global `aliases` map.

Acceptance: two functions requiring the same alias for different modules each
resolve their own bindings; a top-level require stays visible everywhere.

## Stage 3: Compile-time checks

`yarrow-core` has no checker pass: the session pipeline is tokenize, parse,
interpret. The interpreter comments assume a checker exists ("Checker already
enforced the unsafe boundary"), so these spec rules are enforced nowhere.
Decide first whether the checks belong in a new checker module run by
`Session::interpret_source` between parsing and interpretation, or as
interpreter-side validation. A checker module is the better fit because all
of these are static rules.

- [ ] 3.0 Add the checking pass skeleton: a module that walks the `Program`
  AST with access to registered declarations (structs, enums, unions, error
  types, functions) and emits `DiagnosticBatch` entries, wired into
  `Session::interpret_source` after `parse_source`.

### 3.1 Borrow rules

- [ ] 3.1.1 Reject a second live borrow of the same value (single-borrow
  rule). Track borrows from the `borrow` word and from non-copy variable
  reads; a borrow ends at `pop`, at consumption by a call, or at scope exit.
- [ ] 3.1.2 Reject `drop` / `pop` of an owner while a borrow of its value is
  live.

### 3.2 Move rules

- [ ] 3.2.1 Reject any use of a variable after `move` transferred its value.
- [ ] 3.2.2 Reject `move` of a variable that has a live borrow.

### 3.3 Copy rules

- [ ] 3.3.1 Reject `dup` on non-copy types (string, list, hashmap, unions,
  structs); the grammar restricts `dup` to copy types and points to `borrow`
  for the rest.

### 3.4 Static variables

- [ ] 3.4.1 Reject a `static` declaration whose initializer is not a
  compile-time constant (literals and constant expressions only; no variable
  reads, no calls).

### 3.5 Unsafe boundary

- [ ] 3.5.1 Reject calls to `unsafe function`s outside an `unsafe ... end`
  block.
- [ ] 3.5.2 Reject unsafe operations (`@alloc`, `@free`, `@load`, `@store`,
  `load`, `store` on `pointer<T>`, `pointer` arithmetic) outside an unsafe
  block or an unsafe function body.
- [ ] 3.5.3 Keep borrow and ownership checks active inside unsafe blocks, as
  the grammar requires.

Acceptance for stage 3: each rule rejects a small probe program with a
diagnostic pointing at the offending word, and every file in
`docs/examples/valid/` still passes without new diagnostics.

## Stage 4: Robustness and documentation

### 4.1 Division and modulo by zero

- [ ] 4.1.1 Replace the bare `wrapping_div` / `wrapping_rem` calls in
  `eval_int_bin` with a zero check that raises a user diagnostic (new code in
  the `E33x` range) instead of the current ICE surfaced by the Rust panic.

Acceptance: `1 0 //` and `1 0 %` produce an error diagnostic, not an internal
compiler error.

### 4.2 Stale interpreter documentation

- [ ] 4.2.1 Update the module doc of `src/interpreter/mod.rs`: remove the
  claim that `00_grammar_tour.yar` is interpret-out-of-scope (it runs cleanly
  now) and refresh the corpus list, which names 15 of the 24 current valid
  examples.
- [ ] 4.2.2 Re-check this list whenever an example is added or a gap is
  closed.

### 4.3 Embedded std modules are unused

- [ ] 4.3.1 Either wire the `STD_MODULES` table generated by `build.rs` into
  `ModuleLoader` as a built-in fallback search source, so `std.*` resolves
  without `-L`, or remove the build script and document `-L` as the only way.
  Wiring the fallback is preferred: the AGENTS.md workflow
  (`cargo run -- <file.yar>`) currently fails with `E380` for any program
  that requires std.
- [ ] 4.3.2 Keep explicit `-L` paths taking precedence over embedded
  modules, so local overrides stay possible.

Acceptance: `cargo run -- docs/examples/valid/01_hello.yar` succeeds with no
`-L` flag.

## Verification

After each task or stage:

```bash
cargo fmt --all
cargo check
cargo clippy
cargo run -- -L crates/yarrow-core/lib docs/examples/valid/00_grammar_tour.yar
```

And run the whole corpus:

```bash
for f in docs/examples/valid/*.yar; do
	cargo run -q -- -L crates/yarrow-core/lib "$f" || echo "FAIL: $f"
done
```

Probe programs for the acceptance criteria above live in `/tmp/yarrow-audit/`
from the audit session; recreate them from the criteria text if that
directory is gone. Per AGENTS.md, do not add automated tests; tests come
later.
