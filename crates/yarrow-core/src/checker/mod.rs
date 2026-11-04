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
//! - E371: `dup` over an owned heap temporary or a live borrow, both of
//!   which are non-copy values (borrows of copy values stay copies).
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
//! time. An unsafe context is an `unsafe ... end` block or an `unsafe
//! function` body; the borrow and ownership rules above stay active inside
//! it (the unsafe walk uses the same machine):
//!
//! - E370: a call to an `unsafe function` outside an unsafe context.
//! - E370: an unsafe operation outside an unsafe context: the `@alloc`,
//!   `@free`, `@load`, `@store` builtins, `load` / `store` through
//!   `pointer<T>`, and pointer arithmetic (`+` / `-` over a pointer).
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
use crate::parser::ast::{
    BinOp, EnumDecl, ErrorDecl, Expr, Function, MatchCaseKind, Mutability, Primitive, Program,
    StackOp, Stmt, StmtKind, StructDecl, Type, TypeKind, UnionDecl,
};
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

/// One abstract slot of the operand stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// Owned heap temporary; the stack frees it at pop / drop / consume.
    Owned(u32),
    /// Copy value: integers, floats, bool, rune, enum, array, type.
    Copy,
    /// `pointer<T>` value: a copy value whose arithmetic is pointer
    /// arithmetic (PLAN 3.5.2).
    Pointer,
    /// Live borrow of a value (from the `borrow` word or a non-copy read).
    Borrow(BorrowKey),
    /// Opaque value: call results, foreign values, reference reads.
    Unknown,
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
    /// Named struct type, for method call resolution.
    struct_name: Option<String>,
    /// Live borrow held through this `reference<T>` binding.
    holds: Option<BorrowKey>,
    /// `move` transferred the value away: further use is E373 (PLAN 3.2.1).
    /// A `set` or a re-declaration re-owns the name and clears the flag.
    moved: bool,
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
    /// `Interpreter::register_unit` for the main unit.
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
        self.decls.functions.insert(
            fq.to_string(),
            FuncSummary {
                fq: fq.to_string(),
                is_unsafe: f.is_unsafe,
                is_method,
                param_count: f.params.len(),
                return_count: f.returns.len(),
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
        if let Slot::Borrow(key) = &slot {
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
    /// value (PLAN 3.1.1, E375).
    fn create_borrow(&mut self, key: BorrowKey, span: Span) -> Slot {
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
            return Slot::Borrow(key);
        }
        self.live.insert(key.clone(), self.scopes.len() - 1);
        Slot::Borrow(key)
    }

    /// A call whose signature is not visible (module function, builtin):
    /// assume every live on-stack borrow was consumed (PLAN 3.1.1
    /// "consumption by a call") and push one opaque result.
    fn unknown_call(&mut self) {
        self.live.clear();
        for slot in &mut self.stack {
            if matches!(slot, Slot::Borrow(_)) {
                *slot = Slot::Unknown;
            }
        }
        self.push_slot(Slot::Unknown);
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
            if let Slot::Borrow(held) = slot
                && held == key
            {
                *slot = Slot::Unknown;
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
        // Slots whose borrow just ended become opaque.
        for slot in &mut self.stack {
            if let Slot::Borrow(key) = slot
                && !self.live.contains_key(key)
            {
                *slot = Slot::Unknown;
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

    /// Bind `name` to the value described by `slot`, with the shape of `ty`.
    fn bind_var(&mut self, name: &str, ty: &Type, slot: Slot) {
        let is_copy = self.is_copy_type(ty);
        let is_pointer = matches!(ty.kind, TypeKind::Pointer { .. });
        let is_reference = matches!(ty.kind, TypeKind::Reference { .. });
        let struct_name = match &ty.kind {
            TypeKind::Named(n) if self.decls.is_struct(n) => Some(n.clone()),
            _ => None,
        };
        self.bind_var_info(
            name,
            VarInfo {
                is_copy,
                is_pointer,
                is_reference,
                struct_name,
                holds: None,
                moved: false,
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
            (Slot::Borrow(key), true) => info.holds = Some(key.clone()),
            // Any other binding consumes the borrow.
            (Slot::Borrow(key), false) => {
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
        self.caller_fq = fq.to_string();
        self.check_return_signature(f);

        for param in &f.params {
            let slot = match &param.ty.kind {
                TypeKind::Reference { .. } => Slot::Unknown,
                TypeKind::Pointer { .. } => Slot::Pointer,
                _ if self.is_copy_type(&param.ty) => Slot::Copy,
                _ => Slot::Owned(self.fresh_temp()),
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
                    struct_name,
                    holds: None,
                    moved: false,
                },
            );
        }

        if f.is_unsafe {
            self.unsafe_depth += 1;
        }
        self.check_body(&f.body);
        if f.is_unsafe {
            self.unsafe_depth -= 1;
        }
        self.restore_machine(saved);
    }

    /// PLAN 4.2 / 4.4: a two-member union return is only supported as the
    /// fallible form, so its error side must resolve. The checker sees
    /// main-unit declarations only: a bare name resolves when this unit
    /// declares it as an error type, and a module-qualified path is
    /// resolved by the interpreter after module loading.
    fn check_return_signature(&mut self, f: &Function) {
        let signature = classify_signature(&f.returns, &|n| self.resolves_error_type(n));
        let Signature::UnknownError { name } = signature else {
            return;
        };
        let span = f
            .returns
            .first()
            .map(|t| Span::from_location(t.location))
            .unwrap_or_default();
        self.emit(
            Diagnostic::error(
                CODE_UNKNOWN_ERROR_TYPE,
                format!("unknown error type '{name}' in the signature of '{}'", f.name),
            )
            .with_primary(span, "")
            .with_help(
                "require the module that declares the error type, or declare the error type in this unit",
            ),
        );
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
        }
    }

    fn restore_machine(&mut self, saved: MachineSnapshot) {
        self.stack = saved.stack;
        self.scopes = saved.scopes;
        self.live = saved.live;
        self.aliases = saved.aliases;
        self.caller_fq = saved.caller_fq;
    }

    fn check_body(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.check_stmt(stmt);
        }
    }

    // -----------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------

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
                let slot = match value {
                    Some(value) => {
                        self.check_expr(value, stmt.span);
                        self.pop_slot_raw()
                    }
                    None => self.pop_slot_raw(),
                }
                .unwrap_or(Slot::Unknown);
                self.bind_var(name, ty, slot);
            }
            StmtKind::Set { target, value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
                let slot = self.pop_slot_raw().unwrap_or(Slot::Unknown);
                if let Expr::Variable { name } = target
                    && let Some(var) = self.lookup_var(name).cloned()
                {
                    let mut info = var;
                    // The old value is dropped: a borrow it held ends.
                    if let Some(key) = info.holds.take() {
                        self.live.remove(&key);
                    }
                    // `set` re-owns the name, even after a `move`.
                    info.moved = false;
                    match (&slot, info.is_reference) {
                        (Slot::Borrow(key), true) => info.holds = Some(key.clone()),
                        (Slot::Borrow(key), false) => {
                            self.live.remove(key);
                        }
                        _ => {}
                    }
                    self.scopes
                        .last_mut()
                        .expect("scope stack is never empty")
                        .vars
                        .insert(name.clone(), info);
                } else if let Slot::Borrow(key) = &slot {
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
            StmtKind::Struct(_) | StmtKind::Enum(_) | StmtKind::Union(_) | StmtKind::Error(_) => {}
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
                self.pop_n(1);
                let base = self.stack.len();
                let saved = self.stack.clone();
                let saved_live = self.live.clone();

                self.push_scope();
                self.check_body(then_branch);
                self.pop_scope();
                let then_tail = self.split_tail(base);
                self.stack = saved.clone();

                let else_tail = if else_branch.is_empty() {
                    Vec::new()
                } else {
                    self.live = saved_live.clone();
                    self.push_scope();
                    self.check_body(else_branch);
                    self.pop_scope();
                    let tail = self.split_tail(base);
                    self.stack = saved.clone();
                    tail
                };
                self.live = saved_live;
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
            }
            StmtKind::For { source, body } => {
                self.check_expr(source, stmt.span);
                // The loop consumes its condition / iterable.
                self.pop_n(1);
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
            self.bind_var_info(target, info, Slot::Unknown);
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
        let slot = self.pop_releasing().unwrap_or(Slot::Unknown);
        // The target owns the moved value; its static type is opaque here.
        self.bind_var_info(
            target,
            VarInfo {
                is_copy: matches!(slot, Slot::Copy | Slot::Pointer),
                is_pointer: matches!(slot, Slot::Pointer),
                is_reference: false,
                struct_name: None,
                holds: None,
                moved: false,
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
                // stack; simulate them on a scratch copy.
                let saved = self.stack.clone();
                let saved_live = self.live.clone();
                self.check_expr(condition, case.span);
                self.pop_n(1);
                self.live = saved_live;
                self.stack = saved;
            }
            let saved = self.stack.clone();
            let saved_live = self.live.clone();
            if has_subject && matches!(case.kind, MatchCaseKind::Type(_)) {
                // The arm receives a borrowed reference to the member.
                self.push_slot(Slot::Unknown);
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
        if let Some(Slot::Borrow(key)) = subject_slot {
            self.live.remove(&key);
        }
    }

    // -----------------------------------------------------------------
    // Expressions (stack words)
    // -----------------------------------------------------------------

    /// Check one expression, applying its stack effect to the abstract
    /// stack. `span` locates the expression for diagnostics.
    fn check_expr(&mut self, expr: &Expr, span: Span) {
        self.current_span = span;
        match expr {
            Expr::Integer { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Rune { .. }
            | Expr::TypeValue { .. } => self.push_slot(Slot::Copy),
            Expr::String { .. } => {
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp));
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
                self.pop_n(1);
                self.push_slot(Slot::Copy);
            }
            Expr::ApplyUn(_) => {
                self.pop_n(1);
                self.push_slot(Slot::Copy);
            }
            Expr::Call { target } => self.check_call(target, span),
            Expr::Unwrap { inner } => {
                self.check_expr(inner, span);
                self.pop_n(1);
                self.push_slot(Slot::Unknown);
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
                self.push_slot(Slot::Unknown);
            }
            Expr::ApplyLoad => {
                self.require_unsafe(span, "load through pointer<T>");
                self.pop_n(1);
                self.push_slot(Slot::Unknown);
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
                for element in elements {
                    self.check_expr(element, span);
                }
                self.pop_n(elements.len());
                self.push_slot(Slot::Copy);
            }
            Expr::List(elements) => {
                for element in elements {
                    self.check_expr(element, span);
                }
                self.pop_n(elements.len());
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp));
            }
            Expr::Map(entries) => {
                for (key, value) in entries {
                    self.check_expr(key, span);
                    self.check_expr(value, span);
                }
                self.pop_n(entries.len() * 2);
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp));
            }
            Expr::StructLit(fields) => {
                for (_, value) in fields {
                    self.check_expr(value, span);
                }
                self.pop_n(fields.len());
                let temp = self.fresh_temp();
                self.push_slot(Slot::Owned(temp));
            }
            Expr::EmptyMapOrStruct => self.push_slot(Slot::Unknown),
            Expr::Seq(elements) => {
                for (element, element_span) in elements {
                    self.check_expr(element, *element_span);
                }
            }
        }
    }

    /// Reading a variable pushes a copy (copy types), a borrow (non-copy
    /// types), or an opaque lend (`reference<T>` bindings).
    fn check_var_read(&mut self, name: &str, span: Span) {
        let Some(var) = self.lookup_var(name) else {
            // Unknown name: a named type used as a value, or a function
            // name; opaque.
            self.push_slot(Slot::Unknown);
            return;
        };
        if var.moved {
            // PLAN 3.2.1: the name was emptied by `move`.
            let is_copy = var.is_copy;
            self.emit(
                Diagnostic::error(
                    CODE_USE_AFTER_MOVE,
                    format!("'{name}' is used after move: the value now lives in another variable"),
                )
                .with_primary(span, ""),
            );
            // The read still happens at runtime; model it without borrow
            // tracking.
            self.push_slot(if is_copy { Slot::Copy } else { Slot::Unknown });
            return;
        }
        if var.is_reference {
            // Reading a reference lends it; the binding keeps holding the
            // borrow, so this creates no tracked borrow.
            self.push_slot(Slot::Unknown);
        } else if var.is_copy {
            self.push_slot(if var.is_pointer {
                Slot::Pointer
            } else {
                Slot::Copy
            });
        } else {
            let slot = self.create_borrow(BorrowKey::Var(name.to_string()), span);
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
            // owned temporary; `dup` on it is the non-copy E371.
            if self.is_decl_name(name) {
                if self.decls.is_string_enum(name) {
                    let temp = self.fresh_temp();
                    self.push_slot(Slot::Owned(temp));
                } else {
                    self.push_slot(Slot::Copy);
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
            self.push_slot(Slot::Unknown);
            return;
        }
        self.check_expr(base, span);
        self.pop_n(1);
        self.push_slot(Slot::Unknown);
    }

    /// `call`: a known function pops its parameters and pushes its returns;
    /// anything else is an opaque module call.
    fn check_call(&mut self, target: &Expr, span: Span) {
        match target {
            Expr::Variable { name } => match self.resolve_function(name) {
                Some(summary) => {
                    let (is_unsafe, params, returns) =
                        (summary.is_unsafe, summary.param_count, summary.return_count);
                    // PLAN 3.5.1: calling an `unsafe function` needs an
                    // unsafe context.
                    if is_unsafe {
                        self.require_unsafe(span, &format!("call to unsafe function '{name}'"));
                    }
                    self.pop_n(params);
                    for _ in 0..returns {
                        self.push_slot(Slot::Unknown);
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
                    // Method on a local struct variable: the receiver is
                    // the first parameter.
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
                                let (is_unsafe, params, returns) =
                                    (summary.is_unsafe, summary.param_count, summary.return_count);
                                // PLAN 3.5.1: calling an `unsafe function`
                                // needs an unsafe context.
                                if is_unsafe {
                                    self.require_unsafe(
                                        span,
                                        &format!("call to unsafe function '{fq}'"),
                                    );
                                }
                                self.pop_n(params);
                                for _ in 0..returns {
                                    self.push_slot(Slot::Unknown);
                                }
                                return;
                            }
                        }
                    }
                }
                self.unknown_call();
            }
            _ => self.unknown_call(),
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

    /// `typeof` releases the borrow of a borrowed heap value and pushes the
    /// type value.
    fn apply_typeof(&mut self) {
        self.pop_releasing();
        self.push_slot(Slot::Copy);
    }

    /// The `borrow` word: an owned value becomes a borrow of itself; an
    /// existing borrow moves (its liveness continues); a copy stays a copy.
    fn apply_borrow(&mut self, span: Span) {
        match self.stack.pop() {
            Some(Slot::Owned(temp)) => {
                let slot = self.create_borrow(BorrowKey::Temp(temp), span);
                self.push_slot(slot);
            }
            Some(slot @ Slot::Borrow(_)) => self.push_slot(slot),
            Some(slot @ (Slot::Copy | Slot::Pointer)) => self.push_slot(slot),
            Some(slot @ Slot::Unknown) => self.push_slot(slot),
            None => self.push_slot(Slot::Unknown),
        }
    }

    /// Pop two operands and push the binary result. `+` / `-` over a
    /// pointer is pointer arithmetic (PLAN 3.5.2): it needs an unsafe
    /// context and the result stays a pointer.
    fn apply_bin(&mut self, op: BinOp, span: Span) {
        let rhs = self.stack.get(self.stack.len().saturating_sub(1)).cloned();
        let lhs = self.stack.get(self.stack.len().saturating_sub(2)).cloned();
        let pointer_arith = matches!(op, BinOp::Plus | BinOp::Minus)
            && (matches!(lhs, Some(Slot::Pointer)) || matches!(rhs, Some(Slot::Pointer)));
        if pointer_arith {
            self.require_unsafe(span, "pointer arithmetic");
        }
        self.pop_n(2);
        if op == BinOp::Concat {
            let temp = self.fresh_temp();
            self.push_slot(Slot::Owned(temp));
        } else if pointer_arith {
            self.push_slot(Slot::Pointer);
        } else {
            self.push_slot(Slot::Copy);
        }
    }

    /// PLAN 3.5: `what` needs an unsafe context: an `unsafe ... end` block
    /// or an `unsafe function` body. E370 names both escapes.
    fn require_unsafe(&mut self, span: Span, what: &str) {
        if self.unsafe_depth == 0 {
            self.emit(
                Diagnostic::error(
                    CODE_UNSAFE_OUTSIDE,
                    format!(
                        "{what} requires an unsafe context: wrap it in 'unsafe ... end' \
                         or mark the enclosing function 'unsafe function'"
                    ),
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

    /// Stack manipulation words.
    fn check_stack_op(&mut self, op: StackOp, span: Span) {
        match op {
            StackOp::Dup => match self.stack.last().cloned() {
                // PLAN 3.3.1: an owned heap temporary is a non-copy value;
                // `dup` is restricted to copy types. Report it, then keep
                // the PLAN 3.1 modeling (the runtime clones the handle
                // unowned) so the rest of the walk sees the stack it would
                // see without the rule.
                Some(Slot::Owned(temp)) => {
                    self.emit_dup_non_copy(span);
                    let key = BorrowKey::Temp(temp);
                    if !self.is_live(&key) {
                        self.live.insert(key.clone(), self.scopes.len() - 1);
                    }
                    self.push_slot(Slot::Borrow(key));
                }
                // A borrow is always of a heap value (borrows of copy
                // values stay copies), so this is also a non-copy `dup`.
                // Push the borrow without re-registering it: the original
                // borrow is already live, and a second E375 here would
                // cascade.
                Some(Slot::Borrow(key)) => {
                    self.emit_dup_non_copy(span);
                    self.push_slot(Slot::Borrow(key));
                }
                Some(slot @ (Slot::Copy | Slot::Pointer)) => self.push_slot(slot),
                Some(slot @ Slot::Unknown) => self.push_slot(slot),
                None => self.push_slot(Slot::Unknown),
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
                if let Some(Slot::Owned(temp)) = self.pop_releasing() {
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
                        Slot::Owned(temp) => {
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
                        Slot::Borrow(key) => {
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
}

/// Check one parsed program and return the collected diagnostics (PLAN 3.0
/// session wiring). An empty batch means the program passed the check.
pub fn check_program(program: &Program, path: &str, error_limit: usize) -> DiagnosticBatch {
    let mut checker = Checker::new(path, error_limit);
    checker.check_program(program);
    checker.into_batch()
}
