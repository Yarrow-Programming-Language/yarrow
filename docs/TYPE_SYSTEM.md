# Type system

Static types for Yarrow, derived from [`GRAMMAR.md`](GRAMMAR.md), [`SYNTAX.md`](SYNTAX.md), and [`AST.md`](AST.md). Ownership, borrowing, and regions are summarized only where they affect typing. For full details, see [`MEMORY_MODEL.md`](MEMORY_MODEL.md).

Contents: [Primitive types](#primitive-types), [Composite types](#composite-types), [Copy vs non-copy](#copy-vs-non-copy), [Coercions](#coercions), [Conversions](#conversions), [Type checking](#type-checking).

Yarrow is stack-typed: every value on the evaluation stack has a static type, and every word has a known stack effect. There are no lifetime parameters on types; `reference<T>` is a borrow, not a named lifetime.

---

## Primitive types

| Type                   | Kind              | Notes                                 |
| ---------------------- | ----------------- | ------------------------------------- |
| `void`                 | unit              | Default return when `with` is omitted |
| `bool`                 | logical           | `true` / `false`                      |
| `rune`                 | character         | Single character literal, e.g. `'\n'` |
| `string`               | heap text         | Non-copy                              |
| `i8` `i16` `i32` `i64` | signed integers   |                                       |
| `u8` `u16` `u32` `u64` | unsigned integers |                                       |
| `f16` `f32` `f64`      | floats            |                                       |

`void` is not a stack value users push as a literal; it marks an empty return.

### Literal inference

Literals pick the smallest fitting concrete type:

- Positive integers -> smallest unsigned that fits (`42` -> `u8`, `1_000` -> `u16`, `0xAB12` -> `u16`)
- Negative integers -> smallest signed that fits (`-900` -> `i16`)
- Bases `0b` / `0x` follow the same fit rules on their magnitude
- Floats -> smallest float that fits (`3.14` -> `f16`)
- `"..."` -> `string`, `'...'` -> `rune`, `true`/`false` -> `bool`

Underscores in numeric lexemes are digit separators only.

### Types as values

A type name may appear as a stack word (`TypeValue` in the AST). `typeof` pops a value and pushes its static type (for references, the pointee type). Type values compare with `==` / `!=`, e.g. `myVar typeof i32 ==`.

Heap values used with `typeof` typically arrive as borrows (variable read or `dup`); `typeof` releases that borrow and leaves the data owned.

---

## Composite types

Surface forms match [`SYNTAX.md`](SYNTAX.md) / [`AST.md`](AST.md).

| Form           | Meaning                                                         |
| -------------- | --------------------------------------------------------------- |
| `array<T [N]>` | Fixed-size array; `N` optional when inferred from a literal     |
| `list<T>`      | Dynamic sequence                                                |
| `hashmap<K V>` | Key/value map                                                   |
| `pointer<T>`   | Typed raw address (compile-time pointee; runtime is an address) |
| `reference<T>` | Borrow of a value of type `T`                                   |
| `\|T U ...\|`  | Anonymous union (often a function return)                       |
| Named types    | `struct`, `enum`, `union`, `error` declarations                 |

Type arguments inside `<>` are whitespace-separated.

### User-declared types

**Struct** - named fields, each with a type and optional visibility. Default visibility is private.

**Enum** - named members; default underlying type `i32`. An explicit type (e.g. `Color string enum`) yields a typed enum. Members may take an explicit discriminant; otherwise ordinals start at 0 and continue sequentially.

**Union** - holds exactly one of its member types at a time. Member types must be distinct. Initialization and `set` accept any member type. `typeof` on a union value reports the union type, not the active member. `match` with `Type case` discriminates the active member (see Type checking).

**Error** - like an enum specialized for error handling. Optional injection copies members from another error type (`Name other.error error ... end`). Error members are comparable and usable in `|T Err|` returns.

### Empty containers

`()`, `[]`, and `{}` carry no element (or struct) type until a typed context supplies one, typically a variable declaration:

```yarrow
() myList mutable list<i32>
{} myMap mutable hashmap<string i32>
```

Non-empty list/array literals take a common element type from their contents (with coercion where allowed). Hashmap literals use literal keys; struct literals use identifier keys (`{name "Alice" scores (10 20)}`).

### `pointer<T>` vs `reference<T>`

|           | `reference<T>`                                       | `pointer<T>`                                                          |
| --------- | ---------------------------------------------------- | --------------------------------------------------------------------- |
| Safety    | Safe borrow; checked                                 | Unsafe raw address                                                    |
| Creation  | `borrow`, variable read of non-copy, union match arm | `mem.allocate`, integer coerced into a typed pointer slot, arithmetic |
| Autoderef | Field/method access and many reads autoderef         | Field access autoderefs inside `unsafe`                               |
| Typing    | Pointee is `T`; `typeof` reports `T`                 | Pointee is compile-time only                                          |

`mutable` on a `reference<T>` parameter requires the pointee to be mutable.

---

## Copy vs non-copy

From the grammar:

**Copy types** (bit-copy / trivial duplicate with `dup` and variable read):

- integers, floats, `bool`, `rune`
- enums
- `array<T N>`
- `pointer<T>`

**Non-copy types** (variable read pushes a borrow; use `borrow` / `move` explicitly):

- `string`
- `list<T>`
- `hashmap<K V>`
- unions
- structs

`dup` copies copy types. For non-copy values, duplicate access goes through borrows, not bitwise duplication of ownership.

Parameter modifier `copy` deep-copies heap values (`string`, `list`, `hashmap`, structs, unions, arrays) into the callee's local stack; without it, parameters are moved in declaration order. On scalars, enums, and pointers, `copy` has no effect.

---

## Coercions

Implicit coercions are allowed only in specific contexts. Elsewhere, types must match exactly (or via an explicit conversion rule below).

### Where implicit coercion applies

1. **Variable declarations** - the value popped into `name mutable|const|static T` may coerce to `T`.
2. **Function parameters** - each argument may coerce to the declared parameter type.
3. **`set`** - the new value may coerce to the target's declared type (including union member injection into a union variable).
4. **Container / struct literal elements** - elements may coerce to the inferred or contextual element/field type.
5. **`handle` fallback** - the fallback word should be usable at the success type of the handled call.

### Allowed implicit coercions

| From         | To                              | Rule                                                                           |
| ------------ | ------------------------------- | ------------------------------------------------------------------------------ |
| integer      | integer                         | widen or narrow (signedness preserved by extend/reduce)                        |
| float        | float                           | promote or demote                                                              |
| integer      | float                           | convert                                                                        |
| float        | integer                         | saturating convert                                                             |
| `bool`       | integer                         | `0` / `1`, then widen if needed                                                |
| integer      | `bool`                          | non-zero -> `true`                                                             |
| integer      | `pointer<T>` / heap handle slot | address / null-style init (e.g. typed pointer from `allocate`)                 |
| pointer-like | `pointer<_>`                    | address passes through when the target is a generic or compatible pointer slot |

Same-width integer types that differ only in signedness may share a representation for coercion purposes when bits match.

**Not** implicit: arbitrary struct <-> struct, unrelated named types, or safe `reference<T>` <-> `pointer<T>`.

### Numeric promotion (operators)

Binary arithmetic and comparisons on mixed numeric operands use a **common type**: the wider of the two; if equally wide and either side is signed, prefer signed. Then both operands coerce to that type before the op.

Special case: `pointer<T> + int` (and related pointer arithmetic) keeps `pointer<T>`; the integer is a byte offset.

---

## Conversions

Conversions are coercions that change representation. In Yarrow they are not spelled with a dedicated cast keyword; they happen through:

1. **Implicit coercion** in the contexts above
2. **Typed stores** - declaring or `set`ting a slot of type `T`
3. **Union packing** - storing a member value into a union-typed variable
4. **Unsafe memory** - `load` / `store` on `pointer<T>` reinterprets bytes as `T`; raw `mem.load` / `mem.store` use untyped `i64` words and do not check pointee types

There is no general user-level ""as"" / cast operator in the grammar. To change type outside coercion sites, go through a typed binding or (in `unsafe`) through pointers.

### `typeof` and type identity

`typeof` yields a type value suitable for equality checks. It does not convert the original value's payload; for references it reports the pointee. Comparing type values is the supported reflective conversion check (`x typeof T ==`).

---

## Type checking

Checking walks the program in stack order. Each statement updates a typed stack model. Ill-typed stack effects are compile errors.

### Stack discipline

- Literals, names, type values, and containers **push**
- Operators and keyword ops **pop** their arity and **push** results as specified
- `drop` clears the stack and releases borrows on it
- `pop` removes one value (and releases a borrow if the value is a reference)
- Declarations and `set` pop the value being stored
- `if` / conditional `for` require a `bool` on top before the keyword
- Iterable `for` requires an iterable (`array`, `list`, ...) on top
- `match` borrows its subject for the duration and restores the prior stack afterward
- `return` takes the top as the return value (if any) and drops the rest; the result must match `with T` (or void)
- `call` pops the callee (and uses preceding stack args per the signature), then pushes returns
- `unwrap` / `handle` apply only to fallible returns (`|T Err|` or equivalent)

Nested functions are typed in their own stack frame; they are only callable from the enclosing body.

### Operator result types

| Class                                  | Operands                                         | Result                                                                        |
| -------------------------------------- | ------------------------------------------------ | ----------------------------------------------------------------------------- |
| Arithmetic `+ - * / // % ^`            | numeric (or `pointer + int`)                     | common numeric type, or `pointer<T>`                                          |
| Concatenation `~`                      | `string` (autoderef through `reference<string>`) | `string`                                                                      |
| Comparison                             | comparable operands                              | `bool`                                                                        |
| Logical `and or not`                   | `bool`                                           | `bool`                                                                        |
| Bitwise `and or xor lshift rshift not` | integers                                         | integer (common / same width rules)                                           |
| `typeof`                               | any                                              | type value                                                                    |
| `borrow`                               | non-copy / heap-capable value                    | `reference<T>` (logical)                                                      |
| `move`                                 | owned non-copy source + target name              | transfers ownership; no push                                                  |
| `load`                                 | `pointer<T>`                                     | `T`                                                                           |
| `store`                                | `pointer<T>`, value `T`                          | writes; no useful push                                                        |
| `call`                                 | function + args                                  | per signature                                                                 |
| `unwrap`                               | `\|T Err\|`                                      | `T` on success; on error propagates or is rejected if the caller cannot error |

`and` / `or` / `not` overload on `bool` (logical) vs integer (bitwise). `+` is not overloaded: strings concatenate with `~`.

Autoderef: reads through `reference<T>` (and, in `unsafe`, `pointer<T>` field access) behave like `T` for arithmetic, comparison, and concatenation (`~`).

### Functions and returns

```text
name [visibility] [unsafe] function { parameter } do { statement } end [with type]
parameter = type [copy | mutable]
```

- Parameter list is the input stack effect (declaration order = deep to shallow on entry).
- `with T` is the output type; omit for `void`.
- `with |T Err|` (union literal) is a fallible return: success carries `T` (or void), failure carries an error member.
- `main` may omit `with`; a numeric return becomes the process exit code.
- `unsafe function` does not weaken type, stack, ownership, or borrow checks; it only permits unsafe operations and requires callers to be inside `unsafe`.

### Variables

```text
lvalue (mutable | const | static) type   # pops value
lvalue set                               # pops new value
```

- Declared type is authoritative after coercion.
- `const`: single runtime binding.
- `static`: initializer must be known at compile time.
- Read: copy type -> push copy; non-copy -> push borrow.

### Control flow typing

**`if`** - then and else are checked independently; stack height and types at join must agree (both branches leave a compatible stack).

**`match`**

- Value / error style: words before `case` must leave a `bool`.
- Union style: word before `case` is a member `Type`; the arm's stack top becomes `reference<Member>`.
- Case types must be members of the subject union; `else` is required unless cases are exhaustive.
- Subject borrow ends at `end`; the subject value is unchanged.

**`for`**

- Condition form: top is `bool` each iteration (while-style).
- Iterable form: top is iterable; loop body may use `loop.value` / `loop.index` from `std.loop` (typed by the iterable's element type / `i32`-like index).

**`handle`** - success path yields the success type; on error, runs the handler and pushes the fallback, which must match the success type (via coercion if needed).

### Unions and errors

- Assigning into a union variable requires the value's type to be one of the members (after coercion to a member).
- Exhaustiveness: if every member has a `Type case`, `else` may be omitted; otherwise `else` is required.
- Error returns use the same envelope idea as `|T Err|`: `unwrap` projects `T` or propagates; `handle` recovers with a typed fallback.

### Unsafe boundary (typing)

Inside `unsafe ... end` (and only there for call sites of `unsafe function`):

- `load` / `store` on `pointer<T>` are typed as `T`
- Pointer arithmetic preserves `pointer<T>`
- Raw `mem.load` / `mem.store` are untyped word ops (`i64`)

Outside `unsafe`, those operations are rejected. Unsafe never skips stack-effect or borrow checking.

### Modules

`"path" [alias] require` brings typed bindings into a scope. Qualified names (`io.write_line`, `Color.RED`, `error.OUT_OF_MEMORY`) resolve to the imported or declared entity's type (function, enum member, error member, ...).

---

## Summary of obligations

Type checking enforces:

- **Stack effect**: Every word's pops/pushes match the live stack
- **Exactness**: Outside coercion sites, types match
- **Copy**: `dup` / plain reads only duplicate copy types
- **Borrow**: Non-copy reads and `borrow` yield references; `typeof` / `pop` release them
- **Union**: Members distinct; cases are member types; arms see `reference<Member>`
- **Errors**: Fallible returns are union literals; `unwrap`/`handle` match caller capability
- **Unsafe**: Raw pointer ops and unsafe calls only in `unsafe`; checks otherwise unchanged

Implementation details (Cranelift layouts, runtime kind codes) live under `crates/yarrow-core` and must stay consistent with this model, not the other way around.
