//! Static checking pass (PLAN Stage 3).
//!
//! The session pipeline is tokenize -> parse -> check -> interpret. This
//! module owns the check step: it walks the [`Program`] AST with access to
//! the declarations registered from that program (structs, enums, unions,
//! error types, functions) and collects diagnostics into a
//! [`DiagnosticBatch`].
//!
//! Stage 3.0 delivers the skeleton: declaration collection, a full AST walk,
//! and the [`check_program`] entry point wired into
//! [`crate::session::Session::interpret_source`]. The rule groups from PLAN
//! 3.1-3.5 (borrow, move, copy, static initializers, unsafe boundary) hook
//! into the visitor methods below; until they land, the walk emits no
//! diagnostics.
//!
//! Declarations from `require`d modules are not visible here: module loading
//! is an interpreter-side step that runs after the check. Rules that need
//! cross-module knowledge extend this in later tasks.

use std::collections::HashMap;

use crate::diagnostics::{Diagnostic, DiagnosticBatch, Span};
use crate::parser::ast::{
    EnumDecl, ErrorDecl, Expr, Function, MatchCaseKind, Program, Stmt, StmtKind, StructDecl,
    UnionDecl,
};

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
}

impl Checker {
    pub fn new(path: impl Into<String>, error_limit: usize) -> Self {
        Self {
            path: path.into(),
            batch: DiagnosticBatch::with_limit(error_limit),
            decls: Declarations::default(),
            unsafe_depth: 0,
            current_span: Span::default(),
        }
    }

    /// Register every declaration, then walk all bodies.
    pub fn check_program(&mut self, program: &Program) {
        self.register_decls(program);
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
            },
        );
        for stmt in &f.body {
            if let StmtKind::Function(nested) = &stmt.kind {
                let nested_fq = format!("{fq}::{}", nested.name);
                self.register_function(&nested_fq, nested, false);
            }
        }
    }

    fn check_body(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.check_stmt(stmt);
        }
    }

    /// Check one function body. An `unsafe function` body is an unsafe
    /// context for everything it contains.
    fn check_function_body(&mut self, f: &Function) {
        if f.is_unsafe {
            self.unsafe_depth += 1;
        }
        self.check_body(&f.body);
        if f.is_unsafe {
            self.unsafe_depth -= 1;
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        self.current_span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(expr) => self.check_expr(expr, stmt.span),
            StmtKind::VarDecl { value, .. } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
            }
            StmtKind::Set { target, value } => {
                self.check_expr(target, stmt.span);
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
            }
            StmtKind::Function(f) => self.check_function_body(f),
            StmtKind::Implement(imp) => {
                for f in &imp.functions {
                    self.check_function_body(f);
                }
            }
            StmtKind::Struct(_) | StmtKind::Enum(_) | StmtKind::Union(_) | StmtKind::Error(_) => {}
            StmtKind::Require { .. } => {}
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.check_expr(condition, stmt.span);
                self.check_body(then_branch);
                self.check_body(else_branch);
            }
            StmtKind::For { source, body } => {
                self.check_expr(source, stmt.span);
                self.check_body(body);
            }
            StmtKind::Match {
                value,
                cases,
                else_branch,
            } => {
                self.check_expr(value, stmt.span);
                for case in cases {
                    if let MatchCaseKind::Condition(condition) = &case.kind {
                        self.check_expr(condition, case.span);
                    }
                    self.check_body(&case.body);
                }
                self.check_body(else_branch);
            }
            StmtKind::Defer { body } => self.check_body(body),
            StmtKind::Handle { body, fallback } => {
                self.check_body(body);
                if let Some(fallback) = fallback {
                    self.check_expr(fallback, stmt.span);
                }
            }
            StmtKind::Move { source, .. } => self.check_expr(source, stmt.span),
            StmtKind::Fallback { value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
            }
            StmtKind::Return { value } => {
                if let Some(value) = value {
                    self.check_expr(value, stmt.span);
                }
            }
            StmtKind::Unsafe { body } => {
                self.unsafe_depth += 1;
                self.check_body(body);
                self.unsafe_depth -= 1;
            }
        }
    }

    /// Check one expression. `span` locates the expression for diagnostics:
    /// the enclosing statement's span, except `Seq` elements which carry
    /// their own.
    fn check_expr(&mut self, expr: &Expr, span: Span) {
        self.current_span = span;
        match expr {
            Expr::Integer { .. }
            | Expr::Float { .. }
            | Expr::String { .. }
            | Expr::Rune { .. }
            | Expr::Bool { .. }
            | Expr::Variable { .. }
            | Expr::TypeValue { .. }
            | Expr::ApplyBin(_)
            | Expr::ApplyUn(_)
            | Expr::ApplyTypeof
            | Expr::ApplyBorrow
            | Expr::ApplyLoad
            | Expr::Builtin { .. }
            | Expr::StackOp(_)
            | Expr::EmptyMapOrStruct => {}
            Expr::Member { base, .. } => self.check_expr(base, span),
            Expr::Binary { left, right, .. } => {
                self.check_expr(left, span);
                self.check_expr(right, span);
            }
            Expr::Unary { operand, .. }
            | Expr::Call { target: operand }
            | Expr::Unwrap { inner: operand }
            | Expr::Typeof { inner: operand }
            | Expr::Borrow { inner: operand }
            | Expr::Load { inner: operand } => self.check_expr(operand, span),
            Expr::Store { addr, value } => {
                self.check_expr(addr, span);
                self.check_expr(value, span);
            }
            Expr::Array(elements) | Expr::List(elements) => {
                for element in elements {
                    self.check_expr(element, span);
                }
            }
            Expr::Map(entries) => {
                for (key, value) in entries {
                    self.check_expr(key, span);
                    self.check_expr(value, span);
                }
            }
            Expr::StructLit(fields) => {
                for (_, value) in fields {
                    self.check_expr(value, span);
                }
            }
            Expr::Seq(elements) => {
                for (element, element_span) in elements {
                    self.check_expr(element, *element_span);
                }
            }
        }
    }
}

/// Check one parsed program and return the collected diagnostics (PLAN 3.0
/// session wiring). An empty batch means the program passed the check.
pub fn check_program(program: &Program, path: &str, error_limit: usize) -> DiagnosticBatch {
    let mut checker = Checker::new(path, error_limit);
    checker.check_program(program);
    checker.into_batch()
}
