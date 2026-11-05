# yarrow-core PLAN.md

Roadmap for closing the gaps between `docs/GRAMMAR.md` (with the EBNF in
`docs/SYNTAX.md`) and the interpreter in this crate. The previous plan's
stages are complete and were removed; this plan holds the findings of the
fourth spec audit. That audit compared the spec against
`src/interpreter/mod.rs`, `src/checker/mod.rs`, and `src/parser/mod.rs`,
and verified every finding with a probe program (run from
`/tmp/yarrow-audit4/`; recreate the probes from the acceptance criteria if
that directory is gone).

## Deferral policy

This plan has no deferrals, explicitly and without exception. Every task
must close its gap completely in the stage that owns it:

- An `E393` "interpreter does not support ... yet" outcome is not
  acceptable for any item below. Where a spec-valid program reaches an
  `E393` today (Stages 1 through 5), the task replaces it with the real
  feature or a real diagnostic. `E393` remains allowed only for internal
  invariants that no spec-valid program can reach (for example an array as
  the entry return value).
- Where the spec is ambiguous, the stage records exactly one decision and
  implements it in the same stage; no feature waits on a spec
  clarification and nothing is parked behind a diagnostic. Stages 6, 7,
  8, and 9 record such decisions.
- Where a rule needs a new diagnostic code, the code is reserved here and
  must be added in the same task. Reserved codes (verified unused today):
  `E134` (tokenizer family, which does not use the explain catalog) and
  `E400` (explain catalog).
- If a task uncovers a deeper gap beneath its own, the stage grows to
  close that gap too. Partial stages do not graduate.

Status legend:

- `[ ]` todo
- `[~]` in progress
- `[x]` done

## Table of contents

- [Stage 1: Composite struct fields](#stage-1-composite-struct-fields)
- [Stage 2: Container-typed union members](#stage-2-container-typed-union-members)
- [Stage 3: `implement` on enums and unions](#stage-3-implement-on-enums-and-unions)
- [Stage 4: Chained member call targets](#stage-4-chained-member-call-targets)
- [Stage 5: Type values for generic and anonymous union types](#stage-5-type-values-for-generic-and-anonymous-union-types)
- [Stage 6: `for` over a hashmap](#stage-6-for-over-a-hashmap)
- [Stage 7: `const` accepts one runtime `set`](#stage-7-const-accepts-one-runtime-set)
- [Stage 8: The documented coercion matrix](#stage-8-the-documented-coercion-matrix)
- [Stage 9: Unicode rune literals](#stage-9-unicode-rune-literals)
- [Stage 10: Diagnostic quality](#stage-10-diagnostic-quality)
- [Verification](#verification)

## Stage 1: Composite struct fields

The EBNF has `field_decl = type, identifier, [ visibility ]`, and `type`
includes `generic_type` and `union_type_literal`. Two field types fail with
`E393` "interpreter does not support field type ... yet" from
`field_layout` (`src/interpreter/mod.rs`):

- `array<T N>`: a struct with `array<i32 3> data public` fails at
  registration (probe p30). `field_layout` has no `TypeKind::Array` arm.
- Anonymous union `|T U|`: the same struct shape with
  `|i32 string| payload public` fails the same way (probe p32);
  `TypeKind::Union` falls into the catch-all. Named union fields work
  (probe p31).

Recorded implementation decisions (Stage 1):

- An array field stores its elements inline in the struct allocation:
  size `N * element size`, align = element align, kind code
  `0x60 | (elem << 8) | (count << 40)` (the same encoding bindings use).
  A read builds an array value from the bits; a write (a struct literal
  field or `w.data set`) stores the element bits. Arrays are copy types,
  so a field read pushes a real copy.
- An anonymous union field type is interned by its member-type signature:
  the same member list always maps to one union id, shared with Stage 5's
  type values. The field then stores a `KIND_UNION | (id << 8)` handle
  exactly like a named union field.
- The checker already parses both field types; it must model their stack
  effects like any other field. Privacy and visibility rules are
  unchanged.

- [x] 1.1.1 `field_layout` gains a `TypeKind::Array` arm with the inline
  size/align/kind rules above; struct registration, field reads, field
  writes, and struct literals with array fields all work.
- [x] 1.1.2 `field_layout` resolves `TypeKind::Union` through the
  interning table; a struct field of anonymous union type binds, reads,
  writes, and drops like a named union field.
- [x] 1.2.1 The checker models both field types for the main unit.

Acceptance: p30 and p32 run; the named-union-field probe p31 still runs;
a struct with an array field drops without leaks; every file in
`docs/examples/valid/` still passes.

Stage 1 findings, recorded when the stage closed:

- p30's registration, struct literal, field read, field set, and drop
  all work, but its final check line `w.data 1 ==` compares an array
  with an integer, which is `E333` exactly as for a plain array binding
  (probe p34 behaves identically). The probe's check line predates array
  field semantics; the field mechanics were verified with an
  iteration-based probe instead. No `E393` remains on the field path.
- A union field (named or anonymous) now wraps a raw member value at
  struct-literal and `set` time, like a union binding does. Before this
  stage those writes stored the raw bits as the union handle and
  segfaulted at drop.
- A field or element type whose kind code does not fit its embedding
  width (an array of arrays, an array of hashmaps, a hashmap with an
  array key or value) is rejected with `E340` instead of a corrupted
  encoding; a list literal with a fixed-size array element is rejected
  with `E340` because list storage holds one u64 per element.

## Stage 2: Container-typed union members

The EBNF has `union_decl = identifier, "union", type, { type }`, and
`type` includes `list<T>`, `hashmap<K V>`, and `array<T N>`. `member_ty`
(`src/interpreter/mod.rs`) handles only primitives, named types, and
references; a union with a `list<i32>` member (probe p36) or an
`array<i32 2>` member (probe p39) fails with `E393` "union member ... yet".

Recorded implementation decisions (Stage 2):

- `list<T>` and `hashmap<K V>` members store their heap handle in the
  union payload, exactly like string members today.
- `array<T N>` members store their elements inline; the union payload is
  sized to its largest member as today.
- Init and `set` accept values of these member types (union packing), and
  match arms (`list<i32> case`) receive `reference<Member>` like every
  other arm, autoderefing on read.
- Member types must still be distinct (`E385` unchanged).

- [ ] 2.1.1 `member_ty` gains `List`, `Hashmap`, and `Array` arms with
  the storage rules above; union registration sizes payloads correctly.
- [ ] 2.1.2 Union init, `set`, and match arms work for container members;
  the arm in p36 reads the list back through the reference.
- [ ] 2.2.1 The checker models container members for the main unit.

Acceptance: p36 and p39 run; a union with a list member round-trips
through a match arm; duplicate member types still fail with `E385`; the
corpus still passes.

## Stage 3: `implement` on enums and unions

The EBNF has `implement_block = identifier, "implement",
{ function_decl }, "end"`; the identifier is not restricted to structs.
`Color implement` and `U implement` parse and register, but calling the
method fails with `E393` "method call / complex call target" (probes p16
and p38): call resolution only handles struct receivers (`KIND_STRUCT`).

Recorded implementation decisions (Stage 3):

- Methods resolve on enum and union receivers exactly like struct
  receivers: the receiver slot's kind gives the type id, and the method
  is looked up as `Type::method`. The receiver is still an explicit
  borrow plus a `self const reference<T>` binding; enums and unions have
  no fields, so a method body reads the discriminant or payload through
  `self` like any reference.
- Method visibility follows the existing function rules: a private method
  is same-unit only.

- [ ] 3.1.1 Call resolution accepts `KIND_ENUM` and `KIND_UNION`
  receivers in the interpreter; the checker mirrors for the main unit.
- [ ] 3.1.2 p16 and p38 run: an enum method returns through a borrowed
  receiver, and a union method reads the active payload.

Acceptance: p16 prints `red`; p38 runs; struct methods are unchanged
(`00_grammar_tour` and `06_structs_and_enums` pass); the corpus still
passes.

## Stage 4: Chained member call targets

`qualified_name = identifier, { ".", identifier }` allows any depth, and
a word may be a call target. `o.inner.getv call` fails with `E393`
(probe p41): resolution stops after one member.

Recorded implementation decisions (Stage 4):

- A member-chain call target resolves left to right: the first segment is
  a local variable (alias and item paths keep their existing precedence),
  each intermediate segment is a field read that autoderefs through
  references, and the final segment resolves as a method on the field's
  struct type. The receiver pushed for the call is a borrow of that field.
- A failure at any link reports the existing diagnostic for that link
  (`E320` unknown variable, the unknown-field diagnostic, `E330` unknown
  function), never `E393`.

- [ ] 4.1.1 Implement chain resolution in the interpreter's call-target
  path; the checker mirrors for the main unit.
- [ ] 4.1.2 p41 runs: `o.inner.getv call` returns 7 through the nested
  struct.

Acceptance: p41 prints `chained method call ok`; alias and item call
targets are unchanged; a chain with an unknown middle field reports the
unknown-field diagnostic; the corpus still passes.

## Stage 5: Type values for generic and anonymous union types

The EBNF has `word = ... | type | ...`, and `type` includes
`generic_type` and `union_type_literal`. The parser only turns bare
primitive names into type values (`src/parser/mod.rs`, the
`Primitive::parse_name` arm), so `a typeof array<i32 3> ==` parses `<` as
a comparison and fails with `E311` (probe p24). typeof comparisons for
generic types are impossible today. `docs/AST.md` already documents
`TypeValue(Type)`; the implementation carries only a name.

Recorded implementation decisions (Stage 5):

- `Expr::TypeValue` carries a full `Type`, matching `docs/AST.md`. In
  word position the parser accepts `generic_type` (`array`, `list`,
  `hashmap`, `pointer`, `reference`) and `union_type_literal`
  (`|T U|`) and emits the type value.
- Type value equality is structural and uses the same kind encoding as
  bindings (Stage 1's array code, the list/map/pointer codes, and the
  anonymous-union interning): two type values are equal iff their
  encodings match. An anonymous union type value is equal to a named
  union type with the same member signature through the interning table.
- An unknown type name in word position fails with `E340`, like any other
  type use.

- [ ] 5.1.1 Parser: type words for generic and anonymous union types; the
  `TypeValue` AST node carries the full type.
- [ ] 5.1.2 Checker and interpreter compare type values structurally;
  `a typeof array<i32 3> ==` and `u typeof |i32 string| ==` work.
- [ ] 5.2.1 The parse-time `dup` clone list (Stage 1 of the previous plan)
  stays correct for the new type-value words.

Acceptance: p24 runs (array and list typeof comparisons); a union typeof
compares equal to its anonymous signature; `i32` type values are
unchanged; the corpus still passes.

## Stage 6: `for` over a hashmap

`SYNTAX.md` says the iterable form "iterates containers", and
`GRAMMAR.md` names `hashmap<K V>` a container. `m for` fails with `E394`
(interpreter) and `E313` (checker) (probe p13).

Recorded implementation decisions (Stage 6):

- A hashmap iterable walks its values in insertion order. `loop.value`
  pushes the current value with the same copy/borrow semantics as list
  elements; `loop.index` is not available and keeps the existing
  "not available in this for" diagnostic. Keys stay reachable through
  `std.map`; pairs are not a language value.
- The `E394` and `E313` messages shrink to the genuinely invalid sources.

- [ ] 6.1.1 The runtime `for` dispatch accepts hashmap iterables with the
  semantics above.
- [ ] 6.1.2 The checker accepts hashmap sources for the main unit.
- [ ] 6.1.3 p13 runs and prints its line after iterating both values.

Acceptance: p13 runs; array and list iteration are unchanged;
`loop.break` and `loop.continue` work in a hashmap loop; the corpus still
passes.

## Stage 7: `const` accepts one runtime `set`

`GRAMMAR.md`'s variable comment reads "const - set once at runtime". The
interpreter rejects every `name set` on a const binding with `E378`
(probe p09).

Recorded implementation decisions (Stage 7):

- A const binding accepts exactly one `name set` after its declaration.
  The set follows the mutable rules (coercion allowed, old value dropped,
  borrow released first). A second set fails with `E378`; the message and
  explain entry say the binding already received its one set.
- Reading of the spec: the const line uses the `set` keyword in the same
  breath as the mutable line's "reassign with `name set`", and static is
  the zero-set variant. The alternative reading (the initializer is the
  one set) matches today's behavior but leaves the sentence with no work
  to do. The decision is recorded here; `GRAMMAR.md` is not edited.
- The checker tracks the one-set state for the main unit; the interpreter
  backstops module units and the REPL.

- [ ] 7.1.1 Checker: allow the first set on a const binding, reject the
  second with `E378`; update the `E378` message and explain entry.
- [ ] 7.1.2 Interpreter: mirror the rule for module units.
- [ ] 7.1.3 The never-written warning (`21_warning_never_written`) stays
  correct: a const that is never set does not warn.

Acceptance: p09 runs (the first set binds, the second fails with `E378`);
the corpus and the warning examples pass unchanged.

## Stage 8: The documented coercion matrix

`TYPE_SYSTEM.md`'s "Allowed implicit coercions" table documents
integer-to-float (convert), float-to-integer (saturating convert),
bool-to-integer (`0`/`1`, then widen), and integer-to-bool (non-zero to
`true`) at the five coercion sites. The implementation coerces only
within integers or within floats; probe p42 fails all four with `E310`.
`GRAMMAR.md` says only "implicit coercion to Type allowed" and stays
authoritative on where coercion applies; the matrix is the derived doc's
specification of what converts.

Recorded implementation decisions (Stage 8):

- Implement the four conversions at the five sites `TYPE_SYSTEM.md`
  lists: variable declarations, function parameters, `set`,
  container/struct literal elements, and `handle` fallback.
  Integer-to-integer and float-to-float coercion are unchanged;
  struct-to-struct and `reference<T>`-to-`pointer<T>` stay non-implicit.
- Integer narrowing keeps its current fit check: a narrowing coercion
  must preserve the value (probe p47), and an out-of-range narrowing
  stays `E310`. The table's "widen or narrow" is read as
  value-preserving.
- float-to-integer saturates: NaN to 0, out of range to the nearest
  bound.
- `E310`'s message and help text are rewritten to name the matrix and
  drop the stale `\convert the value` advice; no `\` operator exists in
  the grammar or the tokenizer.

- [ ] 8.1.1 Checker: the four conversions at all coercion sites; `E310`
  only for the rest.
- [ ] 8.1.2 Interpreter: mirror the rule for module units and the REPL.
- [ ] 8.1.3 Update `E310`'s message, help text, and explain entry.

Acceptance: p42 runs (all four conversions plus mixed promotion); p47
still narrows fitting values and still rejects `1000 z const u8` with
`E310`; the corpus still passes.

## Stage 9: Unicode rune literals

The EBNF has `rune_char = ? any character except ' or \ ?`. The tokenizer
counts bytes, so a multi-byte character like `'é'` fails with `E133`
"rune literals must contain exactly one character or escape sequence"
(probe p40), although `decode_rune_literal` already yields a Unicode
code point and rune storage is already a 4-byte code point (kind 11).

Recorded implementation decisions (Stage 9):

- A rune literal holds one Unicode scalar value. The tokenizer counts
  scalar values, not bytes; no layout or runtime change is needed.

- [ ] 9.1.1 Fix the tokenizer's rune-length check to count characters;
  `'é'` tokenizes and evaluates to its code point.

Acceptance: `'é' typeof rune ==` runs; `'a'`, `'\n'`, and `'\''` are
unchanged; the corpus still passes.

## Stage 10: Diagnostic quality

Three small gaps found by the audit's probes; all are user-reachable
today.

- [ ] 10.1.1 Bare error member case: `BASE_ONE case` inside `handle`
  reports `E308` "match type dispatch requires a union subject", which
  names the wrong problem; the qualified forms work (probe p23 versus
  p23b). Keep requiring the qualified form, but when the word before
  `case` is a bare identifier that names a known error member, report an
  `E308` message that names the member and the two accepted forms
  (`Type.MEMBER case`, `error.MEMBER case`).
- [ ] 10.2.1 Invalid identifier: `_x` (the EBNF requires identifiers to
  start with a letter) currently surfaces as `E362` "missing operand for
  value" (probe p29). The tokenizer rejects identifiers that do not
  start with a letter with the reserved `E134` and a message that names
  the rule.
- [ ] 10.3.1 `void` binding: `5 x mutable void` is accepted silently
  (probe p46), although `void` marks an empty return and is not a stack
  value. Reject `void` as a declared binding or parameter type with the
  reserved `E400`, added to the explain catalog: use a real type, or
  drop the value instead of binding it.

Acceptance: the p23 probe names the qualified forms; `_x` fails with
`E134` at the right span; `5 x mutable void` fails with `E400`; the
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

Probe programs for the acceptance criteria above live in
`/tmp/yarrow-audit4/` from the audit session; recreate them from the
criteria text if that directory is gone. Per AGENTS.md, do not add
automated tests; tests come later.

When the last stage closes, refresh the corpus list in the module doc of
`src/interpreter/mod.rs` (the maintenance rule of the previous plans still
applies: every file in `docs/examples/valid/` must appear there and run
cleanly).
