# Memory model

How Yarrow allocates, owns, borrows, and frees values. Derived from [`GRAMMAR.md`](GRAMMAR.md), with types and stack effects from [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md) and [`AST.md`](AST.md).

Contents: [Ownership](#ownership), [Borrowing](#borrowing), [Regions](#regions), [Unsafe](#unsafe), [Mental model](#mental-model).

Yarrow is safe by default. Heap lifetimes are tracked at compile time through ownership and borrows. There are no user-visible lifetime parameters on `reference<T>`. Every escape from the safe model is syntactically visible: `unsafe function`, `unsafe ... end`, and `pointer<T>` operations.

---

## Ownership

A value is either **owned** (responsible for freeing heap storage) or **trivial** (scalars, enums, raw pointers, and similar: drop is a no-op). Non-copy heap types (`string`, `list`, `hashmap`, structs, unions, and heap-backed arrays) are owned when created on the stack or stored in a variable.

See [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md) for the copy and non-copy type split.

### Stack ownership

The evaluation stack owns temporary values it pushes:

```yarrow
"temp"   # stack owns the string
pop      # drop: free the string
```

- `pop` removes one value and drops it if owned (or releases a borrow if it is a reference).
- `drop` clears the whole stack and releases every borrow on it.
- Consuming ops (`typeof` on a simple value, arithmetic that takes ownership of temporaries, etc.) drop or consume according to their stack effect.

### Variable ownership

A declaration pops a value and binds it under a name. The variable owns that value until it is overwritten, moved away, or the scope ends.

```yarrow
"hello" myStr mutable string
"world" myStr set    # drops "hello", now owns "world"
# myStr dropped at scope exit
```

Binding ownership:

- **`mutable`**: Owns; `set` drops the previous value
- **`const`**: Owns; set once at runtime
- **`static`**: Owned by the program; initializer known at compile time

Reading a variable:

- **Copy type** -> push a copy (owner unchanged).
- **Non-copy type** -> push a **borrow** (see Borrowing); the variable remains the owner.

### Function parameters

Parameters are moved onto the callee's local stack in declaration order (first declared = deepest).

- Default: move ownership (or pass a borrow for reference parameters).
- `copy`: deep-copy into the local stack; caller keeps ownership of the original.
- `mutable` on `reference<T>`: pointee must be mutable.

Heap-typed receivers and many reference parameters are borrowed from the caller; the callee must not free them.

### Move

```yarrow
source target move
```

Transfers ownership of `source`'s storage to variable `target`. The source is then **moved**: further use or another move is a compile error. `target` drops whatever it previously owned.

```yarrow
myList myList2 move
# myList ...    # error: no longer owns the value
```

### Drop points

Owned heap storage is freed when:

1. The value is `pop`ped or consumed without flowing into another owner
2. A variable is `set` (old value dropped)
3. A variable goes out of scope
4. `drop` clears owned stack slots
5. A region that registered the value is freed (see Regions)
6. `return` drops leftover stack values after taking the return payload

The runtime guards double frees when a value might be dropped both by a region and by a later variable drop.

### Defer

```yarrow
defer
	# statements
end
```

Bodies run at scope exit, in reverse registration order. Typical use: `region.free`, closing files, releasing borrows held only for the scope.

```yarrow
region.create call myRegion const i64
defer myRegion region.free call end
```

---

## Borrowing

A **borrow** is a safe `reference<T>` (or an implicit borrow from reading a non-copy variable). It does not own storage. Dropping or popping the reference **releases** the borrow; the owner is unchanged.

### Creating borrows

Borrows are created by:

- `value borrow`: Pushes `reference<T>`; value must be borrowable (heap / aggregate)
- Read of non-copy variable: Pushes a borrow of the variable's value
- `dup` on non-copy: Not a second owner; use `borrow` (copy types may `dup`)
- Union `Type case` arm: Branch receives `reference<Member>`
- `match` subject: Subject is borrowed for the whole `match`; stack restored after

There can be **only one active borrow** of a given value, but that borrow may move on the stack (passed to calls, rebound with `name const reference<T>`, etc.).

### Releasing borrows

- `pop` the reference
- Consume it in a call that takes `reference<T>` (e.g. method receivers)
- `typeof` on a borrowed heap value releases the borrow and leaves data owned
- End of `match` releases the subject borrow and any arm member borrows
- `drop` releases all borrows on the cleared stack

### Autoderef

Reads through `reference<T>` behave like `T` for arithmetic, comparison, field access, and concatenation (`~`). The reference remains a borrow until released.

```yarrow
self.x self.x *    # autoderef on reference<Point>
```

Writable methods take `reference<T> mutable` so the pointee must be a mutable binding.

### Compile-time borrow rules

The following situations are compile errors:

- Use value while borrowed (mutate / `set` / consuming op)
- `pop` / drop owner while a borrow is live (release the reference first)
- Use or `move` after `move` (use-after-move)
- Second overlapping `borrow` of the same value
- Return or otherwise escape a reference past its owner / region

`unsafe` does **not** disable these checks.

### Match and unions

- Value / error `match`: subject borrowed; cases leave a `bool`; subject restored after.
- Union `match`: arm gets `reference<Member>`; autoderef on reads; borrow ends at arm/`match` end; union value untouched.
- Member types must be distinct; case type must be one of them.

---

## Regions

Regions group heap values so they can be freed **as a unit**. API surface is `std.region` (names in examples: `region.create`, `region.put`, `region.free`).

### Lifecycle

```yarrow
region.create call myRegion const i64
(1 2 3) myListRegion mutable list<i32>
myListRegion myRegion region.put call
drop
myRegion region.free call
# attached values freed with the region
```

1. **Create** - allocate an empty region handle.
2. **Put** - attach an owned value to the region. Nested heap payloads are freed with the parent via kind codes; only top-level puts appear in the region registry.
3. **Free** - free every attached value, then the region. Prefer `defer` so free runs on all exits.

After `put`, the value's lifetime is tied to the region. Escaping a safe reference to region memory past `region.free` (or past the owning scope) is a compile error.

### Interaction with variable drop

If a variable still names a value that a region already freed, the runtime's freed-handle set makes a later scope-exit drop a no-op (no double free). Prefer structuring code so the region outlives (or clearly owns) those bindings, e.g. free in `defer` at the end of the scope that created both.

### What regions are for

- Batch lifetime for graphs of heap objects (structs, lists, strings) without per-object `pop` timing
- Arena-style allocation patterns in safe code
- Not a substitute for `pointer<T>` / manual `mem.allocate`; regions stay inside the safe ownership model

---

## Unsafe

Raw memory leaves the ownership and borrow model for **validity of addresses**. Types, stack effects, ownership of safe values, and borrow checking still apply.

### Visibility

| Construct              | Role                                                                          |
| ---------------------- | ----------------------------------------------------------------------------- |
| `name unsafe function` | Function may perform unsafe ops; **callers** must be in `unsafe`              |
| `unsafe ... end`       | Marks where unsafe ops occur (required even inside an `unsafe function` body) |
| `pointer<T>`           | Typed raw address; pointee type is compile-time only                          |
| `std.mem`              | `allocate`, `free`, and raw word load/store wrappers                          |

Safe by default: every escape is visible at both definition and use.

### Pointer operations (inside `unsafe`)

```yarrow
16 mem.allocate call p mutable pointer<i32>   # address coerces into typed pointer
p 42 store
p load                                  # 42
p 4 + q const pointer<i32>            # byte offset; type stays pointer<i32>
p 123 mem.store call                         # untyped 64-bit word
p mem.load call
7 cp.value set                          # member access autoderefs through pointer<Cell>
cp mem.free call
```

| Op                            | Meaning                                                             |
| ----------------------------- | ------------------------------------------------------------------- |
| `mem.allocate call`           | Allocate `n` bytes; push address (integer / pointer after coercion) |
| `mem.free call`               | Return block to the allocator                                       |
| `pointer value store`         | Typed write of pointee                                              |
| `pointer load`                | Typed read of pointee                                               |
| `pointer + int`               | Byte-offset arithmetic; result stays `pointer<T>`                   |
| `mem.store` / `mem.load`      | Untyped 64-bit words; no pointee type check                         |
| `ptr.field` / `ptr.field set` | Autoderef field access through `pointer<Struct>`                    |

### What unsafe does not do

- Does not turn off the borrow checker
- Does not skip ownership or use-after-move checks on safe values
- Does not skip stack-effect or type checks
- Does not prove raw pointer validity, aliasing, or liveness: that is the programmer's responsibility

### Boundary summary

| Safe                              | Unsafe                              |
| --------------------------------- | ----------------------------------- |
| Stack / variable ownership        | `pointer<T>` address validity       |
| `borrow` / `move` / single borrow | Manual `allocate` / `free`          |
| Regions                           | Untyped `mem.load` / `mem.store`    |
| `reference<T>` autoderef          | Pointer autoderef for fields        |
| Compile-time escape checks        | No lifetime proof for raw addresses |

---

## Mental model

After a value is created:

- **Copy type:** duplicate freely; drop is trivial.
- **Non-copy, owned by the stack:** `pop` / drop / consume frees it.
- **Non-copy, owned by a variable:** `set` / leaving scope / `move` frees or transfers ownership.
- **Borrowed:** at most one live `reference<T>`; `pop` / consume releases the borrow.
- **In a region:** `region.free` frees the whole unit.

`pointer<T>` and `mem.*` are only allowed inside `unsafe`. Borrow rules do not check raw-address validity.

Implementation (JIT handles, kind codes, region registry) lives in `crates/yarrow-core` and must implement this model; the grammar and this document are the authority when they diverge from code.
