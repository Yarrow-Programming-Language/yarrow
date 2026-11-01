# Style guide

How Yarrow source (`.yar`) should look. Language rules live in [`GRAMMAR.md`](GRAMMAR.md) and [`SYNTAX.md`](SYNTAX.md); this document is about layout, naming, and idiomatic form.

The goal is one readable default style so programs look familiar and diffs stay small. Tools and formatters (`yarrow fmt`, `yarrow-fmt`) should target this guide. By default they sort top-level `require` lines (std first, then local); pass `--no-sort-requires` to keep source order. Soft wrap defaults to 100 columns (`--max-width`, minimum 20). Indent is always tabs. Diff-sensitive regions can opt out with paired `# yarrow-fmt-ignore-begin` / `# yarrow-fmt-ignore-end` comments (see [Comments](#comments)). Span edits for scripts use `--range START:END` (byte offsets); editors should keep using LSP range formatting.

---

## Principles

1. **Postfix clarity** - Write stack phrases so left-to-right reading matches evaluation order: operands, then the word that consumes them.
2. **One idea per line** - Prefer a single stack effect or statement per line unless a short phrase clearly belongs together.
3. **Visible structure** - Nesting is always `... do/if/match/for/defer/unsafe/case ... end`. Keep `end` aligned with the opener's indent.
4. **Safe by default** - Mark `public` and `unsafe` only where needed. Do not sprinkle redundant keywords.
5. **Match the grammar tour** - When unsure, follow the shape in [`GRAMMAR.md`](GRAMMAR.md) and [`examples`](examples/README.md).

---

## Source files

Source file conventions:
- **Encoding**: UTF-8
- **Line endings**: LF (`\n`)
- **Trailing whitespace**: None on any line
- **Final newline**: Required
- **File extension**: `.yar`
- **Module path**: Dots map to directories: `"a.b"` -> `a/b.yar`

One public module per file. Nested helpers may live in the same file or in a sibling path under a package directory.

---

## Indentation and line width

- Indent with **one tab per nesting level** (same as [`GRAMMAR.md`](GRAMMAR.md) and the examples).
- Do not mix tabs and spaces for indent.
- Soft wrap target: **100 columns**. Break long stack phrases before a consuming word (`call`, `set`, an operator, `if`, ...) rather than mid-literal when possible.
- Continuation lines indent one level deeper than the start of the phrase.

```yarrow
# Prefer
self.scores score list.push_last call unwrap

# Long call: break before the callee / `call`
veryLongArgumentName
otherArg
module.very_long_function_name call
```

---

## Blank lines

- Separate **top-level items** (requires block, types, implement blocks, functions) with a **single** blank line.
- Inside a function body, use a blank line between logical groups (setup, work, cleanup), not after every line.
- Between `match` cases, prefer a blank line when cases are multi-line; single-line cases may stay tight.
- Do not use more than one consecutive blank line.
- No blank line immediately after `do` / `if` / `else` / `case` / `for` / `defer` / `unsafe` or immediately before the matching `end`, unless it improves a dense multi-branch block.

```yarrow
Point struct
	i32 x public
	i32 y public
end

Point implement
	distance public function
		reference<Point>
	do
		self const reference<Point>
		self.x self.x * self.y self.y * +
		return
	end with i32
end
```

---

## Comments

- Line comments only: `#` through end of line (see grammar).
- Put a single space after `#`.
- Prefer a comment on its own line above the code it describes.
- Trailing comments are fine for short notes; put one space before `#`.
- Comments are sentences or short phrases. Prefer complete sentences for non-obvious rationale.
- Do not use decorative comment banners in ordinary code. Reserve section banners for long illustrative files (as in the grammar tour).
- Never use an em dash in comments.
- **Formatter ignore regions:** to keep a contiguous block byte-stable under `yarrow fmt` / `yarrow-fmt` (aside from source hygiene), wrap it in paired whole-line comments:

```yarrow
# yarrow-fmt-ignore-begin
# Hand-aligned table for a doc example; do not reflow.
messy   function   do
  1    drop
end
# yarrow-fmt-ignore-end
```

- Markers must be own-line comments; the directive is `yarrow-fmt-ignore-begin` / `yarrow-fmt-ignore-end` after `#` (optional trailing note after the directive is allowed).
- The region includes both marker lines and everything between them.
- Ignored text still gets source hygiene: LF endings, no trailing whitespace, final newline. Construct layout, indent, blank-line rules, require sorting, and file-layout reorder do not rewrite the interior.
- Prefer small regions. Do not use ignores to paper over formatter bugs in the shared corpus; fix the printer instead.
- Unmatched `begin` runs through end of file; a stray `end` is an ordinary comment. Nesting is not supported.
- `--reorder-layout` is skipped for a file that contains ignore markers so begin/end pairs stay intact. Require runs that intersect an ignore region are not sorted.

```yarrow
# Literal u8 coerces to i32 at the declaration site.
42 answer mutable i32

5 10 < if
	"less" io.write_line call   # condition already on the stack
end
```

---

## Naming

Naming conventions by kind:
- **Types** (struct, enum, union, error): `PascalCase` - Examples: `Point`, `Color`, `AppError`, `MyUnion`
- **Functions and methods**: `snake_case` - Examples: `write_line`, `push_last`, `open_file`
- **Variables and parameters**: `camelCase` - Examples: `myList`, `score`, `fileHandle`
- **Module aliases**: `snake_case` - Examples: `io`, `list`, `region`
- **Enum / error members**: `SCREAMING_SNAKE` - Examples: `RED`, `NOT_FOUND`, `OUT_OF_MEMORY`
- **File / module path segments**: `snake_case` - Examples: `"helpers.greet"`, `"std.mem"`

### Guidelines

- Names are ASCII letters, digits, and underscores (identifiers). Prefer `camelCase` for variables and parameters (no underscores between words).
- Prefer full words over cryptic abbreviations (`index` not `idx`, unless the domain standard is short).
- Receiver bindings in methods: use `self` for the `reference<T>` (or `reference<T> mutable`) parameter after it is bound.
- Boolean names read as predicates when practical (`found`, `done`); avoid `is` / `has` prefixes unless they clarify (`ready` over `isReady` when both read well).
- Fallible helpers name the success path; errors live in the `with |T Err|` type, not in the function name (`lookup` not `try_lookup`).

---

## File layout

Recommended order in a module file:

1. File comment (optional, short)
2. `require` lines (grouped; std first, then local)
3. Type declarations (`struct`, `enum`, `union`, `error`)
4. `Type implement` blocks (immediately after the type they extend when practical)
5. Private helpers
6. Public API functions
7. `main` (entry files only; conventionally last)

```yarrow
# Optional one-line file summary.

"std.io" io require
"std.list" list require
"helpers.greet" greet require

Point struct
	i32 x public
	i32 y public
end

Point implement
	# methods...
end

helper function do
	# ...
end

main function do
	# ...
end
```

---

## Modules and `require`

- Form: `"path" [alias] require` (keyword last).
- Put requires at the **top of the file** or at the **start of a function** when the import is local to that function.
- Prefer an alias for multi-export modules (`"std.io" io require` -> `io.write_line`).
- Omit the alias only for a single-item import or when names enter the current scope without clash (`"std.math.sqrt" require`).
- One `require` per line.
- Sort requires: standard library (`"std...."`) first, then third-party / local paths, alphabetically within each group if the list grows.

```yarrow
"std.io" io require
"std.list" list require
"std.region" region require

"helpers.greet" greet require
```

---

## Visibility

- Default is **private** for user-declared entities and fields. Omit `private` unless you are documenting intent in an API sketch.
- Mark exports with `public` on the type, field, or function that should leave the module.
- `main` is public by default; do not write `main public function`.
- In `implement` blocks, mark methods `public` only when callers outside the module need them.

```yarrow
Point struct
	i32 x public
	i32 y          # private by default
end

distance public function
	reference<Point>
do
	# ...
end with i32
```

---

## Types

### Struct

```yarrow
Name [visibility] struct
	Type field [visibility]
	...
end
```

- One field per line: type, then name, then optional visibility.
- Keep related fields together; no blank lines between fields unless grouping large structs.

### Enum and error

```yarrow
Color enum
	RED
	GREEN
	BLUE
end

AppError error
	NOT_FOUND
	BAD_INPUT
end
```

- One member per line.
- Use explicit discriminants only when the numeric (or carrier) value matters; keep the list readable.

### Union

```yarrow
Value union
	i32
	string
end
```

- One member type per line.
- Prefer a named union for reused shapes; use `|T Err|` for fallible returns.

### Implement

- Place `Name implement ... end` next to `Name`'s declaration when both are in the same file.
- Methods follow the same formatting as functions.
- First parameter is usually `reference<T>` or `reference<T> mutable`. Bind it as `self` on the first lines of the body when the method uses the receiver more than once.

---

## Functions

```yarrow
name [visibility] [unsafe] function
	Type [copy | mutable]
	...
do
	# body
end [with Type]
```

### Layout rules

- Put each parameter type on its own line between `function` and `do`.
- No parameter list when there are none: `name function do` ... `end`.
- Put `with Type` on the same line as `end` when present: `end with i32`, `end with |i32 AppError|`.
- Omit `with` for `void` returns.
- Nested functions sit inside the enclosing body, indented one level, and appear before the code that calls them when that keeps the story clear.

```yarrow
demo function do
	add function
		i32
		i32
	do
		+
		return
	end with i32

	3 4 add call
	drop
end
```

### Calls

- Push arguments in declaration order (first declared = deepest), then the callee name, then `call`.
- Keep `name call` (or `alias.name call`) on the same line as the last argument when the whole phrase stays short.

```yarrow
3 4 add call
"Hello, Yarrow!" io.write_line call
person borrow
person.greet call unwrap
```

---

## Variables

Form: `<value> <name> (mutable | const | static) <Type>`

```yarrow
42 answer mutable i32
7 limit const i32
3 piApprox static i32
```

- Prefer `const` when the binding is not reassigned; `mutable` only when `set` (or mutation through a mutable reference) is required; `static` only for compile-time constants.
- Declaration and `name set` stay on one line each.
- Do not rebind with a new declaration to emulate mutation; use `set`.

---

## Stack phrases and operators

- Space-separate words. Never jam tokens together (`1 2 +`, not `1 2+`).
- One primary effect per line. Short pure arithmetic may stay on one line:

```yarrow
1 2 +
self.x self.x * self.y self.y * +
```

- Comparison that feeds control flow sits on the line before the keyword (or on the same line if tiny):

```yarrow
5 10 < if
	# ...
end
```

- **Arithmetic** uses `+ - * / // % ^`. **String concatenation** uses `~`. Do not use `+` for strings.
- Prefer named stack ops (`dup`, `swap`, `rot`, `unrot`, `pop`, `drop`) over clever reshuffles when clarity suffers.
- `drop` clears the stack deliberately; do not leave junk values ""for later"" across unrelated sections.
- After a borrow is finished, `pop` (or consume it in a call) before mutating the owner.

---

## Literals and containers

Literal forms and their uses:
- `(...)`: `list` literals
- `[...]`: `array` literals
- `{ k v ... }`: `hashmap` when keys are literals
- `{ field value ... }`: struct literals when keys are identifiers
- `{}` / `()` / `[]`: empty; only with a typed binding

```yarrow
[10 20 30] numbers static array<i32 3>
(43 54 65) myList static list<i32>
{"first" 4 "second" 5} myMap static hashmap<string i32>
{x 5 y 20} point mutable Point
() empty mutable list<i32>
```

- Put spaces between elements inside containers: `[10 20 30]`, not `[10,20,30]` (no commas in the grammar).
- Use `_` in numeric literals for readability: `1_000`, `0xAB12`.
- Prefer double quotes for strings and single quotes for runes: `"hello"`, `'\n'`.

---

## Control flow

### `if` / `else`

```yarrow
condition if
	# then
else
	# else
end
```

- Condition is a `bool` already on the stack; do not invent a parenthesized test syntax.
- Always use `end`. Include `else` only when needed.
- No else-if chain; use `match` for multi-way branches.

### `match` (value)

```yarrow
subject match
	dup 85 == case
		# ...
	end

	dup 50 < case
		# ...
	end

	else
		# ...
	end
end
```

- Indent cases one level; each `case` ... `end` is its own block.
- Keep the subject borrowed for the whole match; restore mental model that the prior stack returns after `end`.

### `match` (union / error)

```yarrow
val match
	i32 case
		# reference<i32> on the stack
	end

	string case
		msg const reference<string>
		# ...
	end
end
```

- Use `Type case` for unions; bind the reference when the arm is more than a tiny phrase.

### `for`

```yarrow
counter 5 < for
	# while-style
end

numbers for
	# iterable; use std.loop helpers as needed
end
```

---

## Defer

- Register cleanup early, right after acquiring the resource.
- Prefer a one-line `defer ... end` when the body is a single short call; otherwise use a block.

```yarrow
region.create call myRegion const i64
defer myRegion region.free call end

defer
	file fs.close_file call
end
```

- Remember: multiple defers run in reverse registration order; statements inside one defer still run top to bottom.

---

## Ownership, borrow, move, regions

Style (not the full memory model; see [`MEMORY_MODEL.md`](MEMORY_MODEL.md)):

- Keep borrow spans short and obvious: `borrow`, use, then `pop` or consume.
- After `move`, do not mention the source name again.
- Pair `region.create` with `defer ... region.free ... end` unless free is intentionally elsewhere.
- Prefer regions for graphs of heap values freed as a unit; prefer `pop` / scope drop for simple locals.

```yarrow
(1 2 3) xs mutable list<i32>
xs borrow
# use reference...
pop
xs 4 list.push_last call unwrap
```

---

## Unsafe

- Mark the function `unsafe function` when its API requires unsafe ops.
- Still wrap the ops in `unsafe ... end` at the use site (including inside the unsafe function).
- Callers of unsafe functions must sit inside `unsafe ... end`.
- Keep unsafe blocks as small as possible: allocate, touch memory, free.
- Prefer `pointer<T>` typed `load` / `store` over raw `mem.load` / `mem.store` when the pointee type is known.
- Use `mem.allocate` / `mem.free` for the std memory API names.

```yarrow
touch private unsafe function do
	unsafe
		16 mem.allocate call p mutable pointer<i32>
		p 42 store
		p load
		drop
		p mem.free call
	end
end

main function do
	unsafe
		touch call
	end
end
```

---

## Errors

- Fallible returns use a union: `end with |T Err|` (or `|void Err|`).
- Prefer `unwrap` when the caller's `with` can propagate the same error type and failure should abort the happy path.
- Prefer `handle` when recovery or a fallback value belongs at the call site.
- Short form when there is no handler body: `call handle <fallback> fallback end`.

```yarrow
5 lookup call unwrap

0 lookup call handle
	match
		AppError.NOT_FOUND case
			"missing" io.write_line call
		end

		else
			"other error" io.write_line call
		end
	end

	-1 fallback
end

0 lookup call handle 0 fallback end
```

---

## Stack hygiene

- Leave the stack as the next reader expects: document lingering values with a short trailing comment when non-obvious (`# Stack: [25]` in tutorials; rare in production code).
- Pop or drop temporaries before starting an unrelated section.
- Do not rely on leftover values across `if` / `match` arms; arms that join must agree on stack shape (language rule) and should agree on intent (style).

---

## Checklist

Before merging Yarrow code:

- [ ] Tabs for indent; no trailing whitespace; final newline
- [ ] `PascalCase` types, `snake_case` functions, `camelCase` variables/parameters, `SCREAMING_SNAKE` enum/error members
- [ ] Requires grouped at the top (or function-local when scoped)
- [ ] `public` / `unsafe` only where required
- [ ] Functions: parameters between `function` and `do`; `end with Type` when returning
- [ ] Strings joined with `~`, never `+`
- [ ] Unsafe ops only inside `unsafe ... end`
- [ ] Borrows released before mutating owners; moves not used again
- [ ] Defers registered next to resource acquisition
