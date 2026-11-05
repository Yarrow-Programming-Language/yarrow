//! Static checking pass (PLAN Stage 3).
//!
//! The session pipeline is tokenize -> parse -> check -> interpret. This
//! module owns the check step: it walks the [`Program`] AST with access to
//! the declarations registered from that program (structs, enums, unions,
//! error types, functions) and collects diagnostics into a
//! [`DiagnosticBatch`].
//!
//! PLAN 3.1 (borrow rules) is implemented as an abstract interpretation of
//! the operand stack. The checker tracks one abstract slot per stack value:
//! owned temporaries, copy values, live borrows, and opaque values. Borrows
//! are created by the `borrow` word and by reads of non-copy variables; they
//! end at `pop`, at consumption by a call, at `typeof`, at `match` end, or
//! at scope exit. Two rules are enforced:
//!
//! - E375: a second live borrow of the same value (single-borrow rule).
//! - E374: `pop` / `drop` of an owner while a borrow of its value is live.
//!
//! PLAN 3.2 (move rules) builds on the same machine: `move` empties the
//! source binding (a `set` or a re-declaration re-owns the name), and
//!
//! - E373: use of a variable after `move` transferred its value.
//! - E374: `move` of a variable whose value has a live borrow.
//!
//! PLAN 3.3 (copy rules) restricts `dup` to copy types:
//!
//! - E371: `dup` over an owned heap temporary, a non-copy value. Duplicating
//!   a borrow or reference is legal (audit-3 PLAN 1.2.1).
//!
//! PLAN 3.4 (static variables) restricts `static` initializers to
//! compile-time constants:
//!
//! - E377: a `static` initializer that reads a variable, calls, or uses a
//!   runtime word (`borrow`, `load`, `store`, `@builtins`, `unwrap`), or a
//!   `static` declaration with no initializer at all (it would bind the
//!   runtime stack).
//!
//! PLAN 3.5 (unsafe boundary) makes the unsafe escapes visible at check
//! time. An unsafe context is an active `unsafe ... end` block only: the
//! `unsafe` marker on a function is declarative, so an unsafe function's
//! body needs its own block (audit-3 Stage 5); the borrow and ownership
//! rules above stay active inside it (the unsafe walk uses the same
//! machine):
//!
//! - E370: a call to an `unsafe function` outside an unsafe context.
//! - E370: an unsafe operation outside an unsafe context: the `@alloc`,
//!   `@free`, `@load`, `@store` builtins, `load` / `store` through
//!   `pointer<T>`, and pointer arithmetic (`+` / `-` over a pointer).
//!
//! PLAN 5.2 (unwrap in a non-fallible caller) tracks the fallibility of
//! the function being checked (Stage 4 predicate):
//!
//! - E379: `unwrap` over a call to a visible fallible function in a
//!   function whose own return is not fallible. The rule covers what the
//!   checker can see; opaque operands (module calls, builtins, variables)
//!   keep the runtime E308 backstop, and `handle` is allowed anywhere.
//!
//! PLAN Stage 6 (static type checking) extends the abstract slots with a
//! static type: the interpreter's kind-code scheme (primitive kind codes
//! plus named-type resolution through this unit's declarations). The
//! coercion matrix: a value binds to a declared type when the kinds are
//! equal; an integer value binds to any integer type (widening, with a
//! literal's value range-checked when statically known); a float value
//! binds to any float type; every other combination is an error. Module
//! functions and module types stay opaque, so cross-unit calls are
//! unchecked, exactly like the borrow-machine simplification.
//!
//! - E310: a binding (`const` / `mutable` / `static` declaration or `set`)
//!   whose value does not coerce to the declared type.
//! - E311: an operator whose operands do not match (arithmetic needs
//!   numbers, `//` `%` `^` and the shifts need integers, `and` / `or` /
//!   `xor` / `not` need bools or integers, `~` needs strings,
//!   comparisons need matching families).
//! - E312: a call to a visible function with too few operands, or with an
//!   operand that does not coerce to its parameter type.
//! - E313: an `if` / `for` condition or `match` case condition that is
//!   not bool.
//!
//! Simplifications (documented per PLAN 3.1): calls whose signature is not
//! visible to the checker (module functions reached through `require`
//! aliases) are assumed to consume every live on-stack borrow and to push
//! one opaque value; borrows bound into a `reference<T>` variable stay live
//! until that variable is set, moved, or goes out of scope; field access
//! pushes an opaque value and creates no tracked borrow.
//!
//! Declarations from `require`d modules are not visible here: module loading
//! is an interpreter-side step that runs after the check. Rules that need
//! cross-module knowledge extend this in later tasks.

use std::collections::HashMap;

use crate::diagnostics::{Diagnostic, DiagnosticBatch, Span};
use crate::interpreter::{int_carrier_range, primitive_kind_code, primitive_to_kind};
use crate::parser::ast::{
    BinOp, EnumDecl, ErrorDecl, Expr, Function, MatchCaseKind, Mutability, Primitive, Program,
    StackOp, Stmt, StmtKind, StructDecl, Type, TypeKind, UnionDecl,
};
use crate::parser::literals::decode_int_literal;
use crate::signature::{Signature, classify_signature};

/// Diagnostic code reserved for the single-borrow rule (PLAN 3.1.1).
const CODE_SECOND_BORROW: &str = "E375";
/// Diagnostic code reserved for dropping an owner while borrowed (PLAN 3.1.2).
const CODE_OWNER_WHILE_BORROWED: &str = "E374";
/// Diagnostic code reserved for use after `move` (PLAN 3.2.1).
const CODE_USE_AFTER_MOVE: &str = "E373";
/// Diagnostic code reserved for `move` of an owner while borrowed (PLAN 3.2.2).
const CODE_MOVE_WHILE_BORROWED: &str = "E374";
/// Diagnostic code reserved for `dup` on a non-copy type (PLAN 3.3.1).
const CODE_DUP_NON_COPY: &str = "E371";
/// Diagnostic code reserved for a non-constant `static` initializer
/// (PLAN 3.4.1).
const CODE_STATIC_NON_CONST: &str = "E377";
/// Diagnostic code reserved for an unsafe operation outside an unsafe
/// context (PLAN 3.5).
const CODE_UNSAFE_OUTSIDE: &str = "E370";
/// Diagnostic code reserved for an unresolvable error side in a fallible
/// signature (PLAN 4.2).
const CODE_UNKNOWN_ERROR_TYPE: &str = "E386";
/// Diagnostic code reserved for `set` on a `const` or `static` binding
/// (PLAN 5.1.1).
const CODE_SET_IMMUTABLE: &str = "E378";
/// Diagnostic code reserved for `unwrap` in a non-fallible caller
/// (PLAN 5.2.2).
const CODE_UNWRAP_NON_FALLIBLE: &str = "E379";
/// Diagnostic code reserved for a union whose member types contain a
/// duplicate (PLAN 5.3.1).
const CODE_UNION_DUPLICATE_MEMBER: &str = "E385";
/// Diagnostic code reserved for an `if` whose branches leave incompatible
/// stacks at the join (PLAN 5.5).
const CODE_IF_JOIN_MISMATCH: &str = "E396";
/// Diagnostic code reserved for a binding whose value does not coerce to
/// the declared type (PLAN 6.2).
const CODE_BIND_TYPE_MISMATCH: &str = "E310";
/// Diagnostic code reserved for an operator with mismatched operand
/// types (PLAN 6.3).
const CODE_OPERATOR_TYPE_MISMATCH: &str = "E311";
/// Diagnostic code reserved for a call with too few operands or an
/// operand that does not coerce to its parameter type (PLAN 6.4).
const CODE_CALL_TYPE_MISMATCH: &str = "E312";
/// Diagnostic code reserved for a non-bool `if` / `for` condition or
/// `match` case condition (PLAN 6.5).
const CODE_CONDITION_NOT_BOOL: &str = "E313";
/// Diagnostic code reserved for an empty `()`/`[]`/`{}` literal outside
/// a typed context (PLAN 2.1.1).
const CODE_EMPTY_LITERAL_UNTYPED: &str = "E395";

/// Summary of one registered function. Rule groups consult this instead of
/// the full AST body (e.g. the unsafe boundary of PLAN 3.5 needs
/// `is_unsafe`).
#[derive(Debug, Clone)]
pub struct FuncSummary {
    /// Fully-qualified name (`demo::add`, `Point::distance`), matching the
    /// interpreter's `funcs` keys.
    pub fq: String,
    /// `name unsafe function`: the body may use unsafe operations and calls
    /// to it require an unsafe context.
    pub is_unsafe: bool,
    /// True when registered from an `implement` block (binds `self`).
    pub is_method: bool,
    /// Number of declared parameters.
    pub param_count: usize,
    /// Number of declared return types (`with ...`).
    pub return_count: usize,
    /// Declared parameter types, resolved through the unit's declarations
    /// (PLAN 6.4). A `reference<T>` parameter resolves to its pointee so
    /// method receivers type-check; module types stay opaque.
    pub params: Vec<Ty>,
    /// Declared return types, resolved (PLAN 6.1). A fallible union
    /// return is opaque: its envelope shape is a runtime concern.
    pub returns: Vec<Ty>,
    /// PLAN 5.2.1: the `with` return is the fallible `|payload error|`
    /// form (Stage 4 predicate), so the call result is an error envelope.
    pub fallible: bool,
}

/// Declarations registered from one program unit, mirroring what
/// `Interpreter::register_unit` keeps for the main unit: struct, enum,
/// union, and error type names plus function summaries.
#[derive(Debug, Default)]
pub struct Declarations {
    structs: HashMap<String, StructDecl>,
    enums: HashMap<String, EnumDecl>,
    unions: HashMap<String, UnionDecl>,
    error_types: HashMap<String, ErrorDecl>,
    functions: HashMap<String, FuncSummary>,
}

impl Declarations {
    pub fn is_struct(&self, name: &str) -> bool {
        self.structs.contains_key(name)
    }

    pub fn is_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    /// Whether `name` declares a string-carried enum (PLAN 3.2): its values
    /// are heap strings at runtime, unlike integer-carried enums.
    pub fn is_string_enum(&self, name: &str) -> bool {
        self.enums.get(name).is_some_and(|d| match &d.underlying {
            Some(ty) => {
                matches!(&ty.kind, TypeKind::Primitive(Primitive::String))
                    || matches!(&ty.kind, TypeKind::Named(n) if n == "string")
            }
            None => false,
        })
    }

    pub fn is_union(&self, name: &str) -> bool {
        self.unions.contains_key(name)
    }

    pub fn is_error_type(&self, name: &str) -> bool {
        self.error_types.contains_key(name)
    }

    /// Registered function by fully-qualified name.
    pub fn function(&self, fq: &str) -> Option<&FuncSummary> {
        self.functions.get(fq)
    }

    /// Whether `fq` names a registered `unsafe function`.
    pub fn is_unsafe_function(&self, fq: &str) -> bool {
        self.functions.get(fq).is_some_and(|f| f.is_unsafe)
    }
}

/// Identity of a value that can be borrowed (PLAN 3.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BorrowKey {
    /// A local variable owns the value; reading a non-copy variable borrows
    /// it.
    Var(String),
    /// A stack-owned temporary (string / list / map / struct literal or a
    /// moved-in parameter); the `Owned(id)` slot owns it.
    Temp(u32),
}

impl BorrowKey {
    /// Human-readable description for diagnostics.
    fn describe(&self) -> String {
        match self {
            BorrowKey::Var(name) => format!("'{name}'"),
            BorrowKey::Temp(_) => "the same value".to_string(),
        }
    }
}

/// Statically known type of one abstract slot (PLAN 6.1). The checker
/// reuses the interpreter's kind-code scheme: primitives map to their
/// runtime kind codes (`primitive_to_kind`), and named types resolve
/// through this unit's declarations. Named types are identified by name
/// rather than by the interpreter's numeric ids: values of named types
/// reach the stack only through bindings the checker itself typed, so
/// name equality is the same test the ids encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A primitive kind code (bool=0, i8=1, ..., rune=11, f16=12, ...,
    /// string=16, error=0x51).
    Prim(u64),
    /// A named type declared in this unit (struct, enum, union, error
    /// type).
    Named(String),
    /// An integer literal with its decoded value: it binds to any
    /// integer type whose range contains the value (PLAN 6.2 widening
    /// with the literal's range check).
    IntLit(i128),
    /// A type used as a value: `typeof` results and type-value words.
    TypeValue,
    /// Type not statically known (module values, opaque words).
    Opaque,
}

impl Ty {
    /// Integer family: the signed and unsigned integer kinds plus
    /// integer literals.
    fn is_int(&self) -> bool {
        match self {
            Ty::Prim(k) => is_int_kind(*k),
            Ty::IntLit(_) => true,
            _ => false,
        }
    }

    /// Float family: `f16`, `f32`, `f64`.
    fn is_float(&self) -> bool {
        matches!(self, Ty::Prim(k) if is_float_kind(*k))
    }

    fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }

    fn is_bool(&self) -> bool {
        matches!(self, Ty::Prim(0))
    }

    fn is_string(&self) -> bool {
        matches!(self, Ty::Prim(16))
    }
}

/// Whether a kind code is one of the integer primitives (i8..i64, u8..u64).
fn is_int_kind(k: u64) -> bool {
    matches!(k, 1..=4 | 6..=9)
}

/// Whether a kind code is one of the float primitives (f16, f32, f64).
fn is_float_kind(k: u64) -> bool {
    matches!(k, 12..=14)
}

/// Human-readable name of a type, for diagnostics.
fn ty_label(ty: &Ty) -> String {
    match ty {
        Ty::Prim(k) => kind_label(*k),
        Ty::Named(n) => n.clone(),
        Ty::IntLit(n) => format!("integer literal {n}"),
        Ty::TypeValue => "a type value".to_string(),
        Ty::Opaque => "an opaque value".to_string(),
    }
}

/// Name of a primitive kind code, mirroring the interpreter's tables.
fn kind_label(k: u64) -> String {
    match k {
        0 => "bool",
        1 => "i8",
        2 => "i16",
        3 => "i32",
        4 => "i64",
        6 => "u8",
        7 => "u16",
        8 => "u32",
        9 => "u64",
        11 => "rune",
        12 => "f16",
        13 => "f32",
        14 => "f64",
        16 => "string",
        0x51 => "error",
        _ => "value",
    }
    .to_string()
}

/// Human-readable name of a declared type, for diagnostics.
fn type_label(ty: &Type) -> String {
    match &ty.kind {
        TypeKind::Primitive(p) => format!("{p:?}").to_lowercase(),
        TypeKind::Named(n) => n.clone(),
        TypeKind::Array { element, size } => match size {
            Some(n) => format!("array<{} {n}>", type_label(element)),
            None => format!("array<{}>", type_label(element)),
        },
        TypeKind::List { element } => format!("list<{}>", type_label(element)),
        TypeKind::Hashmap { key, value } => {
            format!("hashmap<{} {}>", type_label(key), type_label(value))
        }
        TypeKind::Reference { inner } => format!("reference<{}>", type_label(inner)),
        TypeKind::Pointer { inner } => format!("pointer<{}>", type_label(inner)),
        TypeKind::Union(_) => "union".to_string(),
    }
}

/// PLAN 6.2 coercion matrix: a value binds to a declared type when the
/// kinds are equal; an integer value binds to any integer type (widening,
/// with the literal's value range-checked when statically known); a
/// float value binds to any float type; every other combination (bool to
/// integer, float to integer, string to anything else, struct to another
/// struct, and so on) is an error. An opaque value or declared type
/// skips the check.
fn binds(value: &Ty, declared: &Ty) -> bool {
    match (value, declared) {
        (Ty::Opaque, _) | (_, Ty::Opaque) => true,
        (Ty::IntLit(n), Ty::Prim(k)) => {
            is_int_kind(*k) && int_carrier_range(*k).is_some_and(|(lo, hi)| *n >= lo && *n <= hi)
        }
        (Ty::Prim(v), Ty::Prim(d)) => {
            v == d
                || (is_int_kind(*v) && is_int_kind(*d))
                || (is_float_kind(*v) && is_float_kind(*d))
        }
        (Ty::Named(v), Ty::Named(d)) => v == d,
        _ => false,
    }
}

/// One abstract slot of the operand stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// Owned heap temporary; the stack frees it at pop / drop / consume.
    Owned(u32, Ty),
    /// Copy value: integers, floats, bool, rune, enum, array, type.
    Copy(Ty),
    /// `pointer<T>` value: a copy value whose arithmetic is pointer
    /// arithmetic (PLAN 3.5.2).
    Pointer(Ty),
    /// Live borrow of a value (from the `borrow` word or a non-copy read).
    Borrow(BorrowKey, Ty),
    /// Untracked value (call results, foreign values, reference reads):
    /// ownership and borrows are not modeled, but the static type may
    /// still be known (PLAN 6.1).
    Unknown(Ty),
}

impl Slot {
    /// The slot's static type.
    fn ty(&self) -> &Ty {
        match self {
            Slot::Owned(_, ty) | Slot::Copy(ty) | Slot::Pointer(ty) => ty,
            Slot::Borrow(_, ty) => ty,
            Slot::Unknown(ty) => ty,
        }
    }

    /// An untracked value of unknown type.
    fn opaque() -> Slot {
        Slot::Unknown(Ty::Opaque)
    }
}

/// What the checker knows about one variable binding.
#[derive(Debug, Clone)]
struct VarInfo {
    /// Copy types push a copy on read; non-copy types push a borrow.
    is_copy: bool,
    /// `pointer<T>` binding: reading it pushes a pointer slot, so `+` /
    /// `-` over it is pointer arithmetic (PLAN 3.5.2).
    is_pointer: bool,
    /// `reference<T>` binding: may hold a live borrow of another value.
    is_reference: bool,
    /// Named struct, enum, or union type, for method call resolution
    /// (PLAN 3.1.1: `implement` blocks may target any of the three).
    struct_name: Option<String>,
    /// The declared type (PLAN 6.1): reads push it, and `set` checks
    /// against it (PLAN 6.2).
    ty: Ty,
    /// Live borrow held through this `reference<T>` binding.
    holds: Option<BorrowKey>,
    /// `move` transferred the value away: further use is E373 (PLAN 3.2.1).
    /// A `set` or a re-declaration re-owns the name and clears the flag.
    moved: bool,
    /// Mutability of the declaration (PLAN 5.1.1): `set` is restricted to
    /// `mutable` bindings.
    mutability: Mutability,
}

/// One lexical scope: its variables die (and release held borrows) at exit.
struct Scope {
    vars: HashMap<String, VarInfo>,
}

/// Walks one parsed program and collects diagnostics (PLAN Stage 3).
pub struct Checker {
    /// Source path attached to emitted diagnostics.
    path: String,
    batch: DiagnosticBatch,
    decls: Declarations,
    /// Number of enclosing unsafe contexts (`unsafe ... end` blocks plus
    /// `unsafe function` bodies). The PLAN 3.5 rules consult this.
    unsafe_depth: usize,
    /// Span of the innermost expression or statement being checked; the
    /// fallback for diagnostics whose construct carries no span of its own.
    current_span: Span,
    /// Abstract operand stack (PLAN 3.1).
    stack: Vec<Slot>,
    /// Lexical scope stack; `scopes.len() - 1` is the innermost scope.
    scopes: Vec<Scope>,
    /// Live borrows: key -> index of the scope that created the borrow.
    live: HashMap<BorrowKey, usize>,
    /// Next free temporary id for `Slot::Owned`.
    next_temp: u32,
    /// `require` aliases: alias -> module path.
    aliases: HashMap<String, String>,
    /// Fully-qualified name of the function being checked ("" at top level).
    caller_fq: String,
    /// PLAN 5.2.1: whether the function being checked declares a fallible
    /// return (Stage 4 predicate). Top level is never fallible.
    caller_fallible: bool,
    /// PLAN 5.5: set while walking a region whose net stack effect the
    /// checker cannot know (an opaque call or builtin pushes one slot
    /// without popping its operands). Branch heights computed inside such
    /// a region are not comparable at an if/else join.
    height_opaque: bool,
}

impl Checker {
    pub fn new(path: impl Into<String>, error_limit: usize) -> Self {
        Self {
            path: path.into(),
            batch: DiagnosticBatch::with_limit(error_limit),
            decls: Declarations::default(),
            unsafe_depth: 0,
            current_span: Span::default(),
            stack: Vec::new(),
            scopes: vec![Scope {
                vars: HashMap::new(),
            }],
            live: HashMap::new(),
            next_temp: 0,
            aliases: HashMap::new(),
            caller_fq: String::new(),
            caller_fallible: false,
            height_opaque: false,
        }
    }

    /// Register every declaration, then walk all bodies.
    pub fn check_program(&mut self, program: &Program) {
        self.register_decls(program);
        // Top-level requires are visible everywhere, including in function
        // bodies declared above them in the file.
        for item in &program.items {
            if let StmtKind::Require { path, alias } = &item.kind
                && let Some(alias) = alias
            {
                self.aliases.insert(alias.clone(), path.clone());
            }
        }
        for item in &program.items {
            self.check_stmt(item);
        }
    }

    /// Collected diagnostics; empty when the program passed the check.
    pub fn into_batch(self) -> DiagnosticBatch {
        self.batch
    }

    /// Declarations registered from the program.
    pub fn declarations(&self) -> &Declarations {
        &self.decls
    }

    /// Whether the walk is currently inside an unsafe context.
    pub fn is_unsafe_context(&self) -> bool {
        self.unsafe_depth > 0
    }

    /// Span of the innermost expression or statement being checked.
    pub fn current_span(&self) -> Span {
        self.current_span
    }

    /// Record one diagnostic, attaching the source path. Returns `false`
    /// when an error is discarded because the error limit was reached.
    pub fn emit(&mut self, diag: Diagnostic) -> bool {
        self.batch.push(diag.with_path(self.path.clone()))
    }

    // -----------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------

    /// First pass: collect declarations without checking bodies, mirroring
    /// `Interpreter::register_unit` for the main unit. Types register before
    /// functions so a function's fallibility (PLAN 5.2.1) resolves bare
    /// error-type names no matter where the declaration sits in the file.
    fn register_decls(&mut self, program: &Program) {
        for item in &program.items {
            match &item.kind {
                StmtKind::Struct(d) => {
                    self.decls.structs.insert(d.name.clone(), d.clone());
                }
                StmtKind::Enum(d) => {
                    self.decls.enums.insert(d.name.clone(), d.clone());
                }
                StmtKind::Union(d) => {
                    self.decls.unions.insert(d.name.clone(), d.clone());
                }
                StmtKind::Error(d) => {
                    self.decls.error_types.insert(d.name.clone(), d.clone());
                }
                _ => {}
            }
        }
        for item in &program.items {
            match &item.kind {
                StmtKind::Implement(imp) => {
                    for f in &imp.functions {
                        let fq = format!("{}::{}", imp.target, f.name);
                        self.register_function(&fq, f, true);
                    }
                }
                StmtKind::Function(f) => self.register_function(&f.name, f, false),
                _ => {}
            }
        }
    }

    /// Register one function and its nested functions under their
    /// fully-qualified names, matching `Interpreter::register_function_tree`.
    fn register_function(&mut self, fq: &str, f: &Function, is_method: bool) {
        let fallible = matches!(
            classify_signature(&f.returns, &|n| self.resolves_error_type(n)),
            Signature::Fallible { .. }
        );
        let params = f
            .params
            .iter()
            .map(|p| self.resolve_type(&p.ty))
            .collect::<Vec<_>>();
        let returns = f
            .returns
            .iter()
            .map(|t| self.resolve_type(t))
            .collect::<Vec<_>>();
        self.decls.functions.insert(
            fq.to_string(),
            FuncSummary {
                fq: fq.to_string(),
                is_unsafe: f.is_unsafe,
                is_method,
                param_count: f.params.len(),
                return_count: f.returns.len(),
                params,
                returns,
                fallible,
            },
        );
        for stmt in &f.body {
            if let StmtKind::Function(nested) = &stmt.kind {
                let nested_fq = format!("{fq}::{}", nested.name);
                self.register_function(&nested_fq, nested, false);
            }
        }
    }

    // -----------------------------------------------------------------
    // Borrow machine primitives (PLAN 3.1)
    // -----------------------------------------------------------------

    fn fresh_temp(&mut self) -> u32 {
        self.next_temp += 1;
        self.next_temp
    }

    fn push_slot(&mut self, slot: Slot) {
        self.stack.push(slot);
    }

    /// Pop one slot without ending the borrow it holds; callers that do not
    /// re-bind the borrow must release it themselves.
    fn pop_slot_raw(&mut self) -> Option<Slot> {
        self.stack.pop()
    }

    /// Pop one slot, releasing the borrow it holds (PLAN 3.1.1: a borrow
    /// ends at `pop`).
    fn pop_releasing(&mut self) -> Option<Slot> {
        let slot = self.stack.pop()?;
        if let Slot::Borrow(key, _) = &slot {
            self.live.remove(key);
        }
        Some(slot)
    }

    /// Pop `n` slots, releasing their borrows.
    fn pop_n(&mut self, n: usize) {
        for _ in 0..n {
            self.pop_releasing();
        }
    }

    /// Whether `key` currently has a live borrow.
    fn is_live(&self, key: &BorrowKey) -> bool {
        self.live.contains_key(key)
    }

    /// Create a borrow of `key`, rejecting a second live borrow of the same
    /// value (PLAN 3.1.1, E375). The borrow slot carries the borrowed
    /// value's static type (PLAN 6.1).
    fn create_borrow(&mut self, key: BorrowKey, ty: Ty, span: Span) -> Slot {
        if self.live.contains_key(&key) {
            self.emit(
                Diagnostic::error(
                    CODE_SECOND_BORROW,
                    format!(
                        "second live borrow of {}: release the first borrow before borrowing again",
                        key.describe()
                    ),
                )
                .with_primary(span, ""),
            );
            return Slot::Borrow(key, ty);
        }
        self.live.insert(key.clone(), self.scopes.len() - 1);
        Slot::Borrow(key, ty)
    }

    /// A call whose signature is not visible (module function, builtin):
    /// assume every live on-stack borrow was consumed (PLAN 3.1.1
    /// "consumption by a call") and push one opaque result.
    fn unknown_call(&mut self) {
        // PLAN 5.5: the call's arity is invisible here (one slot pushed,
        // operands not popped), so any enclosing branch height that
        // includes this effect is not comparable at a join.
        self.height_opaque = true;
        self.live.clear();
        for slot in &mut self.stack {
            if let Slot::Borrow(_, ty) = slot {
                *slot = Slot::Unknown(ty.clone());
            }
        }
        self.push_slot(Slot::opaque());
    }

    /// Whether any `reference<T>` variable in scope holds a live borrow of
    /// `key`. Such a borrow survives `drop` (it is not on the cleared
    /// stack), so freeing the owner dangles it.
    fn held_by_reference(&self, key: &BorrowKey) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.vars.values().any(|v| v.holds.as_ref() == Some(key)))
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
        });
    }

    /// End every trace of the borrow of `key`: the live entry, any
    /// `reference<T>` binding holding it, and on-stack borrow slots.
    fn end_borrow(&mut self, key: &BorrowKey) {
        self.live.remove(key);
        for scope in &mut self.scopes {
            for var in scope.vars.values_mut() {
                if var.holds.as_ref() == Some(key) {
                    var.holds = None;
                }
            }
        }
        for slot in &mut self.stack {
            if let Slot::Borrow(held, ty) = slot
                && held == key
            {
                *slot = Slot::Unknown(ty.clone());
            }
        }
    }

    /// Leave the innermost scope: its variables die, and every borrow
    /// created inside it ends (PLAN 3.1.1 "a borrow ends at scope exit").
    fn pop_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for var in scope.vars.values() {
            if let Some(key) = &var.holds {
                self.live.remove(key);
            }
        }
        let depth = self.scopes.len();
        self.live.retain(|_, created| *created < depth);
        // Slots whose borrow just ended become untracked; their static
        // type survives (PLAN 6.1).
        for slot in &mut self.stack {
            if let Slot::Borrow(key, ty) = slot
                && !self.live.contains_key(key)
            {
                *slot = Slot::Unknown(ty.clone());
            }
        }
    }

    /// Pop the slots above index `at` (an empty tail when the stack shrank
    /// below it, e.g. a branch that consumed ambient values).
    fn split_tail(&mut self, at: usize) -> Vec<Slot> {
        if at < self.stack.len() {
            self.stack.split_off(at)
        } else {
            Vec::new()
        }
    }

    /// Innermost binding for `name`, if any.
    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|s| s.vars.get(name))
    }

    /// Whether `name` names a registered type.
    fn is_decl_name(&self, name: &str) -> bool {
        self.decls.is_struct(name)
            || self.decls.is_enum(name)
            || self.decls.is_union(name)
            || self.decls.is_error_type(name)
    }

    /// Copy types push a copy on read; non-copy types push a borrow. Named
    /// types that are not declared in this unit default to non-copy (module
    /// types are heap values at runtime).
    /// Stage 2: whether a declared union type has a container member
    /// (`list<T>`, `hashmap<K V>`, `array<T N>`, references stripped).
    /// Such a union gives an empty `()`/`[]`/`{}` its typed context.
    fn union_container_context(&self, ty: &Type) -> bool {
        let TypeKind::Named(n) = &ty.kind else {
            return false;
        };
        let Some(union) = self.decls.unions.get(n) else {
            return false;
        };
        fn container_shape(t: &Type) -> bool {
            match &t.kind {
                TypeKind::List { .. } | TypeKind::Array { .. } | TypeKind::Hashmap { .. } => true,
                TypeKind::Reference { inner } => container_shape(inner),
                _ => false,
            }
        }
        union.types.iter().any(container_shape)
    }

    fn is_copy_type(&self, ty: &Type) -> bool {
        match &ty.kind {
            TypeKind::Primitive(p) => !matches!(p, Primitive::String),
            TypeKind::Array { .. } | TypeKind::Pointer { .. } => true,
            TypeKind::List { .. }
            | TypeKind::Hashmap { .. }
            | TypeKind::Reference { .. }
            | TypeKind::Union(_) => false,
            TypeKind::Named(name) => self.decls.is_enum(name),
        }
    }

    /// PLAN 6.2 binding check with the union rule: a named union accepts
    /// any of its member types at init and `set` (the runtime wraps the
    /// value), and a value of the union itself binds as-is. Everything
    /// else goes through the coercion matrix.
    fn binds_to(&self, value: &Ty, declared: &Ty) -> bool {
        if let Ty::Named(name) = declared
            && let Some(union) = self.decls.unions.get(name)
        {
            return value == declared
                || union
                    .types
                    .iter()
                    .any(|member| binds(value, &self.resolve_type(member)));
        }
        binds(value, declared)
    }

    /// PLAN 6.1: resolve a declared type to the checker's type
    /// representation. Primitives map to their runtime kind codes; named
    /// types resolve through this unit's declarations (a name the unit
    /// does not declare is a module type and stays opaque). Container,
    /// pointer, and union types are opaque: their values arrive as opaque
    /// slots (literals, module calls), so the binding check skips them.
    /// A reference resolves to its pointee so method receivers type-check
    /// against `reference<T>` parameters.
    ///
    /// PLAN 4.2.2: a module-qualified name (`m.Point`) resolves the same
    /// way a qualified call does: when the first segment names a visible
    /// `require` alias the type is a module type and stays opaque (module
    /// contents are not visible here; the interpreter validates the name
    /// and the type's visibility after loading). Module units stay opaque
    /// as today, so type privacy (PLAN 4.1.2, `E398`) is enforced by the
    /// interpreter for every cross-unit use.
    fn resolve_type(&self, ty: &Type) -> Ty {
        match &ty.kind {
            TypeKind::Primitive(p) => primitive_to_kind(*p).map(Ty::Prim).unwrap_or(Ty::Opaque),
            TypeKind::Named(n) => {
                if let Some(k) = primitive_kind_code(n) {
                    return Ty::Prim(k);
                }
                if self.is_decl_name(n) {
                    return Ty::Named(n.clone());
                }
                Ty::Opaque
            }
            TypeKind::Reference { inner } => self.resolve_type(inner),
            _ => Ty::Opaque,
        }
    }

    /// Bind `name` to the value described by `slot`, with the shape of `ty`
    /// and the declared mutability.
    fn bind_var(&mut self, name: &str, ty: &Type, mutability: Mutability, slot: Slot) {
        let is_copy = self.is_copy_type(ty);
        let is_pointer = matches!(ty.kind, TypeKind::Pointer { .. });
        let is_reference = matches!(ty.kind, TypeKind::Reference { .. });
        let struct_name = match &ty.kind {
            TypeKind::Named(n)
                if self.decls.is_struct(n) || self.decls.is_enum(n) || self.decls.is_union(n) =>
            {
                Some(n.clone())
            }
            _ => None,
        };
        let resolved = self.resolve_type(ty);
        self.bind_var_info(
            name,
            VarInfo {
                is_copy,
                is_pointer,
                is_reference,
                struct_name,
                ty: resolved,
                holds: None,
                moved: false,
                mutability,
            },
            slot,
        );
    }

    /// Bind `name` with a precomputed shape; `slot` decides the held borrow.
    fn bind_var_info(&mut self, name: &str, mut info: VarInfo, slot: Slot) {
        // A same-scope rebinding drops the old value first.
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if let Some(old) = scope.vars.get(name)
            && let Some(key) = &old.holds
        {
            self.live.remove(key);
        }
        match (&slot, info.is_reference) {
            // A borrow bound into a `reference<T>` variable stays live,
            // held by that variable.
            (Slot::Borrow(key, _), true) => info.holds = Some(key.clone()),
            // Any other binding consumes the borrow.
            (Slot::Borrow(key, _), false) => {
                self.live.remove(key);
            }
            _ => {}
        }
        self.scopes
            .last_mut()
            .expect("scope stack is never empty")
            .vars
            .insert(name.to_string(), info);
    }

    // -----------------------------------------------------------------
    // Function bodies
    // -----------------------------------------------------------------

    /// Check one function body in isolation: parameters move onto the local
    /// stack (first declared = deepest), an `unsafe function` body is an
    /// unsafe context, and the borrow machine starts fresh.
    fn check_function(&mut self, f: &Function, fq: &str, is_method: bool) {
        let saved = self.save_machine();
        // PLAN 5.5: height comparability is per function body; opaque
        // calls in an earlier function do not blur this one's branches.
        self.height_opaque = false;
        self.caller_fq = fq.to_string();
        // PLAN 5.2.1: fallibility of the function being checked, via the
        // Stage 4 predicate (the same call also emits E386, PLAN 4.2).
        self.caller_fallible = self.check_return_signature(f);

        for param in &f.params {
            let ty = self.resolve_type(&param.ty);
            let slot = match &param.ty.kind {
                TypeKind::Reference { .. } => Slot::Unknown(ty),
                TypeKind::Pointer { .. } => Slot::Pointer(ty),
                _ if self.is_copy_type(&param.ty) => Slot::Copy(ty),
                _ => Slot::Owned(self.fresh_temp(), ty),
            };
            self.push_slot(slot);
        }
        if is_method {
            // The receiver is pre-bound to `self` as a borrow of the
            // caller's value (PLAN 2.3.3: `self` always borrows).
            let struct_name = match f.params.first().map(|p| &p.ty.kind) {
                Some(TypeKind::Reference { inner }) => match &inner.kind {
                    TypeKind::Named(n) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            };
            self.scopes[0].vars.insert(
                "self".to_string(),
                VarInfo {
                    is_copy: false,
                    is_pointer: false,
                    is_reference: true,
                    struct_name: struct_name.clone(),
                    ty: struct_name.map(Ty::Named).unwrap_or(Ty::Opaque),
                    holds: None,
                    moved: false,
                    mutability: Mutability::Mutable,
                },
            );
        }

        // Audit-3 Stage 5: the `unsafe` marker on a function is declarative
        // only; its body is not an unsafe context. Unsafe operations need an
        // active `unsafe ... end` block, also inside unsafe functions.
        self.check_body(&f.body);
        self.restore_machine(saved);
    }

    /// PLAN 4.2 / 4.4: a two-member union return is only supported as the
    /// fallible form, so its error side must resolve. The checker sees
    /// main-unit declarations only: a bare name resolves when this unit
    /// declares it as an error type, and a module-qualified path is
    /// resolved by the interpreter after module loading.
    ///
    /// PLAN 5.2.1: returns whether the signature is the fallible form, so
    /// the caller's `unwrap` rule can consult it.
    fn check_return_signature(&mut self, f: &Function) -> bool {
        let signature = classify_signature(&f.returns, &|n| self.resolves_error_type(n));
        match signature {
            Signature::Fallible { .. } => true,
            Signature::NonFallible => false,
            Signature::UnknownError { name } => {
                let span = f
                    .returns
                    .first()
                    .map(|t| Span::from_location(t.location))
                    .unwrap_or_default();
                self.emit(
                    Diagnostic::error(
                        CODE_UNKNOWN_ERROR_TYPE,
                        format!(
                            "unknown error type '{name}' in the signature of '{}'",
                            f.name
                        ),
                    )
                    .with_primary(span, "")
                    .with_help(
                        "require the module that declares the error type, or declare the error type in this unit",
                    ),
                );
                false
            }
        }
    }

    /// PLAN 4.1: error-side resolution visible to the checker. A bare name
    /// resolves when this unit declares it as an error type; a dotted path
    /// names a module type, and module contents are not visible here, so
    /// it resolves opaquely (the interpreter validates it after loading).
    fn resolves_error_type(&self, name: &str) -> bool {
        match name.rsplit_once('.') {
            Some(_) => true,
            None => self.decls.is_error_type(name),
        }
    }

    /// PLAN 5.3.1: union members must be distinct after reference
    /// stripping, the same normalization the interpreter's `member_ty`
    /// applies at registration. The interpreter backstops module units,
    /// which the checker cannot see.
    fn check_union_members(&mut self, d: &UnionDecl, span: Span) {
        let mut seen: Vec<String> = Vec::new();
        for t in &d.types {
            let Some(key) = union_member_key(t) else {
                continue;
            };
            if seen.contains(&key) {
                self.emit(
                    Diagnostic::error(
                        CODE_UNION_DUPLICATE_MEMBER,
                        format!(
                            "union '{}' declares the member type '{}' more than once",
                            d.name,
                            union_member_label(t)
                        ),
                    )
                    .with_primary(span, "")
                    .with_help("remove the duplicate member, or wrap it in a distinct named type"),
                );
            } else {
                seen.push(key);
            }
        }
    }

    /// Snapshot everything the borrow machine mutates across a function.
    fn save_machine(&mut self) -> MachineSnapshot {
        MachineSnapshot {
            stack: std::mem::take(&mut self.stack),
            scopes: std::mem::replace(
                &mut self.scopes,
                vec![Scope {
                    vars: HashMap::new(),
                }],
            ),
            live: std::mem::take(&mut self.live),
            aliases: std::mem::take(&mut self.aliases),
            caller_fq: std::mem::take(&mut self.caller_fq),
            caller_fallible: self.caller_fallible,
        }
    }

    fn restore_machine(&mut self, saved: MachineSnapshot) {
        self.stack = saved.stack;
        self.scopes = saved.scopes;
        self.live = saved.live;
        self.aliases = saved.aliases;
        self.caller_fq = saved.caller_fq;
        self.caller_fallible = saved.caller_fallible;
    }

    fn check_body(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.check_stmt(stmt);
        }
    }

    // -----------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------

    /// PLAN 6.5: `if` conditions and `match` case conditions must be bool.
    /// An opaque condition (module value, untyped slot) skips the check.
    fn check_condition(&mut self, slot: Option<Slot>, what: &str, span: Span) {
        if let Some(slot) = slot
            && !matches!(slot.ty(), Ty::Opaque)
            && !slot.ty().is_bool()
        {
            self.emit(
                Diagnostic::error(
                    CODE_CONDITION_NOT_BOOL,
                    format!("{what} must be bool, not {}", ty_label(slot.ty())),
                )
                .with_primary(span, "")
                .with_help("comparisons push bool; compare or convert the value first"),
            );
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        self.current_span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(expr) => self.check_expr(expr, stmt.span),
            StmtKind::VarDecl {
                name,
                mutability,
                ty,
                value,
            } => {
                // PLAN 3.4.1: a `static` binding is a compile-time constant.
                if *mutability == Mutability::Static {
                    self.check_static_init(name, value.as_ref(), stmt.span);
                }
                // A missing initializer binds the current stack top. The
                // popped borrow stays live when the binding holds it
                // (`bind_var_info` decides).
                // PLAN 2.1.1: an empty `()`/`[]` is legal only as the
                // value of a binding initializer whose declared type is a
                // container (`list<T>`, `array<T N>`, `hashmap<K V>`).
                // Stage 2: a union with a container member is such a
                // context too (the member types the literal).
                let container_ctx = matches!(
                    ty.kind,
                    TypeKind::List { .. } | TypeKind::Array { .. } | TypeKind::Hashmap { .. }
                ) || self.union_container_context(ty);
                let slot = match value {
                    Some(value) => {
                        self.check_binding_init(value, stmt.span, container_ctx);
                        self.pop_slot_raw()
                    }
                    None => self.pop_slot_raw(),
                }
                .unwrap_or_else(Slot::opaque);
                // PLAN 6.2: binding compatibility. The popped value must
                // coerce to the declared type (E310).
                let declared = self.resolve_type(ty);
                if !self.binds_to(slot.ty(), &declared) {
                    self.emit(
                        Diagnostic::error(
                            CODE_BIND_TYPE_MISMATCH,
                            format!(
                                "cannot bind {} to '{name}': the declaration's type is {}",
                                ty_label(slot.ty()),
                                type_label(ty)
                            ),
                        )
                        .with_primary(stmt.span, "")
                        .with_help(
                            "implicit coercion covers the integer types (and the float types); \\\
                             convert the value or change the declared type",
                        ),
                    );
                }
                self.bind_var(name, ty, *mutability, slot);
            }
            StmtKind::Set { target, value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
                let slot = self.pop_slot_raw().unwrap_or_else(Slot::opaque);
                if let Expr::Variable { name } = target
                    && let Some(var) = self.lookup_var(name).cloned()
                {
                    // PLAN 5.1.1: `set` is restricted to `mutable`
                    // bindings; `const` is set once at runtime (its
                    // initializer) and `static` is a compile-time constant.
                    if matches!(var.mutability, Mutability::Const | Mutability::Static) {
                        let kind = match var.mutability {
                            Mutability::Const => "const",
                            Mutability::Static => "static",
                            Mutability::Mutable => unreachable!(),
                        };
                        self.emit(
                            Diagnostic::error(
                                CODE_SET_IMMUTABLE,
                                format!(
                                    "cannot `set` '{name}': the binding is {kind}, not mutable"
                                ),
                            )
                            .with_primary(stmt.span, "")
                            .with_help(
                                "re-declare the name with `mutable`, or use a `mutable` binding",
                            ),
                        );
                    }
                    // PLAN 6.2: `set` checks the value against the
                    // variable's recorded type (E310).
                    if !self.binds_to(slot.ty(), &var.ty) {
                        self.emit(
                            Diagnostic::error(
                                CODE_BIND_TYPE_MISMATCH,
                                format!(
                                    "cannot set '{name}' ({}): the value is {}",
                                    ty_label(&var.ty),
                                    ty_label(slot.ty())
                                ),
                            )
                            .with_primary(stmt.span, "")
                            .with_help(
                                "implicit coercion covers the integer types (and the float types); \\\
                                 convert the value or re-declare with the right type",
                            ),
                        );
                    }
                    let mut info = var;
                    // The old value is dropped: a borrow it held ends.
                    if let Some(key) = info.holds.take() {
                        self.live.remove(&key);
                    }
                    // `set` re-owns the name, even after a `move`.
                    info.moved = false;
                    match (&slot, info.is_reference) {
                        (Slot::Borrow(key, _), true) => info.holds = Some(key.clone()),
                        (Slot::Borrow(key, _), false) => {
                            self.live.remove(key);
                        }
                        _ => {}
                    }
                    self.scopes
                        .last_mut()
                        .expect("scope stack is never empty")
                        .vars
                        .insert(name.clone(), info);
                } else if let Slot::Borrow(key, _) = &slot {
                    // Field target: the value's borrow is consumed.
                    self.live.remove(key);
                }
                // A field target reads its base transiently (push + pop at
                // runtime), which creates no lasting borrow.
            }
            StmtKind::Function(f) => {
                let fq = if self.caller_fq.is_empty() {
                    f.name.clone()
                } else {
                    format!("{}::{}", self.caller_fq, f.name)
                };
                self.check_function(f, &fq, false);
            }
            StmtKind::Implement(imp) => {
                for f in &imp.functions {
                    let fq = format!("{}::{}", imp.target, f.name);
                    self.check_function(f, &fq, true);
                }
            }
            StmtKind::Struct(_) | StmtKind::Enum(_) | StmtKind::Error(_) => {}
            StmtKind::Union(d) => self.check_union_members(d, stmt.span),
            StmtKind::Require { path, alias } => {
                if let Some(alias) = alias {
                    self.aliases.insert(alias.clone(), path.clone());
                }
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.check_expr(condition, stmt.span);
                // PLAN 6.5: the condition must be bool (E313).
                let condition = self.pop_releasing();
                self.check_condition(condition, "if condition", stmt.span);
                let base = self.stack.len();
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                let saved_opaque = self.height_opaque;

                self.height_opaque = false;
                self.push_scope();
                self.check_body(then_branch);
                self.pop_scope();
                let then_tail = self.split_tail(base);
                let then_opaque = self.height_opaque;
                self.stack = saved.clone();

                let (else_tail, else_opaque) = if else_branch.is_empty() {
                    (Vec::new(), false)
                } else {
                    self.height_opaque = false;
                    self.live = saved_live.clone();
                    self.push_scope();
                    self.check_body(else_branch);
                    self.pop_scope();
                    let tail = self.split_tail(base);
                    let opaque = self.height_opaque;
                    self.stack = saved.clone();
                    (tail, opaque)
                };
                self.live = saved_live;
                // PLAN 5.5: then/else must leave compatible stacks at the
                // join. Slot types are opaque in the checker, so height is
                // the compatibility criterion. An `if` without `else` has an
                // implicit else path that pushes nothing, so its then-branch
                // must leave the pre-`if` height. A branch whose height
                // depends on an opaque call or builtin (unknown arity) is
                // not comparable, like the borrow machine's opacity
                // simplification; the runtime stack-balance backstop
                // covers it.
                let comparable = !then_opaque && !else_opaque;
                if comparable && else_branch.is_empty() {
                    if !then_tail.is_empty() {
                        self.emit(
                            Diagnostic::error(
                                CODE_IF_JOIN_MISMATCH,
                                format!(
                                    "`if` without `else` leaves {} extra value(s) on the stack; the implicit else path pushes none",
                                    then_tail.len()
                                ),
                            )
                            .with_primary(stmt.span, "")
                            .with_help(
                                "drop the value(s) before the end of the then-branch, or add an `else` branch that leaves the same height",
                            ),
                        );
                    }
                } else if comparable && then_tail.len() != else_tail.len() {
                    self.emit(
                        Diagnostic::error(
                            CODE_IF_JOIN_MISMATCH,
                            format!(
                                "then and else branches leave different stack heights ({} vs {} extra value(s))",
                                then_tail.len(),
                                else_tail.len()
                            ),
                        )
                        .with_primary(stmt.span, "")
                        .with_help(
                            "both branches must leave the same number of values on the stack at the join",
                        ),
                    );
                }
                // Exactly one branch runs; keep the longer tail (opaque).
                let tail = if else_tail.len() > then_tail.len() {
                    else_tail
                } else {
                    then_tail
                };
                self.stack = saved;
                for slot in tail {
                    self.push_slot(slot);
                }
                // The joined tail carries the branches' opacity: an
                // enclosing branch height that includes it stays
                // uncomparable.
                self.height_opaque = saved_opaque || then_opaque || else_opaque;
            }
            StmtKind::For { source, body } => {
                self.check_expr(source, stmt.span);
                // The loop consumes its condition / iterable. PLAN 6.5:
                // the source is a bool condition or an array / list /
                // hashmap iterable (PLAN 6.1.2); a value of any other
                // known type is E313. Iterable bindings (list, array,
                // hashmap) are opaque to the checker, so every known
                // non-bool source is rejected.
                let source = self.pop_releasing();
                if let Some(slot) = &source
                    && !matches!(slot.ty(), Ty::Opaque)
                    && !slot.ty().is_bool()
                {
                    self.emit(
                        Diagnostic::error(
                            CODE_CONDITION_NOT_BOOL,
                            format!(
                                "for source must be a bool condition or an array, list, or hashmap iterable, not {}",
                                ty_label(slot.ty())
                            ),
                        )
                        .with_primary(stmt.span, "")
                        .with_help(
                            "comparisons push bool; iterate a list, array, or hashmap, or compare in the condition",
                        ),
                    );
                }
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                self.push_scope();
                self.check_body(body);
                self.pop_scope();
                self.live = saved_live;
                // Values pushed by the body are dropped after each iteration.
                self.stack = saved;
            }
            StmtKind::Match {
                value,
                cases,
                else_branch,
            } => self.check_match(value, cases, else_branch, stmt.span),
            StmtKind::Defer { body } => {
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                self.push_scope();
                self.check_body(body);
                self.pop_scope();
                self.live = saved_live;
                self.stack = saved;
            }
            StmtKind::Handle { body, fallback } => {
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                self.push_scope();
                self.check_body(body);
                self.pop_scope();
                self.live = saved_live;
                self.stack = saved;
                if let Some(fallback) = fallback {
                    self.check_expr(fallback, stmt.span);
                    self.pop_n(1);
                }
            }
            StmtKind::Move { target, source } => self.check_move(target, source, stmt.span),
            StmtKind::Fallback { value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                    self.pop_n(1);
                }
            }
            StmtKind::Return { value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
                // The frame and its stack are gone; every borrow ends.
                self.stack.clear();
                self.live.clear();
            }
            StmtKind::Unsafe { body } => {
                self.unsafe_depth += 1;
                self.push_scope();
                self.check_body(body);
                self.pop_scope();
                self.unsafe_depth -= 1;
            }
        }
    }

    /// PLAN 3.4.1: a `static` binding is a compile-time constant, so its
    /// initializer must be statically known: literals and constant
    /// expressions only, no variable reads, no calls. A declaration with
    /// no initializer would bind the runtime stack, which is not statically
    /// known either.
    fn check_static_init(&mut self, name: &str, value: Option<&Expr>, span: Span) {
        let Some(value) = value else {
            self.emit(
                Diagnostic::error(
                    CODE_STATIC_NON_CONST,
                    format!(
                        "'{name}' is static: it needs a compile-time constant initializer, \
                         not a value from the runtime stack"
                    ),
                )
                .with_primary(span, ""),
            );
            return;
        };
        if let Some(bad) = self.static_init_violation(value, span) {
            self.emit(
                Diagnostic::error(
                    CODE_STATIC_NON_CONST,
                    format!(
                        "'{name}' is static: its initializer must be a compile-time constant \
                         (literals and constant expressions only; no variable reads, no calls)"
                    ),
                )
                .with_primary(bad, ""),
            );
        }
    }

    /// Span of the word that makes a `static` initializer not a
    /// compile-time constant, or `None` when it is constant. The bound
    /// value is the top of the operand stack after the initializer's words
    /// run, so the walk simulates the stack: each slot remembers whether
    /// its value is constant and which word produced it. Words whose
    /// results are consumed before the binding (a call whose result a
    /// later word pops) do not taint the initializer. `fallback` covers
    /// single-word initializers, which the AST stores without their own
    /// span.
    fn static_init_violation(&self, expr: &Expr, fallback: Span) -> Option<Span> {
        let mut stack: Vec<(bool, Span)> = Vec::new();
        match expr {
            Expr::Seq(elems) => {
                for (e, span) in elems {
                    self.static_word_effect(e, *span, &mut stack);
                }
            }
            other => self.static_word_effect(other, fallback, &mut stack),
        }
        match stack.last() {
            Some((true, _)) => None,
            Some((false, span)) => Some(*span),
            None => Some(fallback),
        }
    }

    /// Effect of one initializer word on the simulated stack: push a slot
    /// for value words, apply the runtime stack effect of postfix words.
    /// Calls and builtins consume the whole simulated stack (their arity
    /// is not visible here) and push one non-constant slot, mirroring the
    /// conservative `unknown_call` modeling of the borrow machine.
    fn static_word_effect(&self, expr: &Expr, span: Span, stack: &mut Vec<(bool, Span)>) {
        match expr {
            Expr::Integer { .. }
            | Expr::Float { .. }
            | Expr::String { .. }
            | Expr::Rune { .. }
            | Expr::Bool { .. }
            | Expr::TypeValue { .. }
            | Expr::EmptyMapOrStruct => stack.push((true, span)),
            Expr::Array { .. }
            | Expr::List { .. }
            | Expr::Map { .. }
            | Expr::StructLit { .. }
            | Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Unwrap { .. } => {
                stack.push((self.static_nested_const(expr), span));
            }
            Expr::Typeof { inner } => {
                stack.push((self.static_nested_const(inner), span));
            }
            Expr::Variable { .. } | Expr::Borrow { .. } => stack.push((false, span)),
            Expr::Member { base, .. } => stack.push((self.is_const_member_base(base), span)),
            Expr::Call { .. } | Expr::Builtin { .. } => {
                stack.clear();
                stack.push((false, span));
            }
            Expr::Load { .. } => stack.push((false, span)),
            Expr::Store { .. } => {
                for _ in 0..2 {
                    stack.pop();
                }
            }
            Expr::ApplyBin(_) => {
                let (rc, rs) = stack.pop().unwrap_or((true, span));
                let (lc, ls) = stack.pop().unwrap_or((true, span));
                stack.push((lc && rc, if rc { ls } else { rs }));
            }
            Expr::ApplyUn(_) | Expr::ApplyTypeof => {
                let slot = stack.pop().unwrap_or((true, span));
                stack.push(slot);
            }
            Expr::ApplyBorrow | Expr::ApplyLoad => {
                stack.pop();
                stack.push((false, span));
            }
            Expr::StackOp(op) => self.static_stack_op_effect(*op, stack),
            Expr::Seq(elems) => {
                for (e, elem_span) in elems {
                    self.static_word_effect(e, *elem_span, stack);
                }
            }
        }
    }

    /// Stack shuffles, mirroring the runtime semantics of `StackOp`.
    fn static_stack_op_effect(&self, op: StackOp, stack: &mut Vec<(bool, Span)>) {
        match op {
            StackOp::Dup => {
                if let Some(top) = stack.last().cloned() {
                    stack.push(top);
                }
            }
            StackOp::Swap => {
                let n = stack.len();
                if n >= 2 {
                    stack.swap(n - 1, n - 2);
                }
            }
            StackOp::Rot => {
                let c = stack.pop();
                let b = stack.pop();
                let a = stack.pop();
                if let (Some(a), Some(b), Some(c)) = (a, b, c) {
                    stack.push(b);
                    stack.push(c);
                    stack.push(a);
                }
            }
            StackOp::Unrot => {
                let c = stack.pop();
                let b = stack.pop();
                let a = stack.pop();
                if let (Some(a), Some(b), Some(c)) = (a, b, c) {
                    stack.push(c);
                    stack.push(a);
                    stack.push(b);
                }
            }
            StackOp::Pop => {
                stack.pop();
            }
            StackOp::Drop => stack.clear(),
        }
    }

    /// Whether a nested expression inside an initializer word is a
    /// compile-time constant: literals, constant containers and
    /// expressions over them, and enum or error-type members
    /// (`Color.RED`), which are compile-time discriminants and tags.
    /// Variable reads, calls, borrows, loads, and stores are not.
    fn static_nested_const(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Integer { .. }
            | Expr::Float { .. }
            | Expr::String { .. }
            | Expr::Rune { .. }
            | Expr::Bool { .. }
            | Expr::TypeValue { .. }
            | Expr::EmptyMapOrStruct => true,
            Expr::Array(elems) | Expr::List(elems) => {
                elems.iter().all(|e| self.static_nested_const(e))
            }
            Expr::Map(pairs) => pairs
                .iter()
                .all(|(k, v)| self.static_nested_const(k) && self.static_nested_const(v)),
            Expr::StructLit(fields) => fields.iter().all(|(_, v)| self.static_nested_const(v)),
            Expr::Binary { left, right, .. } => {
                self.static_nested_const(left) && self.static_nested_const(right)
            }
            Expr::Unary { operand, .. } => self.static_nested_const(operand),
            Expr::Unwrap { inner } => self.static_nested_const(inner),
            Expr::Typeof { inner } => self.static_nested_const(inner),
            Expr::Member { base, .. } => self.is_const_member_base(base),
            _ => false,
        }
    }

    /// A member access is a compile-time constant only when its base names
    /// a registered enum or error type (a discriminant or tag).
    fn is_const_member_base(&self, base: &Expr) -> bool {
        matches!(base, Expr::Variable { name }
            if self.decls.is_enum(name) || self.decls.is_error_type(name))
    }

    /// `source target move`: ownership (and a held borrow) transfers to the
    /// target; the target's old value is dropped. PLAN 3.2: the source
    /// binding is emptied (E373 on further use) and must not carry a live
    /// borrow (E374).
    fn check_move(&mut self, target: &str, source: &Expr, span: Span) {
        if let Expr::Variable { name } = source
            && let Some(mut info) = self.lookup_var(name).cloned()
        {
            // PLAN 3.2.1: moving an already-moved source is a use after
            // move.
            if info.moved {
                self.emit(
                    Diagnostic::error(
                        CODE_USE_AFTER_MOVE,
                        format!("'{name}' is used after move: read from '{target}' instead"),
                    )
                    .with_primary(span, ""),
                );
            }
            // PLAN 3.2.2: the borrow would outlive the source binding.
            let key = BorrowKey::Var(name.to_string());
            if self.is_live(&key) {
                self.emit(
                    Diagnostic::error(
                        CODE_MOVE_WHILE_BORROWED,
                        "move transfers an owner while a borrow of its value is live: release the borrow first",
                    )
                    .with_primary(span, ""),
                );
                self.end_borrow(&key);
            }
            // Ownership (and a held borrow) transfers to the target; the
            // target starts out owning its value.
            info.moved = false;
            self.bind_var_info(target, info, Slot::opaque());
            if let Some(src) = self
                .scopes
                .iter_mut()
                .rev()
                .find_map(|s| s.vars.get_mut(name))
            {
                src.holds = None;
                src.moved = true;
            }
            return;
        }
        self.check_expr(source, span);
        let slot = self.pop_releasing().unwrap_or_else(Slot::opaque);
        // The target owns the moved value; its static type is the moved
        // slot's type (opaque when the value is opaque).
        let ty = slot.ty().clone();
        let is_pointer = matches!(slot, Slot::Pointer(_));
        self.bind_var_info(
            target,
            VarInfo {
                is_copy: matches!(slot, Slot::Copy(_) | Slot::Pointer(_)),
                is_pointer,
                is_reference: false,
                struct_name: None,
                ty,
                holds: None,
                moved: false,
                mutability: Mutability::Mutable,
            },
            slot,
        );
    }

    /// Value / error / union `match`: the subject is borrowed for the whole
    /// match, each arm runs in its own scope, and the prior stack is
    /// restored at the end (extras from the taken arm survive).
    fn check_match(
        &mut self,
        value: &Expr,
        cases: &[crate::parser::ast::MatchCase],
        else_branch: &[Stmt],
        span: Span,
    ) {
        let has_subject = !matches!(value, Expr::Variable { name } if name.is_empty());
        let pre = self.stack.len();
        if has_subject {
            self.check_expr(value, span);
        }
        let subject_depth = self.stack.len();
        let subject_slot = self.stack.last().cloned();

        let mut first_tail: Option<Vec<Slot>> = None;
        for case in cases {
            if let MatchCaseKind::Condition(condition) = &case.kind {
                // Conditions run with the subject on top of the current
                // stack; simulate them on a scratch copy. PLAN 6.5: the
                // condition must be bool (E313).
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                self.check_expr(condition, case.span);
                let condition_slot = self.pop_releasing();
                self.check_condition(condition_slot, "match condition", case.span);
                self.live = saved_live;
                self.stack = saved;
            }
            let saved = self.stack.clone();
            let saved_live = self.live.clone();
            if has_subject && matches!(case.kind, MatchCaseKind::Type(_)) {
                // The arm receives a borrowed reference to the member.
                self.push_slot(Slot::opaque());
            }
            self.push_scope();
            self.check_body(&case.body);
            self.pop_scope();
            let tail = self.split_tail(subject_depth);
            self.live = saved_live;
            self.stack = saved;
            if first_tail.is_none() {
                first_tail = Some(tail);
            }
        }
        if !else_branch.is_empty() {
            let saved = self.stack.clone();
            let saved_live = self.live.clone();
            self.push_scope();
            self.check_body(else_branch);
            self.pop_scope();
            let tail = self.split_tail(subject_depth);
            self.live = saved_live;
            self.stack = saved;
            if first_tail.is_none() {
                first_tail = Some(tail);
            }
        }

        // End of match: the subject borrow is released and the prior stack
        // is restored; extras from the (statically first) arm survive.
        self.stack.truncate(pre);
        if let Some(tail) = first_tail {
            for slot in tail {
                self.push_slot(slot);
            }
        }
        if let Some(Slot::Borrow(key, _)) = subject_slot {
            self.live.remove(&key);
        }
    }

    // -----------------------------------------------------------------
    // Expressions (stack words)
    // -----------------------------------------------------------------

    /// Check a binding initializer (PLAN 2.1.1). The trailing word of
    /// the initializer sequence is the value; when the declared type is
    /// a container, an empty `()`/`[]` there is the typed context the
    /// spec requires. Earlier words are side effects and stay subject
    /// to the bare-literal rule.
    fn check_binding_init(&mut self, value: &Expr, span: Span, allow_empty: bool) {
        match value {
            Expr::Seq(elements) => {
                for (element, element_span) in &elements[..elements.len().saturating_sub(1)] {
                    self.check_expr(element, *element_span);
                }
                if let Some((last, last_span)) = elements.last() {
                    self.check_binding_init(last, *last_span, allow_empty);
                }
            }
            Expr::List(elements) if elements.is_empty() => {
                if !allow_empty {
                    self.reject_empty_lit(span);
                }
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp, Ty::Opaque));
            }
            Expr::Array(elements) if elements.is_empty() => {
                if !allow_empty {
                    self.reject_empty_lit(span);
                }
                self.push_slot(Slot::Copy(Ty::Opaque));
            }
            other => self.check_expr(other, span),
        }
    }

    /// Emit E395 for an empty container literal with no typed context.
    fn reject_empty_lit(&mut self, span: Span) {
        self.emit(
            Diagnostic::error(
                CODE_EMPTY_LITERAL_UNTYPED,
                "empty `()`/`[]`/`{}` requires a typed context (e.g., variable declaration)",
            )
            .with_primary(span, "")
            .with_help(
                "bind it in a declaration with a container type, e.g. `() xs const list<i32>`",
            ),
        );
    }

    /// Check one expression, applying its stack effect to the abstract
    /// stack. `span` locates the expression for diagnostics.
    fn check_expr(&mut self, expr: &Expr, span: Span) {
        self.current_span = span;
        match expr {
            // PLAN 6.1: literals push their smallest-fitting type; an
            // integer literal keeps its decoded value so the binding
            // check can range-check it against the declared type.
            Expr::Integer { value } => {
                let ty = decode_int_literal(value)
                    .map(Ty::IntLit)
                    .unwrap_or(Ty::Opaque);
                self.push_slot(Slot::Copy(ty));
            }
            Expr::Float { .. } => self.push_slot(Slot::Copy(Ty::Prim(14))),
            Expr::Bool { .. } => self.push_slot(Slot::Copy(Ty::Prim(0))),
            Expr::Rune { .. } => self.push_slot(Slot::Copy(Ty::Prim(11))),
            Expr::TypeValue { .. } => self.push_slot(Slot::Copy(Ty::TypeValue)),
            Expr::String { .. } => {
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp, Ty::Prim(16)));
            }
            Expr::Variable { name } => self.check_var_read(name, span),
            Expr::Member { base, member } => self.check_member(base, member, span),
            Expr::Binary { op, left, right } => {
                self.check_expr(left, span);
                self.check_expr(right, span);
                self.apply_bin(*op, span);
            }
            Expr::ApplyBin(op) => self.apply_bin(*op, span),
            Expr::Unary { operand, .. } => {
                self.check_expr(operand, span);
                let slot = self.pop_releasing();
                let result = self.not_result(slot, span);
                self.push_slot(result);
            }
            Expr::ApplyUn(_) => {
                let slot = self.pop_releasing();
                let result = self.not_result(slot, span);
                self.push_slot(result);
            }
            Expr::Call { target } => self.check_call(target, span),
            Expr::Unwrap { inner } => {
                // PLAN 5.2.2: `unwrap` propagates a failure to the caller,
                // so the caller must declare a fallible return. The checker
                // rejects it only when the operand is statically an error
                // envelope (a call to a visible fallible function); opaque
                // operands keep the runtime E308 backstop.
                if !self.caller_fallible && self.unwrap_operand_is_envelope(inner) {
                    let caller = if self.caller_fq.is_empty() {
                        "the top level".to_string()
                    } else {
                        format!("'{}'", self.caller_fq)
                    };
                    self.emit(
                        Diagnostic::error(
                            CODE_UNWRAP_NON_FALLIBLE,
                            format!(
                                "unwrap propagates an error to the caller, but {caller} \
                                 cannot error: no fallible return is declared"
                            ),
                        )
                        .with_primary(span, "")
                        .with_help(
                            "declare the return `with |payload error|`, or recover in place \
                             with `handle ... fallback ... end`",
                        ),
                    );
                }
                self.check_expr(inner, span);
                self.pop_n(1);
                self.push_slot(Slot::opaque());
            }
            Expr::Typeof { inner } => {
                self.check_expr(inner, span);
                self.apply_typeof();
            }
            Expr::ApplyTypeof => self.apply_typeof(),
            Expr::Borrow { inner } => {
                self.check_expr(inner, span);
                self.apply_borrow(span);
            }
            Expr::ApplyBorrow => self.apply_borrow(span),
            Expr::Load { inner } => {
                self.check_expr(inner, span);
                self.require_unsafe(span, "load through pointer<T>");
                self.pop_n(1);
                self.push_slot(Slot::opaque());
            }
            Expr::ApplyLoad => {
                self.require_unsafe(span, "load through pointer<T>");
                self.pop_n(1);
                self.push_slot(Slot::opaque());
            }
            Expr::Store { addr, value } => {
                self.check_expr(addr, span);
                self.check_expr(value, span);
                self.require_unsafe(span, "store through pointer<T>");
                self.pop_n(2);
            }
            Expr::Builtin { name } => {
                // PLAN 3.5.2: the raw memory builtins are unsafe
                // operations.
                if matches!(name.as_str(), "alloc" | "free" | "load" | "store") {
                    self.require_unsafe(span, &format!("@{name}"));
                }
                self.unknown_call();
            }
            Expr::StackOp(op) => self.check_stack_op(*op, span),
            Expr::Array(elements) => {
                if elements.is_empty() {
                    self.reject_empty_lit(span);
                }
                for element in elements {
                    self.check_expr(element, span);
                }
                self.pop_n(elements.len());
                self.push_slot(Slot::Copy(Ty::Opaque));
            }
            Expr::List(elements) => {
                if elements.is_empty() {
                    self.reject_empty_lit(span);
                }
                for element in elements {
                    self.check_expr(element, span);
                }
                self.pop_n(elements.len());
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp, Ty::Opaque));
            }
            Expr::Map(entries) => {
                for (key, value) in entries {
                    self.check_expr(key, span);
                    self.check_expr(value, span);
                }
                self.pop_n(entries.len() * 2);
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp, Ty::Opaque));
            }
            Expr::StructLit(fields) => {
                for (_, value) in fields {
                    self.check_expr(value, span);
                }
                self.pop_n(fields.len());
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp, Ty::Opaque));
            }
            Expr::EmptyMapOrStruct => self.push_slot(Slot::opaque()),
            Expr::Seq(elements) => {
                for (element, element_span) in elements {
                    self.check_expr(element, *element_span);
                }
            }
        }
    }

    /// PLAN 6.3: `not` is logical on bools and bitwise on integers (the
    /// grammar tour's `5 not`); any other operand type is E311. The
    /// result keeps the operand's family.
    fn not_result(&mut self, slot: Option<Slot>, span: Span) -> Slot {
        let Some(slot) = slot else {
            return Slot::opaque();
        };
        let ty = slot.ty().clone();
        if matches!(ty, Ty::Opaque) {
            Slot::opaque()
        } else if ty.is_bool() {
            Slot::Copy(Ty::Prim(0))
        } else if ty.is_int() {
            Slot::Copy(Ty::Prim(4))
        } else {
            self.emit(
                Diagnostic::error(
                    CODE_OPERATOR_TYPE_MISMATCH,
                    format!(
                        "'not' requires a bool or integer operand, not {}",
                        ty_label(&ty)
                    ),
                )
                .with_primary(span, "")
                .with_help("'not' is logical on bools and bitwise on integers"),
            );
            Slot::Copy(Ty::Prim(4))
        }
    }

    /// Reading a variable pushes a copy (copy types), a borrow (non-copy
    /// types), or an opaque lend (`reference<T>` bindings). The slot
    /// carries the binding's declared type (PLAN 6.1).
    fn check_var_read(&mut self, name: &str, span: Span) {
        let Some(var) = self.lookup_var(name) else {
            // Unknown name: a named type used as a value, or a function
            // name; opaque.
            self.push_slot(Slot::opaque());
            return;
        };
        if var.moved {
            // PLAN 3.2.1: the name was emptied by `move`.
            let is_copy = var.is_copy;
            let ty = var.ty.clone();
            self.emit(
                Diagnostic::error(
                    CODE_USE_AFTER_MOVE,
                    format!("'{name}' is used after move: the value now lives in another variable"),
                )
                .with_primary(span, ""),
            );
            // The read still happens at runtime; model it without borrow
            // tracking.
            self.push_slot(if is_copy {
                Slot::Copy(ty)
            } else {
                Slot::Unknown(ty)
            });
            return;
        }
        if var.is_reference {
            // Reading a reference lends it; the binding keeps holding the
            // borrow, so this creates no tracked borrow.
            self.push_slot(Slot::Unknown(var.ty.clone()));
        } else if var.is_copy {
            self.push_slot(if var.is_pointer {
                Slot::Pointer(var.ty.clone())
            } else {
                Slot::Copy(var.ty.clone())
            });
        } else {
            let slot = self.create_borrow(BorrowKey::Var(name.to_string()), var.ty.clone(), span);
            self.push_slot(slot);
        }
    }

    /// Member access. A variable base is read transiently (peek at runtime),
    /// so it creates no tracked borrow; the member value is opaque.
    fn check_member(&mut self, base: &Expr, member: &str, span: Span) {
        if let Expr::Variable { name } = base {
            // `loop.break` / `loop.continue` are flow words: no value.
            if let Some(path) = self.aliases.get(name)
                && path == "std.loop"
                && (member == "break" || member == "continue")
            {
                return;
            }
            // Enum members and error tags are copy values. PLAN 3.2: a
            // string-carried enum member is a heap string, so it pushes an
            // owned temporary; `dup` on it is the non-copy E371. PLAN 6.1:
            // member values are opaque (the checker does not model field
            // or member types).
            if self.is_decl_name(name) {
                if self.decls.is_string_enum(name) {
                    let temp = self.fresh_temp();
                    self.push_slot(Slot::Owned(temp, Ty::Opaque));
                } else {
                    self.push_slot(Slot::Copy(Ty::Opaque));
                }
                return;
            }
            // PLAN 3.2.1: field access reads the base variable.
            if self.lookup_var(name).is_some_and(|var| var.moved) {
                self.emit(
                    Diagnostic::error(
                        CODE_USE_AFTER_MOVE,
                        format!(
                            "'{name}' is used after move: the value now lives in another variable"
                        ),
                    )
                    .with_primary(span, ""),
                );
            }
            // Field access on a local, or a module item: opaque.
            self.push_slot(Slot::opaque());
            return;
        }
        self.check_expr(base, span);
        self.pop_n(1);
        self.push_slot(Slot::opaque());
    }

    /// `call`: a known function pops its parameters and pushes its returns;
    /// anything else is an opaque module call.
    fn check_call(&mut self, target: &Expr, span: Span) {
        match target {
            Expr::Variable { name } => match self.resolve_function(name) {
                Some(summary) => {
                    let (is_unsafe, params, returns) = (
                        summary.is_unsafe,
                        summary.params.clone(),
                        summary.returns.clone(),
                    );
                    // PLAN 3.5.1: calling an `unsafe function` needs an
                    // unsafe context.
                    if is_unsafe {
                        self.require_unsafe(span, &format!("call to unsafe function '{name}'"));
                    }
                    // PLAN 6.4: operand count and operand types (E312).
                    self.check_call_operands(name, &params, span);
                    self.pop_n(params.len());
                    for ret in returns {
                        self.push_slot(Slot::Unknown(ret));
                    }
                }
                None => self.unknown_call(),
            },
            Expr::Member { base, member } => {
                if let Expr::Variable { name } = base.as_ref() {
                    // Module alias: opaque module function.
                    if self.aliases.contains_key(name) {
                        self.unknown_call();
                        return;
                    }
                    // Method on a local variable of a named struct, enum,
                    // or union type: the receiver is the first parameter.
                    if let Some(var) = self.lookup_var(name).cloned() {
                        // PLAN 3.2.1: the call reads the receiver.
                        if var.moved {
                            self.emit(
                                Diagnostic::error(
                                    CODE_USE_AFTER_MOVE,
                                    format!(
                                        "'{name}' is used after move: the value now lives in another variable"
                                    ),
                                )
                                .with_primary(span, ""),
                            );
                        }
                        if let Some(struct_name) = &var.struct_name {
                            let fq = format!("{struct_name}::{member}");
                            if let Some(summary) = self.decls.function(&fq) {
                                let (is_unsafe, params, returns) = (
                                    summary.is_unsafe,
                                    summary.params.clone(),
                                    summary.returns.clone(),
                                );
                                // PLAN 3.5.1: calling an `unsafe function`
                                // needs an unsafe context.
                                if is_unsafe {
                                    self.require_unsafe(
                                        span,
                                        &format!("call to unsafe function '{fq}'"),
                                    );
                                }
                                // PLAN 6.4: operand count and operand types
                                // (E312); the receiver is operand 1.
                                self.check_call_operands(&fq, &params, span);
                                self.pop_n(params.len());
                                for ret in returns {
                                    self.push_slot(Slot::Unknown(ret));
                                }
                                return;
                            }
                        }
                    }
                }
                // Stage 4 (PLAN 4.1.1): a member-chain target
                // (`o.inner.getv call`) resolves left to right through
                // struct fields. The call pushes the receiver itself, so
                // only the operands for the remaining parameters sit on
                // the stack.
                if !matches!(base.as_ref(), Expr::Variable { .. })
                    && let Some((fq, root)) = self.resolve_chain_method(base, member)
                {
                    // PLAN 3.2.1: the chain reads the root variable.
                    if self.lookup_var(&root).is_some_and(|var| var.moved) {
                        self.emit(
                            Diagnostic::error(
                                CODE_USE_AFTER_MOVE,
                                format!(
                                    "'{root}' is used after move: the value now lives in another variable"
                                ),
                            )
                            .with_primary(span, ""),
                        );
                    }
                    let summary = self
                        .decls
                        .function(&fq)
                        .expect("resolve_chain_method found it");
                    let (is_unsafe, params, returns) = (
                        summary.is_unsafe,
                        summary.params.clone(),
                        summary.returns.clone(),
                    );
                    // PLAN 3.5.1: calling an `unsafe function` needs an
                    // unsafe context.
                    if is_unsafe {
                        self.require_unsafe(span, &format!("call to unsafe function '{fq}'"));
                    }
                    // PLAN 6.4: operand count and operand types (E312);
                    // the receiver is parameter 1 and is not pushed by
                    // the caller.
                    let user_params: &[Ty] = params.get(1..).unwrap_or(&[]);
                    self.check_call_operands(&fq, user_params, span);
                    self.pop_n(params.len().saturating_sub(1));
                    for ret in returns {
                        self.push_slot(Slot::Unknown(ret));
                    }
                    return;
                }
                self.unknown_call();
            }
            _ => self.unknown_call(),
        }
    }

    /// Stage 4 (PLAN 4.1.1): statically resolve a member-chain call
    /// target (`o.inner.getv call`). The root must be a local of a named
    /// struct type; each intermediate member must be a field of that
    /// struct whose type is again a named struct (a `reference<T>` field
    /// autoderefs). The final member names a method on the last field's
    /// struct, enum, or union type. Returns the method's fully-qualified
    /// name and the root variable; anything unresolvable stays opaque and
    /// the interpreter backstops with the failing link's own diagnostic.
    fn resolve_chain_method(&self, base: &Expr, member: &str) -> Option<(String, String)> {
        let mut fields: Vec<&str> = Vec::new();
        let mut expr = base;
        let root = loop {
            match expr {
                Expr::Member {
                    base: inner,
                    member,
                } => {
                    fields.push(member);
                    expr = inner;
                }
                Expr::Variable { name } => break name.clone(),
                _ => return None,
            }
        };
        fields.reverse();
        let var = self.lookup_var(&root)?;
        let mut ty_name = match var.ty {
            Ty::Named(ref n) => n.clone(),
            _ => return None,
        };
        for (i, field) in fields.iter().enumerate() {
            let decl = self.decls.structs.get(&ty_name)?;
            let field = decl.fields.iter().find(|f| f.name == *field)?;
            let mut fty = &field.ty;
            while let TypeKind::Reference { inner } = &fty.kind {
                fty = inner;
            }
            let TypeKind::Named(n) = &fty.kind else {
                return None;
            };
            let last = i + 1 == fields.len();
            if last {
                let method_target =
                    self.decls.is_struct(n) || self.decls.is_enum(n) || self.decls.is_union(n);
                if !method_target {
                    return None;
                }
            } else if !self.decls.is_struct(n) {
                return None;
            }
            ty_name = n.clone();
        }
        let fq = format!("{ty_name}::{member}");
        self.decls.function(&fq).map(|_| (fq, root))
    }

    /// PLAN 6.4: for functions visible to the checker, reject a call with
    /// too few operands for the parameter count and type-check the
    /// operands against the parameter types (the PLAN 6.2 coercion
    /// matrix). The count check needs a reliable stack height: an opaque
    /// call earlier in the region makes the height uncomparable (the
    /// PLAN 5.5 flag), so it is skipped there. Operand types are checked
    /// whenever the operand slot's type is known; opaque operands skip
    /// individually.
    fn check_call_operands(&mut self, name: &str, params: &[Ty], span: Span) {
        let n = params.len();
        if !self.height_opaque && self.stack.len() < n {
            self.emit(
                Diagnostic::error(
                    CODE_CALL_TYPE_MISMATCH,
                    format!(
                        "call to '{name}' needs {n} operand(s), but the stack has {}",
                        self.stack.len()
                    ),
                )
                .with_primary(span, "")
                .with_help("push the operand(s) before the call word"),
            );
            return;
        }
        let Some(base) = self.stack.len().checked_sub(n) else {
            return;
        };
        for (i, param) in params.iter().enumerate() {
            let Some(slot) = self.stack.get(base + i) else {
                continue;
            };
            if !self.binds_to(slot.ty(), param) {
                self.emit(
                    Diagnostic::error(
                        CODE_CALL_TYPE_MISMATCH,
                        format!(
                            "operand {} of the call to '{name}' is {}, but the parameter is {}",
                            i + 1,
                            ty_label(slot.ty()),
                            ty_label(param)
                        ),
                    )
                    .with_primary(span, "")
                    .with_help(
                        "operands coerce like bindings: an integer widens to an integer \\\
                         parameter; every other mismatch is rejected",
                    ),
                );
            }
        }
    }

    /// Resolve a plain call target the way `resolve_call_name` does: the
    /// enclosing function's nested functions first, then the plain name.
    fn resolve_function(&self, name: &str) -> Option<&FuncSummary> {
        if !self.caller_fq.is_empty() {
            let nested = format!("{}::{}", self.caller_fq, name);
            if let Some(summary) = self.decls.function(&nested) {
                return Some(summary);
            }
        }
        self.decls.function(name)
    }

    /// PLAN 5.2.2: whether the operand of `unwrap` is statically an error
    /// envelope: a call to a function visible to the checker whose return
    /// is the fallible form. Module calls and builtins are opaque (the
    /// runtime E308 backstops them), and so is any other operand word;
    /// `unwrap` on such a value is the identity no-op the runtime defines.
    fn unwrap_operand_is_envelope(&self, inner: &Expr) -> bool {
        let Expr::Call { target } = inner else {
            return false;
        };
        match target.as_ref() {
            Expr::Variable { name } => self.resolve_function(name).is_some_and(|f| f.fallible),
            Expr::Member { base, member } => {
                let Expr::Variable { name } = base.as_ref() else {
                    return false;
                };
                if self.aliases.contains_key(name) {
                    return false;
                }
                let Some(struct_name) = self
                    .lookup_var(name)
                    .and_then(|var| var.struct_name.clone())
                else {
                    return false;
                };
                let fq = format!("{struct_name}::{member}");
                self.decls.function(&fq).is_some_and(|f| f.fallible)
            }
            _ => false,
        }
    }

    /// `typeof` releases the borrow of a borrowed heap value and pushes the
    /// type value (PLAN 6.1: a type value is its own family, comparable
    /// with other type values).
    fn apply_typeof(&mut self) {
        self.pop_releasing();
        self.push_slot(Slot::Copy(Ty::TypeValue));
    }

    /// The `borrow` word: an owned value becomes a borrow of itself; an
    /// existing borrow moves (its liveness continues); a copy stays a copy.
    /// The borrowed value's static type is unchanged (PLAN 6.1).
    fn apply_borrow(&mut self, span: Span) {
        match self.stack.pop() {
            Some(Slot::Owned(temp, ty)) => {
                let slot = self.create_borrow(BorrowKey::Temp(temp), ty, span);
                self.push_slot(slot);
            }
            Some(slot) => self.push_slot(slot),
            None => self.push_slot(Slot::opaque()),
        }
    }

    /// Pop two operands and push the binary result. `+` / `-` over a
    /// pointer is pointer arithmetic (PLAN 3.5.2): it needs an unsafe
    /// context and the result stays a pointer. PLAN 6.3: the operands
    /// must match the operator's families (E311) and the result carries
    /// the operator's result type.
    fn apply_bin(&mut self, op: BinOp, span: Span) {
        let rhs = self.stack.get(self.stack.len().saturating_sub(1)).cloned();
        let lhs = self.stack.get(self.stack.len().saturating_sub(2)).cloned();
        let pointer_arith = matches!(op, BinOp::Plus | BinOp::Minus)
            && (matches!(lhs, Some(Slot::Pointer(_))) || matches!(rhs, Some(Slot::Pointer(_))));
        if pointer_arith {
            self.require_unsafe(span, "pointer arithmetic");
        }
        // PLAN 6.3: operand families (E311). A pointer operand's type is
        // opaque, so pointer arithmetic skips the check like any other
        // opaque operand.
        self.check_bin_operands(op, lhs.as_ref(), rhs.as_ref(), span);
        self.pop_n(2);
        if op == BinOp::Concat {
            let temp = self.fresh_temp();
            self.push_slot(Slot::Owned(temp, Ty::Prim(16)));
        } else if pointer_arith {
            let ty = match (&lhs, &rhs) {
                (Some(Slot::Pointer(ty)), _) => ty.clone(),
                (_, Some(Slot::Pointer(ty))) => ty.clone(),
                _ => Ty::Opaque,
            };
            self.push_slot(Slot::Pointer(ty));
        } else {
            self.push_slot(Slot::Copy(bin_result_ty(op, lhs.as_ref(), rhs.as_ref())));
        }
    }

    /// PLAN 3.5 / audit-3 Stage 5: `what` needs an active `unsafe ... end`
    /// block. An `unsafe function` body is not an unsafe context; the
    /// marker only documents the escape.
    fn require_unsafe(&mut self, span: Span, what: &str) {
        if self.unsafe_depth == 0 {
            self.emit(
                Diagnostic::error(
                    CODE_UNSAFE_OUTSIDE,
                    format!("{what} requires an unsafe context: wrap it in 'unsafe ... end'"),
                )
                .with_primary(span, ""),
            );
        }
    }

    /// PLAN 3.3.1: `dup` on a non-copy value. The stack effect is still
    /// modeled so one mistake yields one diagnostic, not a cascade.
    fn emit_dup_non_copy(&mut self, span: Span) {
        self.emit(
            Diagnostic::error(
                CODE_DUP_NON_COPY,
                "dup applies to copy types only: borrow the value instead of duplicating it",
            )
            .with_primary(span, ""),
        );
    }

    /// PLAN 6.3: operator operand families (E311). Arithmetic requires
    /// numeric operands, `//` `%` `^` and the shifts require integers,
    /// `and` / `or` / `xor` accept bools or integers (the same words are
    /// logical on bools and bitwise on integers, and the runtime mixes
    /// the two for `and` / `or` / `xor`), `~` requires strings, and
    /// comparisons require matching families (the runtime promotes
    /// mixed integers and floats). An operand whose type is opaque
    /// skips the check.
    fn check_bin_operands(
        &mut self,
        op: BinOp,
        lhs: Option<&Slot>,
        rhs: Option<&Slot>,
        span: Span,
    ) {
        let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
            return;
        };
        if matches!(lhs.ty(), Ty::Opaque) || matches!(rhs.ty(), Ty::Opaque) {
            return;
        }
        let ok = match op {
            BinOp::Concat => lhs.ty().is_string() && rhs.ty().is_string(),
            BinOp::Eq | BinOp::Ne | BinOp::Gt | BinOp::Gte | BinOp::Lt | BinOp::Lte => {
                comparable(lhs.ty(), rhs.ty())
            }
            BinOp::Plus | BinOp::Minus | BinOp::Mul | BinOp::Div => {
                lhs.ty().is_numeric() && rhs.ty().is_numeric()
            }
            BinOp::Fdiv | BinOp::Mod | BinOp::Pow | BinOp::Lshift | BinOp::Rshift => {
                lhs.ty().is_int() && rhs.ty().is_int()
            }
            BinOp::And | BinOp::Or | BinOp::Xor => {
                (lhs.ty().is_int() || lhs.ty().is_bool())
                    && (rhs.ty().is_int() || rhs.ty().is_bool())
            }
        };
        if !ok {
            self.emit(
                Diagnostic::error(
                    CODE_OPERATOR_TYPE_MISMATCH,
                    format!(
                        "'{}' cannot be applied to {} and {}",
                        bin_op_label(op),
                        ty_label(lhs.ty()),
                        ty_label(rhs.ty())
                    ),
                )
                .with_primary(span, "")
                .with_help(bin_op_help(op)),
            );
        }
    }

    /// Stack manipulation words.
    fn check_stack_op(&mut self, op: StackOp, span: Span) {
        match op {
            StackOp::Dup => match self.stack.last().cloned() {
                // PLAN 3.3.1: an owned heap temporary is a non-copy value;
                // `dup` is restricted to copy types. Report it, then keep
                // modeling a stack the rest of the walk can follow so one
                // mistake yields one diagnostic, not a cascade.
                Some(Slot::Owned(temp, ty)) => {
                    self.emit_dup_non_copy(span);
                    let key = BorrowKey::Temp(temp);
                    if !self.is_live(&key) {
                        self.live.insert(key.clone(), self.scopes.len() - 1);
                    }
                    self.push_slot(Slot::Borrow(key, ty));
                }
                // Audit-3 PLAN 1.2.1: duplicating a borrow or reference is
                // legal (references are not in the spec's non-copy list).
                // Push the same borrow without re-registering it: the
                // original borrow is already live, and a second E375 here
                // would cascade.
                Some(Slot::Borrow(key, ty)) => {
                    self.push_slot(Slot::Borrow(key, ty));
                }
                Some(slot) => self.push_slot(slot),
                None => self.push_slot(Slot::opaque()),
            },
            StackOp::Swap => {
                let n = self.stack.len();
                if n >= 2 {
                    self.stack.swap(n - 1, n - 2);
                }
            }
            StackOp::Rot => {
                let n = self.stack.len();
                if n >= 3 {
                    let a = self.stack.remove(n - 3);
                    self.stack.push(a);
                }
            }
            StackOp::Unrot => {
                let n = self.stack.len();
                if n >= 3 {
                    let a = self.stack.pop().unwrap();
                    self.stack.insert(n - 3, a);
                }
            }
            StackOp::Pop => {
                // PLAN 3.1.2: popping an owner while a borrow of its value
                // is live leaves a dangling reference.
                if let Some(Slot::Owned(temp, _)) = self.pop_releasing() {
                    let key = BorrowKey::Temp(temp);
                    if self.is_live(&key) {
                        self.emit(
                            Diagnostic::error(
                                CODE_OWNER_WHILE_BORROWED,
                                "pop drops an owner while a borrow of its value is live: release the borrow first",
                            )
                            .with_primary(span, ""),
                        );
                        self.live.remove(&key);
                    }
                }
            }
            StackOp::Drop => {
                // PLAN 3.1.2: freeing an owner dangles any borrow held
                // through a `reference<T>` binding (on-stack borrows are
                // released by the same drop).
                while let Some(slot) = self.stack.pop() {
                    match slot {
                        Slot::Owned(temp, _) => {
                            let key = BorrowKey::Temp(temp);
                            if self.held_by_reference(&key) {
                                self.emit(
                                    Diagnostic::error(
                                        CODE_OWNER_WHILE_BORROWED,
                                        "drop frees an owner while a borrow of its value is live: release the borrow first",
                                    )
                                    .with_primary(span, ""),
                                );
                                self.live.remove(&key);
                            }
                        }
                        Slot::Borrow(key, _) => {
                            self.live.remove(&key);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Saved borrow-machine state across one function body.
struct MachineSnapshot {
    stack: Vec<Slot>,
    scopes: Vec<Scope>,
    live: HashMap<BorrowKey, usize>,
    aliases: HashMap<String, String>,
    caller_fq: String,
    caller_fallible: bool,
}

/// Check one parsed program and return the collected diagnostics (PLAN 3.0
/// session wiring). An empty batch means the program passed the check.
pub fn check_program(program: &Program, path: &str, error_limit: usize) -> DiagnosticBatch {
    let mut checker = Checker::new(path, error_limit);
    checker.check_program(program);
    checker.into_batch()
}

/// PLAN 5.3.1: normalized union member key, matching the interpreter's
/// `member_ty`: references strip, and both `string` spellings
/// (`Primitive(String)` and `Named("string")`) unify. Returns `None` for
/// member shapes the interpreter rejects at registration; those keep their
/// existing unsupported-member path.
fn union_member_key(t: &Type) -> Option<String> {
    match &t.kind {
        TypeKind::Primitive(Primitive::String) => Some("string".to_string()),
        TypeKind::Named(n) if n == "string" => Some("string".to_string()),
        TypeKind::Primitive(p) => Some(format!("prim:{p:?}")),
        TypeKind::Named(n) => Some(format!("named:{n}")),
        TypeKind::Reference { inner } => union_member_key(inner),
        // Stage 2: container members are distinct by their full shape,
        // mirroring the interpreter's kind-code comparison.
        TypeKind::List { element } => Some(format!("list:<{}>", union_member_key(element)?)),
        TypeKind::Hashmap { key, value } => Some(format!(
            "hashmap:<{} {}>",
            union_member_key(key)?,
            union_member_key(value)?
        )),
        TypeKind::Array { element, size } => Some(format!(
            "array:<{} {}>",
            union_member_key(element)?,
            size.map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string())
        )),
        _ => None,
    }
}

/// Human-readable name of a union member type, for the `E385` message.
/// Mirrors the interpreter's `member_ty_label`.
fn union_member_label(t: &Type) -> String {
    match &t.kind {
        TypeKind::Reference { inner } => union_member_label(inner),
        TypeKind::Named(n) => n.clone(),
        TypeKind::Primitive(p) => format!("{p:?}").to_lowercase(),
        TypeKind::List { .. } | TypeKind::Hashmap { .. } | TypeKind::Array { .. } => type_label(t),
        _ => "member".to_string(),
    }
}

/// PLAN 6.3: whether two comparison operands are of matching families:
/// equal types, two numbers (the runtime promotes mixed integers and
/// floats), two strings, two bools, or two type values.
fn comparable(a: &Ty, b: &Ty) -> bool {
    a == b
        || (a.is_numeric() && b.is_numeric())
        || (a.is_string() && b.is_string())
        || (a.is_bool() && b.is_bool())
        || (matches!(a, Ty::TypeValue) && matches!(b, Ty::TypeValue))
}

/// PLAN 6.1: the result type of one binary operator. Comparisons push
/// bool; `/` (true division) always yields a float, mirroring the
/// runtime; `+` `-` `*` yield a float when either operand is a float and
/// an integer otherwise; `and` / `or` / `xor` yield bool only when both
/// operands are bools (the runtime mixes bools into integers otherwise).
/// An opaque operand leaves the result opaque: its width at runtime is
/// not known.
fn bin_result_ty(op: BinOp, lhs: Option<&Slot>, rhs: Option<&Slot>) -> Ty {
    let lhs = lhs.map(Slot::ty);
    let rhs = rhs.map(Slot::ty);
    let opaque = |lhs: Option<&Ty>, rhs: Option<&Ty>| {
        matches!(lhs, Some(Ty::Opaque)) || matches!(rhs, Some(Ty::Opaque))
    };
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Gt | BinOp::Gte | BinOp::Lt | BinOp::Lte => Ty::Prim(0),
        BinOp::Div => Ty::Prim(14),
        BinOp::Plus | BinOp::Minus | BinOp::Mul => {
            if opaque(lhs, rhs) {
                Ty::Opaque
            } else if lhs.is_some_and(|t| t.is_float()) || rhs.is_some_and(|t| t.is_float()) {
                Ty::Prim(14)
            } else {
                Ty::Prim(4)
            }
        }
        BinOp::And | BinOp::Or | BinOp::Xor => {
            if opaque(lhs, rhs) {
                Ty::Opaque
            } else if lhs.is_some_and(|t| t.is_bool()) && rhs.is_some_and(|t| t.is_bool()) {
                Ty::Prim(0)
            } else {
                Ty::Prim(4)
            }
        }
        _ => {
            if opaque(lhs, rhs) {
                Ty::Opaque
            } else {
                Ty::Prim(4)
            }
        }
    }
}

/// Source spelling of a binary operator, for diagnostics.
fn bin_op_label(op: BinOp) -> &'static str {
    match op {
        BinOp::Plus => "+",
        BinOp::Minus => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Fdiv => "//",
        BinOp::Mod => "%",
        BinOp::Pow => "^",
        BinOp::Concat => "~",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Xor => "xor",
        BinOp::Lshift => "lshift",
        BinOp::Rshift => "rshift",
    }
}

/// One-line guidance for the `E311` message of one operator.
fn bin_op_help(op: BinOp) -> &'static str {
    match op {
        BinOp::Plus | BinOp::Minus | BinOp::Mul | BinOp::Div => {
            "arithmetic requires numeric operands"
        }
        BinOp::Fdiv | BinOp::Mod | BinOp::Pow | BinOp::Lshift | BinOp::Rshift => {
            "'//', '%', '^', 'lshift', and 'rshift' require integer operands"
        }
        BinOp::And | BinOp::Or | BinOp::Xor => {
            "'and', 'or', and 'xor' require bool or integer operands"
        }
        BinOp::Concat => "'~' requires string operands",
        BinOp::Eq | BinOp::Ne | BinOp::Gt | BinOp::Gte | BinOp::Lt | BinOp::Lte => {
            "comparisons require operands of matching families"
        }
    }
}
