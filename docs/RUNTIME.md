# Runtime

How a Yarrow program executes: evaluation stack, calls, errors, and modules. Complements [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md) and [`MEMORY_MODEL.md`](MEMORY_MODEL.md). Surface forms come from [`GRAMMAR.md`](GRAMMAR.md) and [`SYNTAX.md`](SYNTAX.md).

Contents: [Execution model](#execution-model), [Stack](#stack), [Functions](#functions), [Errors](#errors), [Modules](#modules), [Interaction with memory and types](#interaction-with-memory-and-types).

## Execution model

Pipeline: source `.yar` is tokenized, parsed to an AST, and then interpreted via the stack VM interpreter.

**Trivia:** The tokenizer emits `Comment` tokens for `#` ... end of line (lexeme includes `#` and comment text; the terminating newline is not part of the lexeme). Whitespace and newlines are not tokens. The parser skips `Comment` tokens the same way it ignores whitespace; comments are not AST nodes. Tools that need comment text (formatters) should read the token stream before parse.

The interpreter tree-walks the AST with an explicit runtime operand stack.

- User modules resolve relative to the source file's directory (`"a.b"` -> `a/b.yar`).
- The standard library is embedded and imported the same way as user code (`"std.io"`, ...).
- Interpreted code talks to a small **host runtime** for heap headers (strings, lists, maps, regions, free) and raw `alloc` / `free`.
- Heap values are opaque handles; scalars and addresses are machine words. Kind codes describe how to free nested heap data.

### Program entry / process `main`

A Yarrow program entry (default name `main`, override via `SessionOptions::entry_name`) is the starting point for execution.

| Piece      | API / symbol                                  | Role                                                                              |
| ---------- | --------------------------------------------- | --------------------------------------------------------------------------------- |
| Entry name | `SessionOptions::entry_name` (default `main`) | Which top-level Yarrow function is the program entry. CLI `--main` maps here.     |
| Session    | `Session::interpret_source`                   | Parses and runs the entry function. Missing entry is `E360` naming that function. |

Exit mapping:

- void -> `0`
- integer (including enum) -> value as exit status
- bool / float / string -> printed, exit `0`

**Host runtime:**

The interpreter uses the host runtime from `yarrow_runtime` for heap operations. The same runtime is used for all execution modes.

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

---

## Stack

The **evaluation stack** is the primary place values live between operations.

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
- Default name is `main`; `SessionOptions::entry_name` (CLI `--main`) may select another top-level function.
- Return optional; numeric return may set the process exit code for the CLI driver.

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
3. **Item import**: parent-first: `"a.b.c" require` may mean function `c` in module `a.b` rather than a nested module file; function wins over module when ambiguous.

Imported modules are parsed and loaded into the interpreter session.

### Visibility and names

- Default visibility for declarations and fields is private; `public` exports.
- `main` is public by default.
- Qualified names: `alias.entity`, `Type.MEMBER`, `error.TAG`, `module.fn`.

### Compilation unit

A complete runnable unit is:

1. The entry `.yar` with `main`
2. Every module reached by `require` (transitively)

A require cycle (A loads B which loads A again while A is still loading) is rejected with `E382` at the `require` site that closes the cycle.

## Interaction with memory and types

| Concern                                           | Doc                                  |
| ------------------------------------------------- | ------------------------------------ |
| What may sit on the stack / coerce                | [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md)   |
| Who frees handles; borrows; regions; `pointer<T>` | [`MEMORY_MODEL.md`](MEMORY_MODEL.md) |
| AST shape of `call`, `handle`, `require`          | [`AST.md`](AST.md)                   |

Runtime invariants the implementation must keep:

- Kind codes and free recursion stay in sync between interpreter and host.
- Error tags are interned per program so `==` and envelope propagation agree.
- Double free of a handle (region free then variable drop) is a no-op at the host.
- Unsafe host functions are unreachable from safe contexts.

When spec and code disagree, follow the grammar and these docs; close gaps via the crate `PLAN.md` stages.
