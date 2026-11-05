# yarrow-core PLAN.md

Roadmap for closing the gaps between `docs/GRAMMAR.md` (with the EBNF in
`docs/SYNTAX.md`) and the interpreter in this crate. The previous plan's
stages are complete and were removed; this plan holds the findings of the
third spec audit. That audit compared the spec against
`src/interpreter/mod.rs`, `src/checker/mod.rs`, and `src/parser/mod.rs`, and
verified every finding with a probe program (run from `/tmp/yarrow-audit3/`;
recreate the probes from the acceptance criteria if that directory is gone).

This plan supersedes one recorded decision of the previous plan: Stage 2 of
the second audit kept type-level privacy unenforced ("type-level privacy
stays as it is"). Stage 4 below reverses that decision; the spec sentence
"Private top-level entities stay file-local and are not exported" applies to
types, and the two std types that importers name become `public`.

## Deferral policy

This plan has no deferrals, explicitly. Every task must close its gap
completely in the stage that owns it:

- An `E393` "interpreter does not support ... yet" outcome is not acceptable
  for any item below. Where a spec-valid program can reach an `E393` today
  (the module-qualified type name in Stage 4, the out-of-scope alias in
  Stage 6), the task replaces it with a real diagnostic or a real feature.
  `E393` remains allowed only for internal invariants that no spec-valid
  program can reach (for example an array as the entry return value).
- Where the spec is ambiguous, the stage records one decision and implements
  it; it does not park the feature behind a diagnostic. Stage 1 (what `dup`
  may duplicate) and Stage 5 (what counts as an unsafe context) record such
  decisions.
- Where a rule needs a new diagnostic code, the code is reserved here and
  must be added to the explain catalog (`src/diagnostics/explain.rs`) in the
  same task. Reserved codes (verified unused today): `E398` `E399`.

Status legend:

- `[ ]` todo
- `[~]` in progress
- `[x]` done

## Table of contents

- [Stage 1: `dup` duplicates the value, not the producer](#stage-1-dup-duplicates-the-value-not-the-producer)
- [Stage 2: Empty container literals require a typed context](#stage-2-empty-container-literals-require-a-typed-context)
- [Stage 3: Condition words may consume prior stack values](#stage-3-condition-words-may-consume-prior-stack-values)
- [Stage 4: Type-level privacy and module-qualified type names](#stage-4-type-level-privacy-and-module-qualified-type-names)
- [Stage 5: Unsafe operations need an unsafe block, also inside unsafe functions](#stage-5-unsafe-operations-need-an-unsafe-block-also-inside-unsafe-functions)
- [Stage 6: Diagnostic and driver quality](#stage-6-diagnostic-and-driver-quality)
- [Verification](#verification)

## Stage 1: `dup` duplicates the value, not the producer

The spec says `dup` is for copy types only ("copy types only; non-copy → use
borrow") and its stack comment is `[42] → [42, 42]`: one value becomes two
copies of that value. Two violations exist today, both caused by the parser.

When the word before `dup` is "dupable", the parser clones the previous
expression at parse time instead of emitting `StackOp::Dup`
(`src/parser/mod.rs`, `TokenKind::Dup` arm, with `is_dupable_word` at the
bottom of the file). That word list includes effectful and non-copy
expressions:

- `Call` and `Unwrap`: `make call dup` evaluates `make` twice. A probe with
  a fallible `make` left two error envelopes (four stack values) where the
  spec promises one duplicated value.
- `String`, `List`, `Map`, `StructLit`, `EmptyMapOrStruct`: `"x" dup` and
  `(1 2 3) dup` never reach the checker's `E371` rule
  (`check_stack_op`, `src/checker/mod.rs`), so the non-copy restriction is
  not enforced for literals. Each clone re-evaluates into a separate value,
  which is memory-safe but spec-illegal.
- `Variable` and `Borrow`: the clone re-reads the variable, which for a
  non-copy binding is a second borrow and fails with the wrong code
  (`E375` second live borrow instead of `E371`).

Recorded implementation decisions (Stage 1):

- The parse-time clone survives only for words that are statically copy
  values: integer, float, bool, and rune literals, and type values. Every
  other `dup` emits `Expr::StackOp(StackOp::Dup)` so the checker sees it.
  The postfix statement grouping the clone was protecting (`i dup 1 + set`)
  must keep working; if the clone was load-bearing for grouping, regroup
  without re-executing the word.
- `dup` on a borrow or reference stays legal. The spec's non-copy list is
  "string, list<T>, hashmap<K V>, unions, structs"; references are not in
  it, and the grammar tour's union arm idiom (`dup *` on the
  `reference<Member>` the arm receives) requires duplicating a reference.
  The runtime already clones a reference unowned, which is safe: releasing
  the clone is a no-op and the original borrow releases normally.
- `dup` on an owned heap value (string, list, map, struct, union kinds) is
  `E371`. The checker's `Owned` arm already emits it; the runtime `Dup` arm
  gains the same rejection as the backstop for units the checker cannot
  model (module units, the REPL).

- [x] 1.1.1 Restrict the parse-time clone to the copy literal words
  (integer, float, bool, rune, type value); every other `dup` emits
  `StackOp::Dup`. Keep `i dup 1 + set` on a copy variable parsing and
  running exactly as today.
- [x] 1.1.2 In `eval_stack_op` (`StackOp::Dup`), reject an owned heap value
  on top with `E371` and the checker's message, instead of cloning the
  handle unowned.
- [x] 1.2.1 In the checker's `check_stack_op` `Dup` arm, keep `E371` for
  `Slot::Owned`; for `Slot::Borrow`, stop emitting `E371` and push the same
  borrow without re-registering it (no `E375` cascade), matching the
  decision above.
- [x] 1.2.2 Check the `E371` explain entry covers both the check-time and
  runtime paths; update its wording if it names only one.

Acceptance: `make call dup` calls `make` exactly once and the duplicated top
value equals the original; `"x" dup`, `(1 2 3) dup`, and `{x 1} dup` fail
with `E371`; `42 dup`, `3.14 dup`, `true dup`, and `'a' dup` still work;
`i dup 1 + set` on a mutable `i32` still sets `i` to `i + 1`; the grammar
tour's union arm `dup *` still checks and runs clean; every file in
`docs/examples/valid/` still passes.

## Stage 2: Empty container literals require a typed context

The spec: "Empty literals have no element type until a typed context (e.g. a
variable declaration) supplies one. Empty literals need a typed binding (or
other typed context) before use." Only `{}` follows it: the interpreter
rejects a bare `Expr::EmptyMapOrStruct` with `E395`
("empty `{}` requires a typed context"). An empty `()` or `[]` parses as a
`List`/`Array` expression with no elements and evaluates into a fresh
container with a default element kind, so `() list.len call` prints `0` and
`[] drop` runs, with no diagnostic anywhere.

Recorded implementation decisions (Stage 2):

- The typed contexts are the binding forms: the initializer of a
  `mutable`/`const`/`static` declaration whose type is `list<T>`,
  `array<T N>`, or `hashmap<K V>`. This matches the spec's own examples and
  the corpus (17_empty_placeholders).
- A bare empty `()`/`[]` anywhere else (a lone statement, an operand, a
  `dup` source, a call argument) is `E395`. Call arguments are not a typed
  context: calls are opaque to both passes and the spec names none.
- The `E395` message and explain entry generalize to name all three empty
  literal forms, not only `{}`.

- [x] 2.1.1 Reject an empty `()`/`[]` that is not the direct initializer of
  a binding with a declared container type: in the checker for the main
  unit, and in the interpreter for every unit (module bodies and the REPL
  included), with `E395`.
- [x] 2.1.2 Update the `E395` message and its explain entry to cover `()`,
  `[]`, and `{}`.

Acceptance: `() list.len call` and `[] drop` fail with `E395`;
`() l const list<i32>`, `[] a const array<i32 0>`, and
`{} m const hashmap<string i32>` still bind; 17_empty_placeholders and the
rest of the corpus still pass.

## Stage 3: Condition words may consume prior stack values

The EBNF models the words before `if` as ordinary statements ("condition
bool already on the stack", `if_stmt = "if", { statement }, ...`), so a
condition may consume values left by earlier statements, exactly like any
other stack words. The interpreter instead folds the preceding words into
one condition expression and requires them to leave exactly one value over
the pre-`if` height (`eval_cond`, `src/interpreter/mod.rs`: the
`stack.len() != before + 1` check). Spec-valid programs fail with `E324`
"condition must evaluate to a single value":

- `risky call handle 0 fallback end 0 == if ... end`: the `handle`
  statement is not foldable, so the condition `0 ==` consumes the payload
  the handle left, net zero.
- `p borrow p.x 5 == if ... end`: the folded words include the `borrow`,
  which stays on the stack under the bool, net greater than one.

The same folding feeds the `for` source words, so the restriction applies
there too.

Recorded implementation decisions (Stage 3):

- Values the condition words leave below the bool stay on the stack; the
  branch (or loop body) runs with them. The `if` handler's old clear back
  to the pre-condition height was a no-op under the exact-height rule and
  would have dropped those values, so it is gone.
- The `for` body's balance check (`E325`) and per-iteration clears are
  relative to the height the source words left the stack at, not the
  pre-`for` height, in both loop forms. A condition-form loop still
  re-evaluates its source before every iteration, so a source that consumes
  values from before the `for` runs out of operands with the underflow
  diagnostic (`E362`) once they are gone.

- [x] 3.1.1 In `eval_cond`, drop the height requirement. After evaluating
  the condition words the stack must be non-empty (an empty result keeps
  the existing underflow diagnostic) and the top must be `bool`; pop it. A
  non-bool top keeps `E324` with the "condition must be bool" message.
- [x] 3.1.2 Apply the same model to the `for` source words (the runtime
  bool-versus-iterable dispatch): the source words may consume prior stack
  values; only the popped top decides the loop form.
- [x] 3.1.3 Match case conditions keep their subject-relative behavior
  (words before `case` operate on the borrowed subject); verify the
  grammar tour idioms (`dup 85 == case`, `dup Color.RED == case`) are
  unchanged.

Acceptance: `risky call handle 0 fallback end 0 == if ... end` runs;
`p borrow p.x 5 == if ... end pop` runs; `5 10 < if` and
`counter 5 < for` behave exactly as before; `1 2 if` fails (top not bool);
`1 if` fails; every file in `docs/examples/valid/` still passes.

## Stage 4: Type-level privacy and module-qualified type names

Two related gaps in the type registry:

- Privacy is not enforced for types. `register_struct`, `register_enum`,
  `register_union`, and `register_error` record field visibility but never
  the type's own visibility, and no use site checks it. A module's
  `Point private struct` binds fine in the main unit by bare name,
  although the spec says private top-level entities "stay file-local and
  are not exported". (Stage 2 of the previous audit enforced this for
  functions only.)
- Module-qualified type names are unsupported. The EBNF allows
  `type = ... | qualified_name`, so `m.Point` is spec-valid syntax, but a
  binding of that type fails with `E393` "interpreter does not support
  binding type ... yet".

Recorded implementation decisions (Stage 4):

- Types stay globally unique by bare name (`E384` from the previous plan is
  unchanged); privacy controls nameability, not namespace separation. A
  private type is usable only inside its own unit; a public type is
  nameable by importers both bare and through its module alias.
- The use sites that must check visibility are: a binding's declared type,
  struct/union/enum literals, `Type implement`, `typeof` comparisons, and
  match type cases. Same-unit use of a private type stays legal.
- The two std types that importers name today become `public`: `File` in
  `lib/std/fs.yar` (the grammar tour binds `{fd -1} failed const File`) and
  `Error` in `lib/std/error.yar` (signatures name `error.Error` across
  units). Default visibility is private, so without this they would break.
- A qualified name that resolves to no alias, or whose alias holds no such
  type, is `E340` (unknown type), not a new code. A resolved but private
  type is the new `E398`.

- [ ] 4.1.1 Record the declaring unit and visibility for every registered
  type (struct, enum, union, error), mirroring what `StructInfo` already
  records per field.
- [ ] 4.1.2 Enforce privacy at the use sites listed above: a private type
  used from a different unit fails with `E398`; same-unit use is legal.
  The checker covers the main unit; the interpreter backstops module units.
- [ ] 4.1.3 Mark `File public struct` in `lib/std/fs.yar` and
  `Error public error` in `lib/std/error.yar`.
- [ ] 4.2.1 Resolve module-qualified type names (`m.Point`) through the
  alias/require tables to the module's type, replacing the `E393` binding
  rejection: public types bind, private types fail with `E398`, unknown
  names fail with `E340`.
- [ ] 4.2.2 The checker resolves qualified type names for the requires it
  can see (main unit), the same way it resolves qualified calls; module
  units stay opaque as today.
- [ ] 4.2.3 Add `E398` to the explain catalog: mark the type `public` in
  the declaring module or move the use into that module.

Acceptance: a module's private struct used by bare name from the main unit
fails with `E398`; the same type used inside its own module runs; `m.Point`
binds for a public struct and fails with `E398` for a private one; an
unknown `m.Nope` fails with `E340`; the corpus (with `File` and `Error` now
public) and the std modules still load and run.

## Stage 5: Unsafe operations need an unsafe block, also inside unsafe functions

The spec: "Even inside an unsafe function, mark the ops with an unsafe
block." Today both passes treat an `unsafe function` body as an unsafe
context: the checker increments `unsafe_depth` around the body of an unsafe
function (`src/checker/mod.rs`, the `if f.is_unsafe` pair), and `E370`'s
help text offers "or mark the enclosing function 'unsafe function'". A probe
runs `mem.allocate`/`mem.free` directly in an unsafe function's body with
no inner block.

Recorded implementation decisions (Stage 5):

- Only an active `unsafe ... end` block is an unsafe context. The
  `unsafe` marker on a function stays declarative, matching "escapes are
  visible at definition (`unsafe function`)": it documents the escape and
  callers still need a block to call it.
- Nothing in the corpus or std relies on the lenient rule: the grammar
  tour's `pointer_function`, 11_unsafe_pointers, and every function in
  `lib/std/mem.yar` already wrap the unsafe ops in inner `unsafe ... end`
  blocks.
- The rule covers what `E370` covers today: calls to unsafe functions and
  the unsafe builtins (`@alloc` and friends). The `store`/`load` words are
  not gated; the spec does not require it and this plan does not add it.

- [ ] 5.1.1 Remove the function-level `unsafe_depth` increment so an unsafe
  function's body is not an unsafe context; `E370` fires for unsafe calls
  and unsafe builtins outside an active block, including inside unsafe
  functions.
- [ ] 5.1.2 Update the `E370` message, help text, and explain entry: drop
  the "mark the enclosing function 'unsafe function'" alternative.
- [ ] 5.1.3 Mirror the rule in the interpreter's runtime backstop for
  module units.

Acceptance: `mem.allocate call` directly in an unsafe function's body fails
with `E370` pointing at the call; the same body with the ops wrapped in
`unsafe ... end` runs; 11_unsafe_pointers and 00_grammar_tour pass
unchanged; calling an unsafe function inside a block in a plain function
still runs; the std modules still load.

## Stage 6: Diagnostic and driver quality

Two small gaps found by the audit's probes; both are user-reachable today.

- [ ] 6.1.1 Out-of-scope alias: `io.write_line call` in a unit where `io`
  was required inside another function's scope reports `E393` "interpreter
  does not support method call / complex call target yet". The scoping
  behavior is correct; the diagnostic is wrong. Resolve the qualified
  call target against the visible aliases and report the first unknown
  segment with `E399` (unknown alias or module), reserving the `E393`
  fallback for targets that are syntactically valid but not yet supported.
- [ ] 6.1.2 Add `E399` to the explain catalog: the alias is not in scope
  here; require the module in this unit or use the function's own scope.
- [ ] 6.2.1 Bare relative source path: `Session::interpret_source` adds
  the source file's parent directory as a module search path only when it
  is non-empty, so `yarrow 12_modules.yar` run from inside the file's
  directory resolves no sibling modules (`E380`), while the same file by
  absolute path works. When the parent is empty, add `.` instead.

Acceptance: the out-of-scope alias probe fails with `E399` naming `io`;
running `yarrow 12_modules.yar` with the shell inside
`docs/examples/valid/` passes, as does the absolute-path invocation; the
corpus still passes.

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

Probe programs for the acceptance criteria above live in `/tmp/yarrow-audit3/`
from the audit session; recreate them from the criteria text if that
directory is gone. Per AGENTS.md, do not add automated tests; tests come
later.

When the last stage closes, refresh the corpus list in the module doc of
`src/interpreter/mod.rs` (the maintenance rule of the previous plans still
applies: every file in `docs/examples/valid/` must appear there and run
cleanly).
