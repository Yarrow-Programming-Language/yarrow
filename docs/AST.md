# AST

Abstract syntax tree for Yarrow, derived from [`SYNTAX.md`](SYNTAX.md) and illustrated by [`GRAMMAR.md`](GRAMMAR.md). This document describes the AST as implemented in `crates/yarrow-core/src/parser/ast.rs`.

Yarrow is postfix and stack-based. Control-flow and declarations form a conventional tree. Stack terms (`word` in the grammar) are kept as flat expression nodes: an operator in word position is a postfix `ApplyBin` / `ApplyUn` node over the runtime stack, not a nested binary tree of operands, so every word keeps its own span. A few words (`call`, `unwrap`, `store`) reassociate their operands into nested nodes because their operand is the word directly before them.

Notation:

- Algebraic datatypes in a Rust-like sketch
- `Vec<T>` for ordered repetition
- `Option<T>` for optional children
- Spans / source locations are assumed on every node and omitted below

Contents: [Program](#program), [Visibility and modifiers](#visibility-and-modifiers), [Type](#type), [Expression](#expression), [Statement](#statement), [Match cases](#match-cases), [Declarations](#declarations-top-level-and-nested), [Mapping from surface syntax](#mapping-from-surface-syntax), [Example](#example).

---

## Program

A program is a flat sequence of top-level items. The entry function is not a distinguished node: it is the top-level `Function` whose name matches the session's entry name (default `main`, override via `SessionOptions::entry_name`).

```rust
Program {
	items: Vec<Stmt>,
}

impl Program {
	fn entry_function(&self, name: &str) -> Option<(&Function, Span)>;
	fn has_entry(&self, name: &str) -> bool;
}
```

`main` is the only entity public by default. It has no parameter list in surface syntax. A missing entry is rejected by the session (`E360`), not by the parser.

---

## Visibility and modifiers

```rust
Visibility = Public | Private

Mutability = Mutable | Const | Static

ParamModifier = Copy | Mutable   // on a parameter type
// Copy: deep-copy into the local stack
// Mutable: on reference<T>, requires a mutable pointee
```

Default visibility for user-declared entities and fields is private; `main` is public by default.

---

## Type

Types appear in declarations, parameters, return clauses, `typeof` comparisons, and union match cases.

```rust
Type {
	kind: TypeKind,
	location: Location,
}

TypeKind =
		Named(String)                    // user type or scoped name
	| Primitive(Primitive)
	| Array { element: Box<Type>, size: Option<u64> }
	| List { element: Box<Type> }
	| Hashmap { key: Box<Type>, value: Box<Type> }
	| Reference { inner: Box<Type> }
	| Pointer { inner: Box<Type> }
	| Union(Vec<Type>)                  // |T U ...| anonymous union

Primitive =
		Void | Bool | String | Rune
	| I8 | I16 | I32 | I64
	| U8 | U16 | U32 | U64
	| F16 | F32 | F64
	| Error                            // legacy catch-all error tag
	| Type                             // a type used as a value
```

Notes from the grammar:

- Type arguments inside `<>` are whitespace-separated (`array<i32 3>`, `hashmap<string i32>`).
- A qualified type name is stored as a dotted `Named` string (`error.MyErr`, `m.Point`), not a list of parts.
- Empty container literals carry no element type until a typed context (e.g. a variable declaration) supplies one.
- `pointer<T>` is a typed raw address at compile time; at runtime it is an address.
- There is no lifetime syntax on `reference<T>`; ownership and regions are the model.

---

## Expression

An expression is one stack term: a push, a name lookup, an operator, or a keyword op. Sequences of expressions are statements (`StmtKind::Expr`) or appear inside container literals and match conditions.

```rust
Expr =
		Integer { value: String }        // raw lexeme; decoded later
	| Float { value: String }
	| String { value: String }
	| Rune { value: String }
	| Bool { value: bool }
	| Variable { name: String }
	| Member { base: Box<Expr>, member: String }   // a.b: field, enum member, module item
	| TypeValue { ty: Type }           // a type used as a stack word
	| Call { target: Box<Expr> }       // `callee call`
	| Unwrap { inner: Box<Expr> }      // `value unwrap`
	| Typeof { inner: Box<Expr> }      // nested form; the parser emits ApplyTypeof
	| Borrow { inner: Box<Expr> }      // nested form; the parser emits ApplyBorrow
	| Load { inner: Box<Expr> }        // nested form; the parser emits ApplyLoad
	| Store { addr: Box<Expr>, value: Box<Expr> }   // `addr value store`
	| Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> }  // not produced by the parser
	| Unary { op: UnOp, operand: Box<Expr> }                  // not produced by the parser
	| ApplyBin(BinOp)                  // binary op over the runtime stack
	| ApplyUn(UnOp)                    // unary op over the runtime stack
	| ApplyTypeof                      // `typeof` over the runtime stack
	| ApplyBorrow                      // `borrow` over the runtime stack
	| ApplyLoad                        // `load` over the runtime stack
	| Builtin { name: String }         // `@name` raw host word
	| StackOp(StackOp)
	| Array(Vec<Expr>)                 // [ ... ]
	| List(Vec<Expr>)                  // ( ... )
	| Map(Vec<(Expr, Expr)>)           // { k v ... } with literal keys
	| StructLit(Vec<(String, Expr)>)   // { field value ... } with identifier keys
	| EmptyMapOrStruct                 // {} needs a typed context
	| Seq(Vec<(Expr, Span)>)           // several words flushed as one value; flat
```

Which form the parser emits:

- Word-position `typeof`, `borrow`, and `load` emit the flat `ApplyTypeof` / `ApplyBorrow` / `ApplyLoad` nodes, because their operand is whatever sits on the runtime stack.
- `call`, `unwrap`, and `store` take their operands from the words directly before them, so they reassociate into the nested `Call` / `Unwrap` / `Store` nodes.
- `Binary` / `Unary` and the nested `Typeof` / `Borrow` / `Load` forms are accepted by consumers (checker, interpreter, formatter) but are not produced by the parser.
- `Seq` groups the words drained into a declaration, condition, or value slot; its elements are flat (never nested `Seq`) and each keeps its own span.

### Operators and keyword ops

```rust
BinOp =
		Plus | Minus | Mul | Div | Fdiv | Mod | Pow
	| Concat                           // `~`; string join, not overloaded `+`
	| Eq | Ne | Gt | Gte | Lt | Lte
	| And | Or | Xor | Lshift | Rshift

UnOp = Not
// `and` / `or` / `not` are overloaded: bool → logical, integer → bitwise
// `+` is arithmetic / pointer offset only; concatenation is `~`

StackOp  = Dup | Swap | Rot | Unrot | Pop | Drop
```

Stack effects (informal):

| Node                                                               | Effect                        |
| ------------------------------------------------------------------ | ----------------------------- |
| Literal / Container / Variable / Member / TypeValue / Builtin      | push                          |
| `ApplyBin`, `Store`                                                | pop 2, push 0 or 1            |
| `ApplyUn`, `ApplyTypeof`, `ApplyBorrow`, `ApplyLoad`, `Dup`, `Pop` | pop 1 (push 0 or 1)           |
| `Call`, `Unwrap`                                                   | pop callee / operand, push    |
| `Drop`                                                             | clear stack / release borrows |
| `Swap` / `Rot` / `Unrot`                                           | rearrange                     |

Exact typing and ownership of these effects belong to the type and memory models.

---

## Statement

Statements are the sequenced body of functions, blocks, and top-level items that are not pure declarations.

```rust
Stmt {
	kind: StmtKind,
	span: Span,
}

StmtKind =
		Expr(Expr)                       // one stack term as a statement
	| VarDecl { name: String, mutability: Mutability, ty: Type, value: Option<Expr> }
	| Set { target: Expr, value: Option<Expr> }
	| Function(Function)
	| Struct(StructDecl)
	| Implement(Implement)
	| Enum(EnumDecl)
	| Union(UnionDecl)
	| Error(ErrorDecl)
	| Require { path: String, alias: Option<String> }
	| If { condition: Expr, then_branch: Vec<Stmt>, else_branch: Vec<Stmt> }
	| For { source: Expr, body: Vec<Stmt> }
	| Match { value: Expr, cases: Vec<MatchCase>, else_branch: Vec<Stmt> }
	| Defer { body: Vec<Stmt> }
	| Handle { body: Vec<Stmt>, fallback: Option<Expr> }
	| Move { target: String, source: Expr }
	| Fallback { value: Option<Expr> }
	| Return { value: Option<Expr> }
	| Unsafe { body: Vec<Stmt> }
```

Nested `function` declarations are allowed inside bodies; they are only callable from that enclosing body.

### Reassociation of preceding words

The parser keeps an operand stack while scanning a body. Words are pushed as they appear; a declaration or control-flow keyword drains the pending words into its payload:

- `42 myVar mutable i32` -> `VarDecl { value: Some(Integer(42)) }`
- `23 myVar set` -> `Set { target: Variable(myVar), value: Some(Integer(23)) }`
- `5 10 < if` -> `If { condition: Seq([5, 10, <]) }`
- `numbers for` -> `For { source: Variable(numbers) }`
- `score match` -> `Match { value: Variable(score) }`
- `myList myList2 move` -> `Move { target: myList2, source: Variable(myList) }`

A multi-word drain becomes one flat `Seq`; a single word is stored directly. `else_branch` is an empty `Vec` when `else` is absent.

### Require

```rust
Require {
	path: String,                    // decoded string literal module path
	alias: Option<String>,            // omit → import into current scope
}
```

Surface form: `"path" [alias] require`.

### Function

```rust
Function {
	name: String,
	visibility: Option<Visibility>,
	params: Vec<Parameter>,
	body: Vec<Stmt>,
	returns: Vec<Type>,               // `with T` is one entry
	is_unsafe: bool,
}

Parameter {
	ty: Type,
	modifier: Option<ParamModifier>,  // copy | mutable
}
```

Parameters are moved onto the local stack in declaration order (first declared = deepest). Body bindings such as `name const Type` pop from the top, so the last parameter binds first.

A `with |T Err|` return is a single `TypeKind::Union` entry. `returns` is empty when `with` is omitted (void).

`unsafe function` marks the function as unsafe to call. Call sites still need an `unsafe` block. Unsafe does not disable borrow or ownership checking.

### Variables and assignment

- `mutable` / `const`: runtime storage; declaration pops and owns.
- `static`: compile-time constant; initializer must be known at compile time.
- Reading a variable pushes a copy (copy types) or a borrow (non-copy types).
- `Set` on a field target stores a `Member` in `target`; the value may be `None` when the new value is the runtime stack top.

### Control flow

- `If`: condition reassociated from the words before `if`; branches checked independently.
- `For`: `source` is the condition (bool) or the iterable drained from before `for`; iteration value/index come from `std.loop`.
- `Match`: `value` is the subject; the subject is borrowed for the duration and the original stack is restored afterward.
- `Defer`: bodies run at scope exit; multiple defers run in reverse registration order.
- `Unsafe`: marks where unsafe ops occur; the body interprets like any other block.
- `Handle`: wraps a preceding fallible call. The body may contain one `Fallback` statement; the parser extracts it into `fallback` and the remaining statements stay in `body`.
- `Return`: the parser flushes the return value as a preceding `Expr` statement and emits `Return { value: None }`; the value field exists for consumers that build returns programmatically.

---

## Match cases

Match-case discriminators appear only inside `Match`.

```rust
MatchCase {
	kind: MatchCaseKind,
	body: Vec<Stmt>,
	span: Span,                       // condition/type through the closing `end`
}

MatchCaseKind =
		Condition(Expr)               // words before `case` producing a bool
	| Type(Type)                      // union member type before `case`
```

Examples:

```text
dup 85 == case                 → Condition(Seq([StackOp(Dup), Integer(85), ApplyBin(Eq)]))
i32 case                       → Type(Primitive(I32))
error.OUT_OF_MEMORY == case    → Condition(Seq([Member(error.OUT_OF_MEMORY), ApplyBin(Eq)]))
```

The condition form stores the flat word sequence from the grammar (`word { word } case`) as one `Expr` (a `Seq` when several words), each element keeping its own span.

---

## Declarations (top-level and nested)

These are `StmtKind` variants and, where the grammar allows, embeddable statements (`Require`, `Function`).

### Struct

```rust
StructDecl {
	name: String,
	visibility: Option<Visibility>,
	fields: Vec<Field>,
}

Field {
	name: String,
	ty: Type,
	visibility: Option<Visibility>,
}
```

### Implement

```rust
Implement {
	target: String,                  // type being implemented
	functions: Vec<Function>,
}
```

Methods are ordinary `Function` nodes. Receivers appear as the first parameter type (typically `reference<T>` or `reference<T> mutable`).

### Enum

```rust
EnumDecl {
	name: String,
	underlying: Option<Type>,         // default i32 if absent
	members: Vec<EnumMember>,
	visibility: Option<Visibility>,
}

EnumMember {
	name: String,
	value: Option<String>,            // explicit discriminant; raw integer lexeme
}
```

Without an explicit value, members get the next implicit ordinal starting at 0.

### Union

```rust
UnionDecl {
	name: String,
	types: Vec<Type>,                 // at least one; member types must be distinct
	visibility: Option<Visibility>,
}
```

### Error

```rust
ErrorDecl {
	name: String,
	inject: Option<String>,           // dotted path of another error type
	members: Vec<String>,
	visibility: Option<Visibility>,
}
```

Error types behave like enums specialized for error handling and `|T Err|` return unions. The optional injection copies members from another error type (`MyErr error.Error error ... end`).

---

## Mapping from surface syntax

| Syntax (EBNF)     | AST node                                       |
| ----------------- | ---------------------------------------------- |
| `program`         | `Program`                                      |
| `main_function`   | `Function` named `main` (entry found by name)  |
| `require_stmt`    | `Require`                                      |
| `function_decl`   | `Function`                                     |
| `struct_decl`     | `Struct`                                       |
| `implement_block` | `Implement`                                    |
| `enum_decl`       | `Enum`                                         |
| `union_decl`      | `Union`                                        |
| `error_decl`      | `Error`                                        |
| `var_decl`        | `VarDecl`                                      |
| `assignment`      | `Set`                                          |
| `move_stmt`       | `Move`                                         |
| `if_stmt`         | `If`                                           |
| `match_stmt`      | `Match`                                        |
| `match_case`      | `MatchCase` + `MatchCaseKind`                  |
| `for_stmt`        | `For`                                          |
| `defer_stmt`      | `Defer`                                        |
| `unsafe_block`    | `Unsafe`                                       |
| `handle_stmt`     | `Handle`                                       |
| `fallback_stmt`   | `Fallback` (extracted into `Handle::fallback`) |
| `return_stmt`     | `Return`                                       |
| `word`            | `Expr` (via `StmtKind::Expr` or nested)        |
| `type`            | `Type` / `Expr::TypeValue`                     |
| `lvalue`          | `Expr` (`Variable` or `Member`)                |
| `builtin`         | `Builtin`                                      |

Operands that sit on the stack before a keyword (`if`, `for`, `match`, `set`, declarations, `move`) are reassociated into that node's payload by the parser, as one flat expression (`Seq` when several words), preserving stack order for checking.

---

## Example

Source:

```yarrow
add function
	i32
	i32
do
	+
	return
end with i32

main function do
	3 4 add call
end
```

AST (sketch):

```text
Program
	items:
	Function
		name: add
		params: [i32, i32]
		body: [Expr(ApplyBin(Plus)), Return]
		returns: [i32]
	Function
		name: main
		body:
			Expr(Integer(3))
			Expr(Integer(4))
			Expr(Call { target: Variable(add) })
		returns: []
```
