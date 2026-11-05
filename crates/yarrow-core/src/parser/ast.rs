//! Abstract syntax tree for the Yarrow language.
//!
//! Yarrow is a stack-based language: values are pushed onto a stack and
//! operators/words pop operands. Because there are no newline tokens, the
//! parser works on an *operand stack* and emits one statement per region of
//! the stack. See the parser docs for details.

use crate::diagnostics::Span;
use crate::tokenizer::token::Location;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Stmt>,
}

impl Program {
    /// Top-level function named `name`, if present (default entry is `"main"`).
    pub fn entry_function(&self, name: &str) -> Option<(&Function, Span)> {
        self.items.iter().find_map(|item| match &item.kind {
            StmtKind::Function(f) if f.name == name => Some((f, item.span)),
            _ => None,
        })
    }

    /// Whether this program declares a top-level function named `name`.
    pub fn has_entry(&self, name: &str) -> bool {
        self.entry_function(name).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    Mutable,
    Const,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

/// Modifier on a function parameter type (`type copy` / `type mutable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamModifier {
    Copy,
    Mutable,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Named(String),
    Primitive(Primitive),
    Array {
        element: Box<Type>,
        size: Option<u64>,
    },
    List {
        element: Box<Type>,
    },
    Hashmap {
        key: Box<Type>,
        value: Box<Type>,
    },
    Reference {
        inner: Box<Type>,
    },
    Pointer {
        inner: Box<Type>,
    },
    /// `|T U …|` anonymous union (also used for fallible `|T Err|` returns).
    Union(Vec<Type>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F16,
    F32,
    F64,
    String,
    Rune,
    Bool,
    Void,
    /// Legacy catch-all error tag; prefer named `error` declarations.
    Error,
    Type,
}

impl Primitive {
    pub fn parse_name(name: &str) -> Option<Self> {
        Some(match name {
            "i8" => Primitive::I8,
            "i16" => Primitive::I16,
            "i32" => Primitive::I32,
            "i64" => Primitive::I64,
            "u8" => Primitive::U8,
            "u16" => Primitive::U16,
            "u32" => Primitive::U32,
            "u64" => Primitive::U64,
            "f16" => Primitive::F16,
            "f32" => Primitive::F32,
            "f64" => Primitive::F64,
            "string" => Primitive::String,
            "rune" => Primitive::Rune,
            "bool" => Primitive::Bool,
            "void" => Primitive::Void,
            "error" => Primitive::Error,
            "type" => Primitive::Type,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub kind: TypeKind,
    pub location: Location,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Plus,
    Minus,
    Mul,
    Div,
    Fdiv,
    Mod,
    Pow,
    /// String concatenation (`~`); not overloaded `+`.
    Concat,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    And,
    Or,
    Xor,
    Lshift,
    Rshift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
}

/// Operators that manipulate the runtime stack directly. Used when the
/// operand stack does not contain enough local values to apply the operation
/// at parse time (e.g. `dup` in a function body whose parameters arrive at
/// runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackOp {
    Dup,
    Swap,
    Rot,
    Unrot,
    Pop,
    Drop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Integer {
        value: String,
    },
    Float {
        value: String,
    },
    String {
        value: String,
    },
    Rune {
        value: String,
    },
    Bool {
        value: bool,
    },
    Variable {
        name: String,
    },
    Member {
        base: Box<Expr>,
        member: String,
    },
    /// A type used as a value (e.g. the `i32` in `myVar typeof i32 ==`).
    TypeValue {
        name: String,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Call {
        target: Box<Expr>,
    },
    Unwrap {
        inner: Box<Expr>,
    },
    /// `value typeof`: pops the value and pushes its static type.
    Typeof {
        inner: Box<Expr>,
    },
    /// `value borrow`: pushes a borrow reference to the value.
    Borrow {
        inner: Box<Expr>,
    },
    /// `pointer<T> load`: reads the pointee from the address.
    Load {
        inner: Box<Expr>,
    },
    /// `pointer<T> value store`: writes the value at the address.
    Store {
        addr: Box<Expr>,
        value: Box<Expr>,
    },
    /// A binary operator whose operands come from the runtime stack.
    ApplyBin(BinOp),
    /// An unary operator whose operand comes from the runtime stack.
    ApplyUn(UnOp),
    /// `typeof` applied to the top of the runtime stack.
    ApplyTypeof,
    /// `borrow` applied to the top of the runtime stack.
    ApplyBorrow,
    /// `load` applied to the top of the runtime stack.
    ApplyLoad,
    /// `@name`: a raw builtin word (`@alloc`, `@list_push`, ...).
    Builtin {
        name: String,
    },
    StackOp(StackOp),
    Array(Vec<Expr>),
    List(Vec<Expr>),
    /// `{ k v ... }` hashmap literal: keys are literals (see grammar).
    Map(Vec<(Expr, Expr)>),
    /// `{ field value ... }` struct literal: keys are identifiers.
    StructLit(Vec<(String, Expr)>),
    /// `{}` empty map/struct placeholder; needs a typed binding context.
    EmptyMapOrStruct,
    /// Multiple stack words flushed as one expression (conditions / initializers).
    /// Each element keeps its span so lowering can attribute errors per word.
    /// Elements are flat (no nested `Seq`).
    Seq(Vec<(Expr, Span)>),
}

impl Expr {
    pub fn variable(name: impl Into<String>) -> Expr {
        Expr::Variable { name: name.into() }
    }

    /// Span of a leaf expression when known; `Seq` uses the first element's span.
    pub fn span_hint(&self) -> Option<Span> {
        match self {
            Expr::Seq(elems) => elems.first().map(|(_, s)| *s),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub ty: Type,
    pub modifier: Option<ParamModifier>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub visibility: Option<Visibility>,
    pub params: Vec<Parameter>,
    pub body: Vec<Stmt>,
    /// Return types from `with …`. A `|T Err|` literal is one `TypeKind::Union`.
    pub returns: Vec<Type>,
    /// `name unsafe function`: marks the function as unsafe, so its body may
    /// use unsafe operations and calls to it require an unsafe context.
    pub is_unsafe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub visibility: Option<Visibility>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Implement {
    pub target: String,
    pub functions: Vec<Function>,
}

/// A single enum member: its name and optional explicit value (the raw integer
/// lexeme, kept lossless like other literals). Without a value, the member gets
/// the next implicit ordinal starting at 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumMember {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    /// Optional underlying type (`Name i32 enum`); default is `i32`.
    pub underlying: Option<Type>,
    pub members: Vec<EnumMember>,
    /// `Name public enum`: nameable by importers (PLAN 4.1.1). Default
    /// visibility is private.
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionDecl {
    pub name: String,
    pub types: Vec<Type>,
    /// `Name public union`: nameable by importers (PLAN 4.1.1). Default
    /// visibility is private.
    pub visibility: Option<Visibility>,
}

/// `Name [QualifiedName] error { member } end`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDecl {
    pub name: String,
    /// Optional injected error type (`MyErr error.Error error … end`).
    pub inject: Option<String>,
    pub members: Vec<String>,
    /// `Name public error`: nameable by importers (PLAN 4.1.1). Default
    /// visibility is private.
    pub visibility: Option<Visibility>,
}

/// The condition of a `match` case. Either a boolean expression evaluated
/// against the subject (value match) or a member type the union subject is
/// dispatched on (type match).
#[derive(Debug, Clone, PartialEq)]
pub enum MatchCaseKind {
    Condition(Expr),
    Type(Type),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchCase {
    pub kind: MatchCaseKind,
    pub body: Vec<Stmt>,
    /// Span covering the case condition/type through its closing `end`.
    pub span: Span,
}

/// A statement with its source span (keyword / name through the construct).
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Expr(Expr),
    VarDecl {
        name: String,
        mutability: Mutability,
        ty: Type,
        value: Option<Expr>,
    },
    Set {
        target: Expr,
        value: Option<Expr>,
    },
    Function(Function),
    Struct(StructDecl),
    Implement(Implement),
    Enum(EnumDecl),
    Union(UnionDecl),
    Require {
        path: String,
        alias: Option<String>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    /// `for` forms (docs): condition (`i 3 < for`) or iterable (`numbers for`).
    /// Iteration value/index come from `std.loop`, not binder names before `for`.
    For {
        source: Expr,
        body: Vec<Stmt>,
    },
    Match {
        value: Expr,
        cases: Vec<MatchCase>,
        else_branch: Vec<Stmt>,
    },
    Defer {
        body: Vec<Stmt>,
    },
    /// Error-handling block: `expression handle <body> end`. The body may
    /// contain a `fallback` statement, which is extracted into `fallback`.
    Handle {
        body: Vec<Stmt>,
        fallback: Option<Expr>,
    },
    /// `source target move`: transfers ownership of `source` to `target`.
    Move {
        target: String,
        source: Expr,
    },
    /// `value fallback`: the value pushed on the stack if a `handle` catches an
    /// error. Only meaningful inside a `handle ... end` block.
    Fallback {
        value: Option<Expr>,
    },
    Return {
        value: Option<Expr>,
    },
    Error(ErrorDecl),
    /// `unsafe ... end`: an unsafe block. Statements inside may perform
    /// operations that are normally restricted (pointer access, host calls).
    Unsafe {
        body: Vec<Stmt>,
    },
}
