# yarrow-core PLAN.md

Roadmap for closing the gaps between `docs/GRAMMAR.md` and the interpreter in
this crate. The previous plan's stages are complete and were removed; this
plan holds the findings of the second spec audit. That audit compared
`docs/GRAMMAR.md` against `src/interpreter/mod.rs`, `src/checker/mod.rs`, and
`src/parser/mod.rs`, and verified every finding with a probe program
(run from `/tmp/yarrow-audit2/`; recreate the probes from the acceptance
criteria if that directory is gone).

## Deferral policy

This plan has no deferrals. Every task must close its gap completely:

- An `E393` "interpreter does not support ... yet" outcome is not acceptable
  for any item below. In particular, the string enum carrier, which the
  previous plan deferred, is implemented here (Stage 3).
- Where the spec is ambiguous, the stage records one decision and implements
  it; it does not park the feature behind a diagnostic.
- Where a rule needs a new diagnostic code, the code is reserved here and
  must be added to the explain catalog (`src/diagnostics/explain.rs`) in the
  same task. Reserved codes (verified unused today): `E310` `E311` `E312`
  `E313` `E378` `E379` `E384` `E385` `E386` `E396` `E397`.

Status legend:

- `[ ]` todo
- `[~]` in progress
- `[x]` done

## Table of contents

- [Stage 1: Runtime arithmetic and loop semantics](#stage-1-runtime-arithmetic-and-loop-semantics)
- [Stage 2: Module privacy and type namespacing](#stage-2-module-privacy-and-type-namespacing)
- [Stage 3: Enum carriers and typeof](#stage-3-enum-carriers-and-typeof)
- [Stage 4: Fallible signature detection](#stage-4-fallible-signature-detection)
- [Stage 5: New static rules](#stage-5-new-static-rules)
- [Stage 6: Static type checking](#stage-6-static-type-checking)
- [Verification](#verification)

## Stage 1: Runtime arithmetic and loop semantics

### 1.1 Floor division and remainder

The spec defines `//` as floor division. `eval_int_bin` currently uses
`wrapping_div` / `wrapping_rem`, which truncate toward zero, so `-7 2 //`
pushes `-3` instead of `-4` and `-7 2 %` pushes `-1` instead of `1`.

- [x] 1.1.1 Change the `Fdiv` arm of `eval_int_bin` to floor semantics:
  quotient rounded toward negative infinity, with the same `i64` wrapping
  behavior as today on overflow.
- [x] 1.1.2 Change the `Mod` arm so the remainder is consistent with floor
  division: `a == (a // b) * b + a % b` must hold for all inputs, which means
  the remainder takes the divisor's sign.
- [x] 1.1.3 Keep the existing division-by-zero diagnostics (`E337`) and their
  messages unchanged.

Acceptance: `-7 2 //` pushes `-4`, `-7 2 %` pushes `1`, `7 2 //` pushes `3`,
`10 3 %` pushes `1`, and `1 0 //` and `1 0 %` still raise `E337`.

### 1.2 `for` over a bool value on the stack

The spec says "for with a bool on top ≈ while". Today the condition-versus-
iterable choice is made syntactically by `for_source_is_condition`, so a
comparison (`counter 5 < for`) loops but a bool variable (`go for`) or a call
pushing a bool fails with `E394`.

- [x] 1.2.1 Replace the syntactic test in `eval_stmt` (`StmtKind::For`) with
  a runtime dispatch: evaluate the source expression once; if the popped
  value is a `bool`, run the condition-loop form, re-evaluating the source
  expression before every iteration (the source may be a variable that the
  body reassigns, or a call); if it is an array or list, run the iterable
  form on the already-popped value.
- [x] 1.2.2 Keep `E394` for sources that are neither bool nor iterable, and
  keep the stack-balance (`E325`) and `loop.break` / `loop.continue`
  behavior identical in both forms.
- [x] 1.2.3 Remove `for_source_is_condition` once nothing calls it, so the
  syntactic special case cannot regress.

Acceptance: a loop `go for` over a bool variable that the body sets to
`false` terminates with the counter correct; `counter 5 < for` and
`numbers for` behave exactly as before; `[1 2 3] for` with `loop.break`
still exits early.

### 1.3 Array size inference

The spec says the array size `N` "may be inferred from a non-empty literal".
`array<i32>` parses with `size: None` today, but the binding records count
`0` in the slot kind, discarding the literal's length.

- [x] 1.3.1 In the `VarDecl` binding path, when the declared type is
  `array<T>` with no size and the initializer is a non-empty array literal,
  record the literal's element count as the array's `N` in the bound slot's
  kind code (the `count << 40` field).
- [x] 1.3.2 An explicitly written `N` always wins over inference, and an
  empty literal with no declared `N` is rejected with a clear diagnostic
  (there is nothing to infer from). The diagnostic is `E381` (assigned by
  this task; added to the explain catalog with it).
- [x] 1.3.3 Inference applies to `mutable`, `const`, and `static` bindings
  alike; the `static` constant rule (E377) is unaffected because array
  literals are constant words.

Acceptance: `[10 20 30] a static array<i32>` binds and iterates three
elements; the recorded kind carries count `3`; `[10 20 30] b const array<i32 2>` still fails or coerces exactly as it does today (declared size
wins); `[] c const array<i32>` produces the new no-inference diagnostic.

## Stage 2: Module privacy and type namespacing

### 2.1 Private functions must be callable inside their own module

`register_unit` drops non-public functions of module units entirely, so a
module's public function cannot call its own private helper (the call fails
with `E330 unknown function`). The spec says private entities "stay
file-local and are not exported": callable within the file, invisible
outside it.

- [x] 2.1.1 Register every function of a module unit in `funcs` under its
  module-qualified name, regardless of visibility.
- [x] 2.1.2 Keep the export filters as they are: only `public` functions are
  bound into alias scopes (`aliases`), plain-name imports (`plain_funcs`),
  and the per-function `local_plain_funcs` tables. A private module function
  must not be reachable as `alias.name call` or `name call` from another
  unit.
- [x] 2.1.3 Restrict item imports (`"mod.item" require`) to `public`
  functions: `resolve_require` must check the target function's visibility
  in the parent module and reject a private item with a diagnostic instead
  of binding it.
- [x] 2.1.4 `register_implement` follows the same rule for methods: private
  methods register and are callable inside their module, but are not
  exported.

Acceptance: a module whose public function calls a private helper runs;
`pm.double call` from the main unit (private helper) fails with `E330`;
`"mod.double" require` for a private `double` is rejected at load time;
every file in `docs/examples/valid/` still passes.

### 2.2 Duplicate type names across units are rejected

Structs, enums, unions, and error types register globally by bare name and a
second registration of the same name is silently skipped. Two modules that
each declare a private `Point` with different fields produce a confusing
`E340` (or a wrong layout) instead of a name-collision diagnostic.

- [x] 2.2.1 In `register_struct`, `register_enum`, `register_union`, and
  `register_error`, replace the silent `contains_key` early return with an
  `E384` diagnostic that names the type, the unit that declared it first,
  and the unit that redeclares it.
- [x] 2.2.2 The main unit registers first, then modules in require order
  (the existing order in `load_program`), so the diagnostic always blames the
  later unit.
- [x] 2.2.3 Add `E384` to the explain catalog with guidance: rename one of
  the types or require the module under an alias.

Decision (recorded, no code): type-level privacy stays as it is. The grammar
tour's defer section binds `{fd -1} failed const File` with `File` coming
from `std.fs`, so module types must remain nameable by importers; the spec's
"private top-level entities stay file-local" sentence is satisfied for
types by the uniqueness rule above plus the field-visibility rule (5.4).
Functions are the one category where the spec's privacy sentence maps to
hiding, and 2.1 implements it.

Acceptance: two modules declaring the same struct name produce `E384` with
both unit names; the corpus and the std modules (which declare `File`,
`Error`, and others) still load cleanly.

## Stage 3: Enum carriers and typeof

### 3.1 `typeof` on an enum value reports the enum, not the carrier

Enum values are stored as their carrier integer, and the slot kind is the
carrier kind, so `c typeof i32 ==` is `true` for a default enum. The spec
says `typeof` pushes the value's static type; for an enum value that is the
enum type, distinct from its carrier.

- [x] 3.1.1 Give enum values a kind code that identifies the enum itself
  (for example a dedicated tag with the enum id in the high bits, mirroring
  `KIND_STRUCT`), while keeping the stored bits as the carrier value so
  container and field layouts do not change size.
- [x] 3.1.2 Update `named_type_kind_code` so the type value `Color` pushes
  the enum's own kind code, making `c typeof Color ==` push `true` and
  `c typeof i32 ==` push `false`.
- [x] 3.1.3 Update every consumer of the raw carrier kind on enum slots:
  struct field layout (fields stay carrier-sized), list and map element
  kinds, `dup` (enum is a copy type), integer operators on enum values
  (comparisons such as `dup Color.RED ==` must keep working), and
  `bits_to_slot` round-trips through containers.
- [x] 3.1.4 `typeof` on an enum value still releases a borrow exactly as it
  does today (the `apply_typeof` ownership path is unchanged).

Acceptance: `Color.RED c const Color` then `c typeof Color ==` pushes
`true`, `c typeof i32 ==` pushes `false`, `c dup Color.RED ==` pushes
`true`, and an enum stored in a struct field or a list still reads back
correctly.

### 3.2 String-carried enums

The spec presents `string` as an example carrier ("Write `Name <type>
enum` for another carrier (e.g. string)"). The previous plan deferred this;
per the deferral policy it is implemented now.

Decision: a string carrier stores each member's name as a heap string. The
member access `Color.RED` pushes the member's string value; equality between
enum values goes through the existing string `==`; `typeof` reports the
enum's own kind code (3.1). Integer carriers keep discriminants exactly as
today.

Recorded implementation decisions (3.2):

- Member access pushes an owned clone of the interned member string, not
  the interned handle itself, so container storage (struct fields, list
  elements, map values) can free its copy without dangling the interned
  handle on the next access.
- The host runtime learns each enum's carrier kind at registration
  (`yarrow_register_enum_carrier`), so `free_value` and `yarrow_value_clone`
  free and clone enum-typed fields and elements through the carrier
  (string handle vs inline scalar bits).
- The checker models string-enum member access as an owned heap temp, so
  `dup` on the value is the non-copy `E371`, exactly as for strings.
  Bindings of enum type keep the checker's existing copy modeling so the
  grammar tour's match idiom (`dup Color.RED == case`) keeps checking
  clean; at runtime a `dup` of such a binding pushes an unowned clone,
  which is safe.
- Explicit member values on a string carrier are rejected with `E363`
  (the carrier stores member names; there is nothing to assign).

- [x] 3.2.1 Extend `register_enum` and `enum_carrier_kind` to admit the
  string carrier: intern one heap string per member at registration, store
  the member-to-handle table in the enum's metadata, and keep the
  discriminant table for integer carriers unchanged.
- [x] 3.2.2 Member access (`Expr::Member` on an enum type) pushes the
  member's string handle for string-carried enums, with the enum's kind code
  from 3.1 on the slot.
- [x] 3.2.3 Storage and ownership: a string-carried enum value is a heap
  handle, so struct fields, list elements, and map values of that type store
  the 8-byte handle and free it through the string free path; `dup` on a
  string-carried enum value is rejected by the existing non-copy `dup` rule
  (E371) since the value is a heap string, unlike integer-carried enums
  which stay copy values.
- [x] 3.2.4 `match` on a string-carried enum works through string equality
  (`dup Color.RED == case`), and `typeof` reports the enum.
- [x] 3.2.5 Carriers other than the integer primitives and `string` (floats,
  named types) are rejected at registration with `E363` and a message that
  names the carrier. This is a spec boundary, not a deferral: the spec only
  defines integer and string carriers.

Acceptance: `Color string enum` with `RED` and `GREEN` declares; `Color.RED
c const Color` binds; `c typeof Color ==` pushes `true`; `c Color.RED ==`
pushes `true` and `c Color.GREEN ==` pushes `false`; a struct with a
string-carried enum field frees without leaks or double frees (verify by
running the probe twice in one program).

## Stage 4: Fallible signature detection

Fallibility is detected by name: `fallible_payload` accepts a union return
whose error side is the `error` primitive or a name found in
`error_type_ids`. When no type named `Error` is registered (for example
`|i64 error.Error|` without requiring `std.error`), the function silently
behaves as non-fallible and `unwrap` degenerates to identity.

Recorded implementation decisions (Stage 4):

- The shared predicate lives in `src/signature.rs`
  (`classify_signature`); each pass supplies the error-type resolver
  appropriate to what it can see.
- The checker sees main-unit declarations only, so a bare error-side name
  resolves when the unit declares it as an error type, and a
  module-qualified path (`error.Error`) is opaque to the checker: the
  alias may come from a hoisted module require the checker cannot see
  (15_fs.yar names `error.Error` while only std.fs requires std.error).
  The interpreter validates every registered unit once all modules and
  alias bindings are registered, which covers module units and backstops
  the main unit's module-qualified references.
- A bare name is unit-local: module error types are referenced through
  their alias (`error.Error`), the grammar's own convention. The old
  `n == "Error"` fallback in `fallible_payload` is gone.
- A two-member union return with no resolvable error side at all
  (`|i64 i64|`) is also `E386`: anonymous unions exist only as the
  fallible form.

- [x] 4.1 Centralize the fallibility predicate in one function used by both
  the interpreter (`fallible_payload`) and the checker (5.2): a return union
  with exactly two members is fallible when one side is the `error`
  primitive or a named type that resolves to a registered error type,
  including dotted paths such as `error.Error` resolved through the
  module-alias table.
- [x] 4.2 When a two-member return union names an error side that resolves
  to no registered error type, reject the declaration with `E386` (unknown
  error type in fallible signature) instead of silently treating the
  function as non-fallible. Anonymous unions are only supported as this
  fallible form, so every two-member union return must resolve.
- [x] 4.3 Add `E386` to the explain catalog: require the module that
  declares the error type (for example `"std.error" error require`) or
  declare the error type in this unit.
- [x] 4.4 The checker emits `E386` for main-unit declarations it can see,
  and the interpreter emits it at registration for module units; both use
  the same predicate.

Acceptance: `f function do ... end with |i64 error.Error|` without
requiring `std.error` fails with `E386` pointing at the signature; the same
program with `"std.error" error require` runs; a custom `|i64 MyErr|` with
`MyErr` declared in the unit runs; the corpus (examples 10 and 15 use
fallible forms) still passes.

## Stage 5: New static rules

### 5.1 `set` on a `const` or `static` binding

The spec allows `set` on `mutable` only: `const` is set once at runtime (its
initializer) and `static` is a compile-time constant. Today `5 c set` on a
const binding is accepted.

- [x] 5.1.1 Record mutability in the checker's `VarInfo` (from the
  declaration's `Mutability`) and reject `StmtKind::Set` whose target is a
  `const` or `static` binding with `E378`.
- [x] 5.1.2 Record mutability alongside the interpreter's frame locals as
  well and reject the assignment at runtime with the same code, so the rule
  also holds for paths that reach the interpreter without the checker (the
  REPL and `EvalContext`).
- [x] 5.1.3 A re-declaration of the same name rebinds it with the new
  mutability; only `set` is restricted. Add `E378` to the explain catalog.

Acceptance: `100 c const i64` then `5 c set` fails with `E378` pointing at
the `set`; the same probe with `mutable` prints `5`; `50 s static i64` then
`5 s set` fails with `E378`.

### 5.2 `unwrap` in a non-fallible caller is a compile error

The spec: "if the caller could not error, unwrap would be a compile error".
Today this is a runtime `E308` after interpretation has started.

Recorded implementation decisions (5.2):

- The rule fires where the checker can see the error-carrying operand:
  an `unwrap` whose operand is a call to a function visible to the
  checker (main unit, nested, or method) whose signature is fallible.
  That operand is statically an error envelope, so unwrapping it in a
  non-fallible caller is `E379`.
- Opaque operands (module calls such as `list.push_last`, builtins,
  variables) stay allowed: `unwrap` on a non-error value is the identity
  no-op the runtime defines, and the corpus relies on that
  (`xs 4 list.push_last call unwrap` in a non-fallible `main`). The
  runtime `E308` remains the backstop for envelopes the checker cannot
  see, exactly like the borrow machine's opacity simplification.

- [x] 5.2.1 Track the fallibility of the function being checked (its
  return union, via the Stage 4 predicate) in the checker.
- [x] 5.2.2 Reject `Expr::Unwrap` in a function whose return is not
  fallible with `E379`, keeping the existing runtime `E308` as a backstop.
  Add `E379` to the explain catalog.
- [x] 5.2.3 `handle` stays allowed anywhere; only `unwrap` is restricted.

Acceptance: a non-fallible `main` that does `f call unwrap` fails at check
time with `E379` and no interpretation happens; the same call inside a
function declared `with |i64 error.Error|` passes the check.

### 5.3 Union members must be distinct

The spec: union "Members must be distinct". `U union` with two `i32` members
is accepted today; wrapping and matching silently use the first member.

- [x] 5.3.1 Reject a union declaration whose member types contain a
  duplicate (after reference stripping, matching the existing
  `member_ty` normalization) with `E385` at registration, in both the
  interpreter and the checker. Add `E385` to the explain catalog.

Acceptance: `U union` with two `i32` members fails with `E385`; `U union`
with `i32` and `reference<i32>` also fails (same member after stripping);
the corpus unions still declare.

### 5.4 Field visibility is enforced

The spec: "Default visibility is private for the type and each field;
`public` exports." Field visibility is parsed today but never enforced.

- [ ] 5.4.1 Extend `StructInfo` with each field's visibility and the unit
  (main or module path) that declared the struct.
- [ ] 5.4.2 In field access and field `set` (`Expr::Member` evaluation and
  the `StmtKind::Set` member target), reject access to a non-`public` field
  from a function whose unit differs from the declaring unit, with `E397`.
  Same-unit access to private fields stays legal.
- [ ] 5.4.3 Add `E397` to the explain catalog: mark the field `public` in
  the declaring module or move the access into that module.

Acceptance: a module struct with a private field, accessed from the main
unit, fails with `E397`; the same access from inside the module runs; the
corpus (std.fs `File.fd` is public) still passes.

### 5.5 if/else join stack compatibility

The spec: "Then/else must leave compatible stacks at join." Mismatched
branch heights are accepted today.

- [ ] 5.5.1 In the checker's `If` walk, compare the then-branch and
  else-branch tails (it already computes both) and reject differing stack
  heights with `E396`. Slot types are opaque in the checker, so height is
  the compatibility criterion.
- [ ] 5.5.2 An `if` with no `else` must leave the then-branch height at
  zero extra values (the else path pushes nothing), so a then-branch that
  leaves values above the pre-`if` height is rejected too. Add `E396` to
  the explain catalog.

Acceptance: a branch pair leaving one value versus two values fails with
`E396`; a then-only `if` that leaves a value on the stack fails with
`E396`; the corpus control flow still passes.

## Stage 6: Static type checking

The spec calls Yarrow "statically typed" and says declarations perform
"implicit coercion to Type allowed", which implies a compatibility check.
Today nothing validates types: `3.14 f const i32` and `"hello" s const i64`
bind silently and the failure surfaces only at use, if at all. The checker
is flow-only. This stage extends the Stage 3 checker machine with a type
per abstract slot.

Decision: the checker reuses the interpreter's kind-code scheme
(`primitive_kind_code` plus named-type resolution) as its type
representation. The coercion matrix is: a value binds to a declared type
when the kinds are equal; an integer value binds to any integer type
(widening, with the literal's value range-checked when statically known);
a float literal binds to any float type; every other combination (bool to
integer, float to integer, string to anything else, struct to another
struct, and so on) is an error. Module functions and module types stay
opaque, so cross-unit calls are unchecked, exactly like the existing
borrow-machine simplification.

- [ ] 6.1 Extend the checker's abstract `Slot` with a type (kind code or
  opaque) and thread it through every `check_expr` arm that pushes a value:
  literals (integer literals get their smallest-fitting kind, floats
  theirs), type values, container literals, calls, builtins, and field
  access.
- [ ] 6.2 Binding compatibility (`E310`): `VarDecl` and parameter binding
  check the popped slot's type against the declared type using the coercion
  matrix; `set` checks against the variable's recorded type.
- [ ] 6.3 Operator operands (`E311`): arithmetic requires numeric operands
  of compatible kinds, bitwise requires integers, logical `and` / `or` /
  `not` requires bools, `~` requires strings, comparisons require
  matching families, and pointer arithmetic follows the existing pointer
  slot rule.
- [ ] 6.4 Call signatures (`E312`): for functions visible to the checker
  (main unit and its nested functions and methods), reject a call with too
  few operands for the parameter count and type-check the operands against
  the parameter types; unknown and module functions stay opaque.
- [ ] 6.5 Conditions (`E313`): `if` and `for` conditions and `match` case
  conditions must be bool. The interpreter's `eval_match_cond` currently
  accepts integers as truthy; tighten it to bool so the runtime agrees with
  the check.
- [ ] 6.6 Add `E310` through `E313` to the explain catalog, each naming the
  construct (binding, operator, call, condition) and the coercion the spec
  allows.

Acceptance: `3.14 f const i32` and `"hello" s const i64` fail at check time
with `E310`; `1 "a" +` fails with `E311`; a call with a missing argument
fails with `E312`; `5 if` fails with `E313`; `42 myVar mutable i32` (u8
literal widened) still passes; every file in `docs/examples/valid/` passes
without new diagnostics.

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

Probe programs for the acceptance criteria above live in `/tmp/yarrow-audit2/`
from the audit session; recreate them from the criteria text if that
directory is gone. Per AGENTS.md, do not add automated tests; tests come
later.

When the last stage closes, refresh the corpus list in the module doc of
`src/interpreter/mod.rs` (the maintenance rule of the previous plan still
applies: every file in `docs/examples/valid/` must appear there and run
cleanly).
