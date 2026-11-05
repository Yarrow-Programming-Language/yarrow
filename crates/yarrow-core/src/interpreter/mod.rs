//! AST interpreter for checked Yarrow programs (Stages 13b / 21 / 32 / 36 / 37).
//!
//! Design choice: **tree-walk** the checked AST with an explicit operand stack,
//! calling into [`crate::runtime`] for heap strings, lists, maps, structs,
//! unions, regions, and raw memory. A stack bytecode VM can replace this later
//! without changing the Session surface (`interpret_source` / [`EvalContext`]).
//!
//! ## Corpus coverage
//!
//! Interprets cleanly (stdout matches JIT `run --target jit`):
//! - `docs/examples/valid/00_grammar_tour.yar`
//! - `docs/examples/valid/01_hello.yar`
//! - `docs/examples/valid/02_arithmetic_and_stack.yar`
//! - `docs/examples/valid/03_variables_and_typeof.yar`
//! - `docs/examples/valid/04_functions.yar`
//! - `docs/examples/valid/05_control_flow.yar`
//! - `docs/examples/valid/06_structs_and_enums.yar`
//! - `docs/examples/valid/07_unions.yar`
//! - `docs/examples/valid/08_ownership_borrow_move.yar`
//! - `docs/examples/valid/09_regions_and_defer.yar`
//! - `docs/examples/valid/10_errors.yar`
//! - `docs/examples/valid/11_unsafe_pointers.yar`
//! - `docs/examples/valid/12_modules.yar`
//! - `docs/examples/valid/13_containers.yar`
//! - `docs/examples/valid/14_io_and_string.yar`
//! - `docs/examples/valid/15_fs.yar`
//! - `docs/examples/valid/16_list_iteration.yar`
//! - `docs/examples/valid/17_empty_placeholders.yar`
//! - `docs/examples/valid/18_complex_map_keys.yar`
//! - `docs/examples/valid/19_array_in_list.yar`
//! - `docs/examples/valid/20_warning_unused.yar`
//! - `docs/examples/valid/21_warning_never_written.yar`
//! - `docs/examples/valid/22_warning_unreachable.yar`
//! - `docs/examples/valid/23_warning_require_ambiguous.yar`
//! - `docs/examples/valid/24_warning_empty_match.yar`
//! - `docs/examples/valid/25_warning_empty_if_unsafe.yar`
//!
//! Supported surface: Stage 36 plus `unsafe` blocks, `pointer<T>` load/store
//! and field access/`set`, `std.mem` (`@alloc`/`@free`), `move`, runes, and
//! `std.fs` host helpers.
//!
//! Maintenance: re-check this list whenever an example is added to
//! `docs/examples/valid/` or a spec gap is closed; every file in that
//! directory must appear here and run cleanly.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::diagnostics::Span;
use crate::modules::{ModuleLoader, RequiredModule};
use crate::parser::ast::{
    BinOp, EnumDecl, ErrorDecl, Expr, Function, Implement, MatchCase, MatchCaseKind, Mutability,
    ParamModifier, Primitive, Program, StackOp, Stmt, StmtKind, StructDecl, Type, TypeKind, UnOp,
    UnionDecl, Visibility,
};
use crate::parser::literals::{
    decode_float_literal, decode_int_literal, decode_rune_literal, decode_string_literal,
};
use crate::parser::parse;
use crate::result::RunResult;
use crate::runtime::{
    self, FieldDesc, KIND_ENUM, KIND_LIST, KIND_MAP, KIND_STRING, KIND_STRUCT, KIND_UNION,
    UNION_PAYLOAD_OFFSET, UNION_TAG_OFFSET, free_value,
};
use crate::signature::{Signature, classify_signature};
use crate::tokenizer::Tokenizer;

/// Kind tag used for fallible error envelopes on the interpreter stack.
const KIND_ERROR: u64 = 0x51;
/// Raw `pointer<T>` tag; pointee kind code lives in the high bits (`>> 8`).
const KIND_PTR: u64 = 0x50;

/// Runtime value on the interpreter operand stack / in a local.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Heap string handle from [`runtime::yarrow_str_new`].
    Str {
        handle: u64,
        owned: bool,
    },
    /// Fixed array (Stage 21 iterable `for`).
    Array(Vec<Value>),
    /// Host heap handle: struct, union, list, or hashmap (`Slot.kind` selects).
    Heap {
        handle: u64,
        owned: bool,
    },
}

impl Value {
    fn drop_owned(self) {
        match self {
            Value::Str {
                handle,
                owned: true,
            } => free_value(handle, KIND_STRING),
            Value::Heap { .. } => {
                // Owned heaps are freed through [`Slot::drop_owned`] (needs kind).
            }
            Value::Array(elems) => {
                for e in elems {
                    e.drop_owned();
                }
            }
            _ => {}
        }
    }

    fn clone_for_stack(&self) -> Value {
        match self {
            Value::Str { handle, .. } => Value::Str {
                handle: *handle,
                owned: false,
            },
            Value::Heap { handle, .. } => Value::Heap {
                handle: *handle,
                owned: false,
            },
            Value::Array(elems) => {
                Value::Array(elems.iter().map(|e| e.clone_for_stack()).collect())
            }
            other => other.clone(),
        }
    }

    fn as_bits(&self) -> Option<u64> {
        match self {
            Value::Int(n) => Some(*n as u64),
            Value::Bool(b) => Some(u64::from(*b)),
            Value::Float(f) => Some(f.to_bits()),
            Value::Str { handle, .. } | Value::Heap { handle, .. } => Some(*handle),
            Value::Array(_) => None,
        }
    }
}

/// Operand-stack slot: value plus the runtime kind code used by `typeof`.
#[derive(Debug, Clone)]
struct Slot {
    value: Value,
    kind: u64,
}

impl Slot {
    fn drop_owned(self) {
        match self.value {
            Value::Str {
                handle,
                owned: true,
            } => free_value(handle, KIND_STRING),
            Value::Heap {
                handle,
                owned: true,
            } => free_value(handle, self.kind),
            Value::Array(elems) => {
                for e in elems {
                    e.drop_owned();
                }
            }
            other => other.drop_owned(),
        }
    }
}

/// Error while interpreting an already-checked program.
#[derive(Debug, Clone)]
pub struct InterpretError {
    pub message: String,
    pub code: String,
    pub span: Span,
}

impl InterpretError {
    fn new(message: impl Into<String>, span: Span, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: code.into(),
            span,
        }
    }

    fn unsupported(what: impl Into<String>, span: Span) -> Self {
        Self::new(
            format!("interpreter does not support {} yet", what.into()),
            span,
            "E393",
        )
    }
}

type IResult<T> = Result<T, InterpretError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Next,
    Return,
    Break,
    Continue,
}

/// A `require` statement inside a function body: its alias or plain names
/// bind only for that function (PLAN 2.6).
#[derive(Debug, Clone)]
struct LocalRequire {
    /// Alias scope name, when the require names one.
    alias: Option<String>,
    /// Module path (dotted, e.g. "std.io").
    path: String,
    /// Single item name for item imports ("std.math.sqrt").
    item: Option<String>,
}

/// One registered function body (top-level, nested, method, or from a module).
#[derive(Debug, Clone)]
struct FuncEntry {
    /// Fully-qualified name (`demo::add`, `std.io::write_line`, `Point::distance`).
    fq: String,
    module: Option<String>,
    function: Function,
    /// True when registered from an `implement` block (binds `self`).
    is_method: bool,
    /// `require` statements of this function body, resolved at registration.
    local_requires: Vec<LocalRequire>,
}

#[derive(Debug, Clone)]
struct FieldInfo {
    name: String,
    /// Runtime kind of the field value.
    kind: u64,
    offset: i32,
    size: u32,
    /// True when the field is declared `public` (PLAN 5.4). Default
    /// visibility is private.
    public: bool,
}

#[derive(Debug, Clone)]
struct StructInfo {
    name: String,
    fields: Vec<FieldInfo>,
    size: u32,
    /// Module path of the unit that declared the struct; `None` is the main
    /// unit (PLAN 5.4). Backs the field-visibility gate.
    module: Option<String>,
}

#[derive(Debug, Clone)]
struct UnionInfo {
    name: String,
    /// Member type names / primitive tags in declaration order.
    members: Vec<MemberTy>,
}

#[derive(Debug, Clone)]
struct EnumInfo {
    name: String,
    /// Runtime kind code of the underlying carrier (default `i32`).
    carrier: u64,
    /// String-carrier member table: member name -> interned heap string
    /// handle (PLAN 3.2). `None` for integer carriers, whose discriminants
    /// live in `enum_members`.
    strings: Option<Vec<(String, u64)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberTy {
    Primitive(u64),
    Named(String),
    String,
}

struct LoopCtx {
    value: Option<Slot>,
    index: Option<i64>,
}

struct Frame {
    locals: HashMap<String, Slot>,
    /// Declared mutability per local (PLAN 5.1.2): `set` is restricted to
    /// `mutable` bindings. Names bound outside a declaration (`self` at
    /// call entry, `move` targets) default to `mutable`.
    local_mutability: HashMap<String, Mutability>,
    loops: Vec<LoopCtx>,
    /// `defer` bodies, one entry per active scope (innermost last). Each
    /// scope runs its registered bodies in reverse order at its exit; the
    /// function body is the outermost scope.
    deferred: Vec<Vec<Vec<Stmt>>>,
    /// Payload type of this frame's fallible `|T Err|` return, when present.
    error_payload: Option<MemberTy>,
    /// Set by `unwrap` when propagating an error out of a fallible function.
    pending_error_return: Option<i64>,
    /// Set by `loop.break` / `loop.continue`; consumed by the innermost `for`.
    pending_flow: Option<Flow>,
    /// Function-scoped `require` bindings (PLAN 2.6): alias → module path,
    /// alias → item for item imports, bare name → fq for plain imports.
    local_aliases: HashMap<String, String>,
    local_item_aliases: HashMap<String, String>,
    local_plain_funcs: HashMap<String, String>,
}

/// Stack-machine interpreter over a checked AST.
pub struct Interpreter {
    loader: ModuleLoader,
    /// Fully-qualified name → entry.
    funcs: HashMap<String, FuncEntry>,
    /// Module alias → module path (`io` → `std.io`).
    aliases: HashMap<String, String>,
    /// Bare name → fq name for alias-less requires.
    plain_funcs: HashMap<String, String>,
    /// Alias → single exported item for item imports under a scope.
    item_aliases: HashMap<String, String>,
    /// Top-level `require` statements in load order (PLAN 2.6): only these
    /// bind globally; function-body requires bind per function entry.
    top_requires: Vec<(String, Option<String>)>,
    #[allow(dead_code)]
    public_funcs: std::collections::HashSet<String>,
    modules: Vec<RequiredModule>,
    struct_ids: HashMap<String, u32>,
    structs: Vec<StructInfo>,
    enum_ids: HashMap<String, u32>,
    /// `(enum_name, member_name)` → ordinal value.
    enum_members: HashMap<(String, String), i64>,
    /// Enum id → declaration metadata (name, carrier kind).
    enums: Vec<EnumInfo>,
    union_ids: HashMap<String, u32>,
    unions: Vec<UnionInfo>,
    error_type_ids: HashMap<String, u32>,
    /// Error type id → `(member_name, tag)`.
    error_types: Vec<(String, Vec<(String, u32)>)>,
    /// Interned error tags, keyed by member name so a tag is program-unique:
    /// `MyErr.OUT_OF_MEMORY` and `error.OUT_OF_MEMORY` share one tag.
    error_tags: HashMap<String, u32>,
    /// Error type id + inject path awaiting resolution. Module units register
    /// after the main unit, so an inject that targets a module type (e.g.
    /// `error.Error` from std.error) is resolved in `resolve_error_injects`.
    pending_error_injects: Vec<(u32, String)>,
    /// Declaring unit per registered type, keyed by category and bare name.
    /// Backs the `E384` duplicate diagnostic (PLAN 2.2).
    type_units: HashMap<(TypeCategory, String), String>,
}

/// Which registry a type name was declared into. Types register globally by
/// bare name, so a second declaration in the same category is a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TypeCategory {
    Struct,
    Enum,
    Union,
    Error,
}

/// REPL-oriented wrapper around [`Interpreter`] (Stage 13b surface).
pub struct EvalContext {
    interp: Interpreter,
}

impl EvalContext {
    pub fn new() -> Self {
        Self {
            interp: Interpreter::new(),
        }
    }

    pub fn add_module_search_path(&mut self, path: impl Into<PathBuf>) {
        self.interp.add_module_search_path(path);
    }

    /// Load a checked program (registers `require`s and functions). Does not run `main`.
    pub fn load_program(&mut self, program: &Program) -> IResult<()> {
        self.interp.load_program(program)
    }

    /// Execute `main` and return a driver-displayable result.
    pub fn run_main(&mut self) -> IResult<RunResult> {
        self.run_entry(crate::DEFAULT_ENTRY_NAME)
    }

    /// Execute the named top-level entry and return a driver-displayable result.
    pub fn run_entry(&mut self, name: &str) -> IResult<RunResult> {
        self.interp.run_entry(name)
    }
}

impl Default for EvalContext {
    fn default() -> Self {
        Self::new()
    }
}
impl Interpreter {
    pub fn new() -> Self {
        Self {
            loader: ModuleLoader::new(),
            funcs: HashMap::new(),
            aliases: HashMap::new(),
            plain_funcs: HashMap::new(),
            item_aliases: HashMap::new(),
            top_requires: Vec::new(),
            public_funcs: std::collections::HashSet::new(),
            modules: Vec::new(),
            struct_ids: HashMap::new(),
            structs: Vec::new(),
            enum_ids: HashMap::new(),
            enum_members: HashMap::new(),
            enums: Vec::new(),
            union_ids: HashMap::new(),
            unions: Vec::new(),
            error_type_ids: HashMap::new(),
            error_types: Vec::new(),
            error_tags: HashMap::new(),
            pending_error_injects: Vec::new(),
            type_units: HashMap::new(),
        }
    }

    pub fn add_module_search_path(&mut self, path: impl Into<PathBuf>) {
        self.loader.add_search_path(path);
    }

    pub fn load_program(&mut self, program: &Program) -> IResult<()> {
        self.funcs.clear();
        self.aliases.clear();
        self.plain_funcs.clear();
        self.item_aliases.clear();
        self.top_requires.clear();
        self.public_funcs.clear();
        self.modules.clear();
        self.struct_ids.clear();
        self.structs.clear();
        self.enum_ids.clear();
        self.enum_members.clear();
        // PLAN 3.2: the registry owns the interned string-carrier member
        // strings; release them before dropping the enum table.
        for e in &self.enums {
            if let Some(strings) = &e.strings {
                for (_, handle) in strings {
                    free_value(*handle, KIND_STRING);
                }
            }
        }
        self.enums.clear();
        self.union_ids.clear();
        self.unions.clear();
        self.error_type_ids.clear();
        self.error_types.clear();
        self.error_tags.clear();
        self.pending_error_injects.clear();
        self.type_units.clear();

        let mut loaded = Vec::new();
        let mut loading = HashSet::new();
        self.load_requires(program, &mut loaded, &mut loading)?;
        self.modules = loaded;

        self.register_unit(None, program)?;
        for m in &self.modules.clone() {
            self.register_unit(Some(m.path.clone()), &m.program)?;
        }
        self.resolve_error_injects()?;
        self.register_module_bindings()?;
        self.validate_error_signatures()?;
        Ok(())
    }

    pub fn run_main(&mut self) -> IResult<RunResult> {
        self.run_entry(crate::DEFAULT_ENTRY_NAME)
    }

    pub fn run_entry(&mut self, name: &str) -> IResult<RunResult> {
        let entry = self.funcs.get(name).cloned().ok_or_else(|| {
            InterpretError::new(
                format!("program has no '{name}' function"),
                Span::default(),
                "E360",
            )
        })?;
        let fallible = self.fallible_payload(
            &entry.function.returns,
            &require_aliases(&entry.local_requires),
        );
        let mut stack = Vec::new();
        self.call_function(&entry, &mut stack)?;
        if fallible.is_some() {
            // Fallible entry: `(payload, env)` on the stack. Non-zero env is a
            // runtime failure; success is void (matching JIT `run_main`).
            if stack.len() < 2 {
                return Err(InterpretError::new(
                    format!(
                        "{name} fallible entry left {} value(s); expected 2",
                        stack.len()
                    ),
                    Span::default(),
                    "E393",
                ));
            }
            let env = stack.pop().unwrap();
            let payload = stack.pop().unwrap();
            while let Some(v) = stack.pop() {
                v.drop_owned();
            }
            let tag = match env.value {
                Value::Int(n) => n,
                other => {
                    other.drop_owned();
                    payload.drop_owned();
                    return Err(InterpretError::new(
                        format!("{name} fallible envelope must be an int tag"),
                        Span::default(),
                        "E360",
                    ));
                }
            };
            payload.drop_owned();
            if tag != 0 {
                return Err(InterpretError::new(
                    format!("{name} returned error tag {tag}"),
                    Span::default(),
                    "E360",
                ));
            }
            return Ok(RunResult::Void);
        }
        if entry.function.returns.is_empty() {
            while let Some(v) = stack.pop() {
                v.drop_owned();
            }
            return Ok(RunResult::Void);
        }
        if stack.len() != 1 {
            return Err(InterpretError::new(
                format!(
                    "{name} left {} value(s) on the stack; expected 1",
                    stack.len()
                ),
                Span::default(),
                "E393",
            ));
        }
        Ok(match stack.pop().unwrap().value {
            Value::Int(n) => RunResult::Int(n),
            Value::Float(f) => RunResult::Float(f),
            Value::Bool(b) => RunResult::Bool(b),
            Value::Str { handle, owned } => {
                let bytes = runtime::string_bytes(handle).unwrap_or_default();
                let s = String::from_utf8_lossy(&bytes).into_owned();
                if owned {
                    free_value(handle, KIND_STRING);
                }
                RunResult::Str(s)
            }
            Value::Array(_) => {
                return Err(InterpretError::unsupported(
                    "array as entry return value",
                    Span::default(),
                ));
            }
            Value::Heap { .. } => {
                return Err(InterpretError::unsupported(
                    "heap handle as entry return value",
                    Span::default(),
                ));
            }
        })
    }

    fn register_unit(&mut self, module: Option<String>, program: &Program) -> IResult<()> {
        let unit = Self::unit_label(module.as_deref());
        for item in &program.items {
            match &item.kind {
                StmtKind::Struct(d) => self.register_struct(module.as_deref(), d)?,
                StmtKind::Enum(d) => self.register_enum(&unit, d)?,
                StmtKind::Union(d) => self.register_union(&unit, d)?,
                StmtKind::Error(d) => self.register_error(&unit, d)?,
                StmtKind::Implement(imp) => self.register_implement(module.clone(), imp)?,
                StmtKind::Function(f) => {
                    // Every function of a module unit registers under its
                    // qualified name so private helpers are callable inside
                    // their own module (PLAN 2.1.1). The cross-unit gate in
                    // `eval_call` keeps them file-local.
                    let fq = match &module {
                        Some(path) => format!("{path}::{}", f.name),
                        None => f.name.clone(),
                    };
                    self.register_function_tree(module.clone(), fq, f, false)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Human-readable name of the unit being registered, for diagnostics.
    fn unit_label(module: Option<&str>) -> String {
        match module {
            Some(path) => format!("module '{path}'"),
            None => "the main unit".to_string(),
        }
    }

    /// Reject a second declaration of a type name in the same category
    /// (PLAN 2.2). Types register globally by bare name, so the later unit
    /// is always the one blamed; registration order (main unit first, then
    /// modules in require order) guarantees that.
    fn check_type_duplicate(
        &mut self,
        category: TypeCategory,
        name: &str,
        unit: &str,
    ) -> IResult<()> {
        if let Some(first) = self.type_units.get(&(category, name.to_string())) {
            return Err(InterpretError::new(
                format!("duplicate type '{name}': declared by {first}, redeclared by {unit}"),
                Span::default(),
                "E384",
            ));
        }
        self.type_units
            .insert((category, name.to_string()), unit.to_string());
        Ok(())
    }

    fn register_struct(&mut self, module: Option<&str>, d: &StructDecl) -> IResult<()> {
        let unit = Self::unit_label(module);
        self.check_type_duplicate(TypeCategory::Struct, &d.name, &unit)?;
        let id = self.structs.len() as u32;
        self.struct_ids.insert(d.name.clone(), id);
        let mut fields = Vec::new();
        let mut size = 0u32;
        let mut align = 1u32;
        for f in &d.fields {
            let (kind, fsize, falign) = self.field_layout(&f.ty)?;
            align = align.max(falign);
            size = (size + (falign - 1)) & !(falign - 1);
            fields.push(FieldInfo {
                name: f.name.clone(),
                kind,
                offset: size as i32,
                size: fsize,
                public: matches!(f.visibility, Some(Visibility::Public)),
            });
            size += fsize;
        }
        size = (size + (align - 1)) & !(align - 1);
        if size == 0 {
            size = 1;
        }
        let descs: Vec<FieldDesc> = fields
            .iter()
            .map(|f| FieldDesc {
                offset: f.offset as u32,
                _pad: 0,
                kind: f.kind,
            })
            .collect();
        unsafe {
            runtime::yarrow_register_struct_descs(id, descs.as_ptr(), descs.len() as u64);
        }
        self.structs.push(StructInfo {
            name: d.name.clone(),
            fields,
            size,
            module: module.map(str::to_string),
        });
        Ok(())
    }

    fn field_layout(&self, ty: &Type) -> IResult<(u64, u32, u32)> {
        match &ty.kind {
            TypeKind::Primitive(p) => {
                let kind = primitive_to_kind(*p).ok_or_else(|| {
                    InterpretError::unsupported(format!("field type {ty:?}"), Span::default())
                })?;
                let (size, align) = match kind {
                    0 | 1 | 6 => (1, 1),
                    2 | 7 | 12 => (2, 2),
                    3 | 8 | 11 | 13 => (4, 4),
                    _ => (8, 8),
                };
                Ok((kind, size, align))
            }
            TypeKind::Named(n) if n == "string" => Ok((KIND_STRING, 8, 8)),
            TypeKind::Named(n) => {
                if let Some(id) = self.struct_ids.get(n) {
                    Ok((KIND_STRUCT | ((*id as u64) << 8), 8, 8))
                } else if let Some(id) = self.union_ids.get(n) {
                    Ok((KIND_UNION | ((*id as u64) << 8), 8, 8))
                } else if let Some(id) = self.enum_ids.get(n) {
                    // PLAN 3.1: the field kind identifies the enum itself so
                    // reads report the enum type; storage stays carrier-sized.
                    let carrier = self.enums.get(*id as usize).map_or(3, |e| e.carrier);
                    let (size, align) = match carrier {
                        1 | 6 => (1, 1),
                        2 | 7 => (2, 2),
                        3 | 8 => (4, 4),
                        _ => (8, 8),
                    };
                    Ok((KIND_ENUM | ((*id as u64) << 8), size, align))
                } else {
                    Err(InterpretError::unsupported(
                        format!("field type '{n}'"),
                        Span::default(),
                    ))
                }
            }
            TypeKind::Reference { inner } => self.field_layout(inner),
            TypeKind::Pointer { inner } => {
                let (pk, _, _) = self.field_layout(inner)?;
                Ok((KIND_PTR | (pk << 8), 8, 8))
            }
            TypeKind::List { element } => {
                let (ek, _, _) = self.field_layout(element)?;
                Ok((KIND_LIST | (ek << 8), 8, 8))
            }
            TypeKind::Hashmap { key, value } => {
                let (kk, _, _) = self.field_layout(key)?;
                let (vk, _, _) = self.field_layout(value)?;
                Ok((KIND_MAP | (kk << 8) | (vk << 40), 8, 8))
            }
            _ => Err(InterpretError::unsupported(
                format!("field type {ty:?}"),
                Span::default(),
            )),
        }
    }

    fn register_enum(&mut self, unit: &str, d: &EnumDecl) -> IResult<()> {
        self.check_type_duplicate(TypeCategory::Enum, &d.name, unit)?;
        // Carrier story: integer carriers keep discriminants of that width
        // (range-checked below); a string carrier stores one interned heap
        // string per member (PLAN 3.2). Any other carrier is rejected with
        // `E363` by `enum_carrier_kind`.
        let carrier = self.enum_carrier_kind(d)?;
        let id = self.enum_ids.len() as u32;
        self.enum_ids.insert(d.name.clone(), id);
        if carrier == KIND_STRING {
            let mut strings = Vec::with_capacity(d.members.len());
            for m in &d.members {
                if m.value.is_some() {
                    return Err(InterpretError::new(
                        format!(
                            "enum '{}' member '{}' cannot take an explicit value: \
                             a string carrier stores member names",
                            d.name, m.name
                        ),
                        Span::default(),
                        "E363",
                    ));
                }
                let handle = runtime::yarrow_str_new(m.name.as_ptr() as u64, m.name.len() as u64);
                strings.push((m.name.clone(), handle));
            }
            self.enums.push(EnumInfo {
                name: d.name.clone(),
                carrier,
                strings: Some(strings),
            });
        } else {
            let (lo, hi) = int_carrier_range(carrier)
                .expect("enum_carrier_kind only admits integer and string carriers");
            let mut next = 0i128;
            for m in &d.members {
                let v = if let Some(raw) = &m.value {
                    decode_int_literal(raw)
                        .map_err(|msg| InterpretError::new(msg, Span::default(), "E363"))?
                } else {
                    next
                };
                if v < lo || v > hi {
                    return Err(InterpretError::new(
                        format!(
                            "enum '{}' member '{}' value {v} does not fit its carrier",
                            d.name, m.name
                        ),
                        Span::default(),
                        "E363",
                    ));
                }
                next = v + 1;
                self.enum_members
                    .insert((d.name.clone(), m.name.clone()), v as i64);
            }
            self.enums.push(EnumInfo {
                name: d.name.clone(),
                carrier,
                strings: None,
            });
        }
        // The host runtime frees and clones enum-typed fields and elements
        // through the carrier kind (string handles vs inline scalar bits).
        unsafe {
            runtime::yarrow_register_enum_carrier(id, carrier);
        }
        Ok(())
    }

    /// Kind code of an enum's underlying carrier. `None` means the default
    /// `i32`. Integer primitives and `string` are the supported carriers
    /// (PLAN 3.2); anything else is rejected with `E363`.
    fn enum_carrier_kind(&self, d: &EnumDecl) -> IResult<u64> {
        let carrier = match &d.underlying {
            None => primitive_to_kind(Primitive::I32),
            Some(ty) => match &ty.kind {
                TypeKind::Primitive(p) => primitive_to_kind(*p),
                TypeKind::Named(n) => primitive_kind_code(n),
                _ => None,
            },
        };
        carrier
            .filter(|k| int_carrier_range(*k).is_some() || *k == KIND_STRING)
            .ok_or_else(|| {
                let what = match &d.underlying {
                    Some(ty) => match &ty.kind {
                        // Primitive variant names lowercase to their source
                        // names (`String` -> `string`, `F32` -> `f32`).
                        TypeKind::Primitive(p) => format!("{p:?}").to_lowercase(),
                        TypeKind::Named(n) => format!("'{n}'"),
                        _ => "unsupported".to_string(),
                    },
                    None => "'i32'".to_string(),
                };
                InterpretError::new(
                    format!(
                        "enum carrier {what} for '{}': only integer carriers \
                         and string are supported",
                        d.name
                    ),
                    Span::default(),
                    "E363",
                )
            })
    }

    fn register_union(&mut self, unit: &str, d: &UnionDecl) -> IResult<()> {
        self.check_type_duplicate(TypeCategory::Union, &d.name, unit)?;
        let id = self.unions.len() as u32;
        self.union_ids.insert(d.name.clone(), id);
        let mut members = Vec::new();
        for t in &d.types {
            members.push(self.member_ty(t)?);
        }
        // PLAN 5.3.1: union members must be distinct after the reference
        // stripping `member_ty` already applies.
        for (i, m) in members.iter().enumerate() {
            if members[..i].contains(m) {
                return Err(InterpretError::new(
                    format!(
                        "union '{}' declares the member type '{}' more than once",
                        d.name,
                        self.member_ty_label(m)
                    ),
                    Span::default(),
                    "E385",
                ));
            }
        }
        self.unions.push(UnionInfo {
            name: d.name.clone(),
            members,
        });
        Ok(())
    }

    fn member_ty(&self, t: &Type) -> IResult<MemberTy> {
        match &t.kind {
            TypeKind::Primitive(Primitive::String) => Ok(MemberTy::String),
            TypeKind::Named(n) if n == "string" => Ok(MemberTy::String),
            TypeKind::Primitive(p) => {
                let kind = primitive_to_kind(*p).ok_or_else(|| {
                    InterpretError::unsupported(format!("union member {t:?}"), Span::default())
                })?;
                Ok(MemberTy::Primitive(kind))
            }
            TypeKind::Named(n) => Ok(MemberTy::Named(n.clone())),
            TypeKind::Reference { inner } => self.member_ty(inner),
            _ => Err(InterpretError::unsupported(
                format!("union member {t:?}"),
                Span::default(),
            )),
        }
    }

    /// Human-readable name of a union member type, for the `E385` message.
    fn member_ty_label(&self, m: &MemberTy) -> String {
        match m {
            MemberTy::String => "string".to_string(),
            MemberTy::Named(n) => n.clone(),
            MemberTy::Primitive(kind) => match *kind & 0xff {
                0 => "bool".to_string(),
                1 => "i8".to_string(),
                2 => "i16".to_string(),
                3 => "i32".to_string(),
                4 => "i64".to_string(),
                6 => "u8".to_string(),
                7 => "u16".to_string(),
                8 => "u32".to_string(),
                9 => "u64".to_string(),
                11 => "rune".to_string(),
                12 => "f16".to_string(),
                13 => "f32".to_string(),
                14 => "f64".to_string(),
                _ => format!("kind {kind}"),
            },
        }
    }

    fn register_error(&mut self, unit: &str, d: &ErrorDecl) -> IResult<()> {
        self.check_type_duplicate(TypeCategory::Error, &d.name, unit)?;
        let id = self.error_types.len() as u32;
        self.error_type_ids.insert(d.name.clone(), id);
        let mut members = Vec::new();
        for m in &d.members {
            let tag = self.intern_error_tag(m);
            members.push((m.clone(), tag));
        }
        self.error_types.push((d.name.clone(), members));
        if let Some(inject) = &d.inject {
            // Tags are interned by member name, so injected and local members
            // that share a name already share a tag; the deferred pass only
            // adds the injected names missing from the member list.
            self.pending_error_injects.push((id, inject.clone()));
        }
        Ok(())
    }

    fn intern_error_tag(&mut self, member: &str) -> u32 {
        let next = self.error_tags.len() as u32 + 1;
        *self.error_tags.entry(member.to_string()).or_insert(next)
    }

    /// Resolve every deferred `inject` clause. Runs once, after all units are
    /// registered, because module units register after the main unit.
    fn resolve_error_injects(&mut self) -> IResult<()> {
        let pending = std::mem::take(&mut self.pending_error_injects);
        for (id, inject) in &pending {
            self.apply_error_inject(*id, inject, &pending, &mut Vec::new())?;
        }
        Ok(())
    }

    /// Copy the members of the injected error type into type `id`. The
    /// target's own pending inject resolves first so chains propagate;
    /// `visiting` guards against inject cycles.
    fn apply_error_inject(
        &mut self,
        id: u32,
        inject: &str,
        pending: &[(u32, String)],
        visiting: &mut Vec<u32>,
    ) -> IResult<()> {
        if visiting.contains(&id) {
            return Err(InterpretError::new(
                format!(
                    "cyclic error injection involving '{}'",
                    self.error_type_name(id)
                ),
                Span::default(),
                "E320",
            ));
        }
        visiting.push(id);
        let target = self.injected_error_id(inject).ok_or_else(|| {
            InterpretError::new(
                format!(
                    "unknown injected error type '{inject}' for '{}'",
                    self.error_type_name(id)
                ),
                Span::default(),
                "E320",
            )
        })?;
        if let Some((_, target_inject)) = pending.iter().find(|(tid, _)| *tid == target) {
            let target_inject = target_inject.clone();
            self.apply_error_inject(target, &target_inject, pending, visiting)?;
        }
        let members = self.error_types[target as usize].1.clone();
        let (_, own) = &mut self.error_types[id as usize];
        for (name, tag) in members {
            if !own.iter().any(|(n, _)| *n == name) {
                own.push((name, tag));
            }
        }
        visiting.pop();
        Ok(())
    }

    /// Resolve an inject path to a registered error type id. Error types
    /// register globally by bare name, so a module-qualified path such as
    /// `error.Error` falls back to its last segment.
    fn injected_error_id(&self, inject: &str) -> Option<u32> {
        if let Some(id) = self.error_type_ids.get(inject) {
            return Some(*id);
        }
        inject
            .rsplit_once('.')
            .and_then(|(_, last)| self.error_type_ids.get(last))
            .copied()
    }

    fn error_type_name(&self, id: u32) -> &str {
        self.error_types
            .get(id as usize)
            .map_or("", |(name, _)| name)
    }

    fn register_implement(&mut self, module: Option<String>, imp: &Implement) -> IResult<()> {
        for f in &imp.functions {
            let short = format!("{}::{}", imp.target, f.name);
            let fq = match &module {
                Some(path) => format!("{path}::{short}"),
                None => short.clone(),
            };
            if matches!(f.visibility, Some(Visibility::Public)) {
                self.public_funcs.insert(fq.clone());
                self.public_funcs.insert(short.clone());
            }
            let local_requires = self.register_function_tree(module.clone(), fq, f, true)?;
            // Calls resolve as `Type::method` without a module prefix.
            if module.is_some() {
                self.funcs.insert(
                    short.clone(),
                    FuncEntry {
                        fq: short,
                        module: module.clone(),
                        function: f.clone(),
                        is_method: true,
                        local_requires,
                    },
                );
            }
        }
        Ok(())
    }

    /// Collect `(path, alias)` pairs of the `require` statements in a function
    /// body (PLAN 2.6.1). Nested functions are excluded: they register their
    /// own entries. Block bodies (if / for / match / defer / handle) belong
    /// to the function's scope per the grammar.
    fn body_requires(body: &[Stmt], out: &mut Vec<(String, Option<String>)>) {
        for s in body {
            match &s.kind {
                StmtKind::Require { path, alias } => out.push((path.clone(), alias.clone())),
                StmtKind::Function(_) => {}
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    Self::body_requires(then_branch, out);
                    Self::body_requires(else_branch, out);
                }
                StmtKind::For { body, .. }
                | StmtKind::Defer { body }
                | StmtKind::Unsafe { body }
                | StmtKind::Handle { body, .. } => Self::body_requires(body, out),
                StmtKind::Match {
                    cases, else_branch, ..
                } => {
                    for c in cases {
                        Self::body_requires(&c.body, out);
                    }
                    Self::body_requires(else_branch, out);
                }
                _ => {}
            }
        }
    }

    /// Resolve the function's `require` statements against the loaded module
    /// table. Modules are hoisted at load time, so every path here is loaded.
    fn resolve_local_requires(&mut self, f: &Function) -> IResult<Vec<LocalRequire>> {
        let mut raw = Vec::new();
        Self::body_requires(&f.body, &mut raw);
        let mut local_requires = Vec::new();
        for (path, alias) in raw {
            let (module_path, item) = self.resolve_require(&path)?;
            local_requires.push(LocalRequire {
                alias,
                path: module_path,
                item,
            });
        }
        Ok(local_requires)
    }

    fn register_function_tree(
        &mut self,
        module: Option<String>,
        fq: String,
        f: &Function,
        is_method: bool,
    ) -> IResult<Vec<LocalRequire>> {
        if matches!(f.visibility, Some(Visibility::Public)) {
            self.public_funcs.insert(fq.clone());
        }
        for stmt in &f.body {
            if let StmtKind::Function(nf) = &stmt.kind {
                let nested = format!("{fq}::{}", nf.name);
                self.register_function_tree(module.clone(), nested, nf, false)?;
            }
        }
        let local_requires = self.resolve_local_requires(f)?;
        self.funcs.insert(
            fq.clone(),
            FuncEntry {
                fq,
                module,
                function: f.clone(),
                is_method,
                local_requires: local_requires.clone(),
            },
        );
        Ok(local_requires)
    }

    /// Bind top-level requires globally (PLAN 2.6.2). Function-body requires
    /// are bound per function entry instead (PLAN 2.6.1) and never reach the
    /// global maps.
    fn register_module_bindings(&mut self) -> IResult<()> {
        for (path, alias) in self.top_requires.clone() {
            let (module_path, item) = self.resolve_require(&path)?;
            if let Some(alias) = alias {
                self.aliases.insert(alias.clone(), module_path.clone());
                if let Some(item) = item {
                    self.item_aliases.insert(alias, item);
                }
            } else if let Some(item) = item {
                let fq = format!("{module_path}::{item}");
                self.plain_funcs.insert(item, fq);
            } else if let Some(m) = self.modules.iter().find(|m| m.path == module_path) {
                for item in &m.program.items {
                    if let StmtKind::Function(f) = &item.kind
                        && matches!(f.visibility, Some(Visibility::Public))
                    {
                        let fq = format!("{}::{}", m.path, f.name);
                        self.plain_funcs.insert(f.name.clone(), fq);
                    }
                }
            }
        }
        Ok(())
    }

    fn load_requires(
        &mut self,
        program: &Program,
        out: &mut Vec<RequiredModule>,
        loading: &mut HashSet<String>,
    ) -> IResult<()> {
        self.load_requires_stmts(&program.items, out, loading, false)
    }

    fn load_requires_stmts(
        &mut self,
        stmts: &[Stmt],
        out: &mut Vec<RequiredModule>,
        loading: &mut HashSet<String>,
        in_function: bool,
    ) -> IResult<()> {
        for s in stmts {
            match &s.kind {
                StmtKind::Require { path, alias } => {
                    // Top-level requires bind globally (PLAN 2.6.2);
                    // function-body requires bind per function entry.
                    if !in_function {
                        self.top_requires.push((path.clone(), alias.clone()));
                    }
                    self.load_one(path, alias, s.span, out, loading)?
                }
                StmtKind::Function(f) => self.load_requires_stmts(&f.body, out, loading, true)?,
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.load_requires_stmts(then_branch, out, loading, in_function)?;
                    self.load_requires_stmts(else_branch, out, loading, in_function)?;
                }
                StmtKind::For { body, .. } => {
                    self.load_requires_stmts(body, out, loading, in_function)?
                }
                StmtKind::Match {
                    cases, else_branch, ..
                } => {
                    for c in cases {
                        self.load_requires_stmts(&c.body, out, loading, in_function)?;
                    }
                    self.load_requires_stmts(else_branch, out, loading, in_function)?;
                }
                StmtKind::Defer { body } | StmtKind::Unsafe { body } => {
                    self.load_requires_stmts(body, out, loading, in_function)?;
                }
                StmtKind::Handle { body, .. } => {
                    self.load_requires_stmts(body, out, loading, in_function)?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn load_one(
        &mut self,
        path: &str,
        alias: &Option<String>,
        span: Span,
        out: &mut Vec<RequiredModule>,
        loading: &mut HashSet<String>,
    ) -> IResult<()> {
        let (module_path, item) = self.resolve_require(path)?;
        if out.iter().any(|m| m.path == module_path) {
            // Already loaded (each module loads once). Alias and plain-name
            // bindings are registered from the recorded require statements,
            // so there is nothing more to do here.
            return Ok(());
        }
        if !loading.insert(module_path.clone()) {
            return Err(InterpretError::new(
                format!("module dependency cycle involving '{module_path}'"),
                span,
                "E382",
            ));
        }
        let source = self
            .loader
            .load_at(&module_path, span)
            .map_err(|e| InterpretError::new(e.message, e.span, e.code))?;
        let tokens = Tokenizer::new(source)
            .tokenize()
            .map_err(|e| InterpretError::new(e.message, Span::from_location(e.location), e.code))?;
        let sub = parse(tokens).map_err(|batch| {
            let d = batch.iter().next();
            InterpretError::new(
                d.map(|x| x.message.clone())
                    .unwrap_or_else(|| "parse error in required module".into()),
                d.and_then(|x| x.primary_span()).unwrap_or_default(),
                d.map(|x| x.code.clone()).unwrap_or_else(|| "E200".into()),
            )
        })?;
        self.load_requires(&sub, out, loading)?;
        loading.remove(&module_path);
        out.push(RequiredModule {
            path: module_path,
            alias: alias.clone(),
            item,
            program: sub,
        });
        Ok(())
    }

    fn resolve_require(&mut self, path: &str) -> IResult<(String, Option<String>)> {
        if let Some((parent, last)) = path.rsplit_once('.')
            && let Some(parent_source) = self.loader.try_load(parent)
        {
            let tokens = Tokenizer::new(parent_source).tokenize().map_err(|e| {
                InterpretError::new(e.message, Span::from_location(e.location), e.code)
            })?;
            let parent_prog = parse(tokens).map_err(|_| {
                InterpretError::new("failed to parse parent module", Span::default(), "E380")
            })?;
            // Item imports are restricted to `public` functions (PLAN 2.1.3):
            // a private function stays file-local, so requiring it by name
            // is rejected at load time instead of binding it.
            if let Some(f) = parent_prog.items.iter().find_map(|i| match &i.kind {
                StmtKind::Function(f) if f.name == last => Some(f),
                _ => None,
            }) {
                if !matches!(f.visibility, Some(Visibility::Public)) {
                    return Err(InterpretError::new(
                        format!("module '{parent}' does not export '{last}': private function"),
                        Span::default(),
                        "E330",
                    ));
                }
                return Ok((parent.to_string(), Some(last.to_string())));
            }
        }
        Ok((path.to_string(), None))
    }

    fn call_function(&mut self, entry: &FuncEntry, stack: &mut Vec<Slot>) -> IResult<()> {
        let n = entry.function.params.len();
        if stack.len() < n {
            return Err(InterpretError::new(
                format!("call to '{}' requires {n} argument(s)", entry.function.name),
                Span::default(),
                "E331",
            ));
        }
        let args: Vec<Slot> = stack.split_off(stack.len() - n);
        let mut local_stack = args;
        // `T copy` parameters receive a deep copy of the heap argument so the
        // callee cannot mutate the caller's value. The method receiver is
        // exempt: `reference<T> mutable` receivers must keep mutating the
        // pointee, so `self` always borrows. The default (no modifier) keeps
        // the grammar's move-on-call behavior.
        for (i, param) in entry.function.params.iter().enumerate() {
            if !matches!(param.modifier, Some(ParamModifier::Copy)) {
                continue;
            }
            if entry.is_method && i == 0 {
                continue;
            }
            if let Some(arg) = local_stack.get_mut(i) {
                self.deep_copy_arg(arg);
            }
        }
        // Function-scoped `require` bindings (PLAN 2.6.1): alias and plain
        // names from this function's own requires shadow the global ones.
        let mut local_aliases = HashMap::new();
        let mut local_item_aliases = HashMap::new();
        let mut local_plain_funcs = HashMap::new();
        for r in &entry.local_requires {
            if let Some(alias) = &r.alias {
                local_aliases.insert(alias.clone(), r.path.clone());
                if let Some(item) = &r.item {
                    local_item_aliases.insert(alias.clone(), item.clone());
                }
            } else if let Some(item) = &r.item {
                local_plain_funcs.insert(item.clone(), format!("{}::{item}", r.path));
            } else if let Some(m) = self.modules.iter().find(|m| m.path == r.path) {
                for item in &m.program.items {
                    if let StmtKind::Function(f) = &item.kind
                        && matches!(f.visibility, Some(Visibility::Public))
                    {
                        let fq = format!("{}::{}", r.path, f.name);
                        local_plain_funcs.insert(f.name.clone(), fq);
                    }
                }
            }
        }
        // PLAN 4.1: the signature resolver consults the function's own
        // aliases before the global ones.
        let error_payload = self.fallible_payload(&entry.function.returns, &local_aliases);
        let mut frame = Frame {
            locals: HashMap::new(),
            local_mutability: HashMap::new(),
            loops: Vec::new(),
            deferred: vec![Vec::new()],
            error_payload: error_payload.clone(),
            pending_error_return: None,
            pending_flow: None,
            local_aliases,
            local_item_aliases,
            local_plain_funcs,
        };
        if entry.is_method
            && let Some(recv) = local_stack.first()
        {
            frame.locals.insert(
                "self".to_string(),
                Slot {
                    value: recv.value.clone_for_stack(),
                    kind: recv.kind,
                },
            );
        }
        let _flow = self.eval_body(
            &entry.function.body,
            &mut local_stack,
            &mut frame,
            entry.module.as_deref(),
            &entry.fq,
        )?;

        if let Some(_payload_ty) = error_payload {
            // Fallible ABI: leave `(payload, env)` on the caller stack.
            let (mut payload, env_tag) =
                if matches!(local_stack.last().map(|s| s.kind), Some(KIND_ERROR)) {
                    let env = local_stack.pop().unwrap();
                    let tag = match env.value {
                        Value::Int(n) => n,
                        other => {
                            other.drop_owned();
                            return Err(InterpretError::new(
                                "fallible error return must be an error tag",
                                Span::default(),
                                "E328",
                            ));
                        }
                    };
                    (
                        Slot {
                            value: Value::Int(0),
                            kind: 4,
                        },
                        tag,
                    )
                } else if local_stack.is_empty() {
                    (
                        Slot {
                            value: Value::Int(0),
                            kind: 4,
                        },
                        0,
                    )
                } else {
                    (local_stack.pop().unwrap(), 0)
                };
            Self::claim_returned_heaps(std::slice::from_mut(&mut payload), &mut frame);
            self.run_deferred(
                &mut local_stack,
                &mut frame,
                entry.module.as_deref(),
                &entry.fq,
            )?;
            while let Some(v) = local_stack.pop() {
                v.drop_owned();
            }
            for (_, slot) in frame.locals.drain() {
                slot.drop_owned();
            }
            stack.push(payload);
            stack.push(Slot {
                value: Value::Int(env_tag),
                kind: KIND_ERROR,
            });
            return Ok(());
        }

        let ret_n = entry
            .function
            .returns
            .iter()
            .filter(|t| !matches!(t.kind, TypeKind::Primitive(Primitive::Void)))
            .count();
        let mut rets = if ret_n == 0 {
            Vec::new()
        } else {
            if local_stack.len() < ret_n {
                return Err(InterpretError::new(
                    format!(
                        "function '{}' returned {} value(s), expected {ret_n}",
                        entry.function.name,
                        local_stack.len()
                    ),
                    Span::default(),
                    "E328",
                ));
            }
            local_stack.split_off(local_stack.len() - ret_n)
        };
        Self::claim_returned_heaps(&mut rets, &mut frame);
        self.run_deferred(
            &mut local_stack,
            &mut frame,
            entry.module.as_deref(),
            &entry.fq,
        )?;
        while let Some(v) = local_stack.pop() {
            v.drop_owned();
        }
        for (_, slot) in frame.locals.drain() {
            slot.drop_owned();
        }
        stack.extend(rets);
        Ok(())
    }

    /// Transfer ownership of heap returns out of locals (matches JIT `moved`
    /// on return) so scope exit does not free values the caller now owns.
    fn claim_returned_heaps(rets: &mut [Slot], frame: &mut Frame) {
        for ret in rets.iter_mut() {
            match &mut ret.value {
                Value::Heap { handle, owned } => {
                    let h = *handle;
                    for local in frame.locals.values_mut() {
                        if let Value::Heap {
                            handle: lh,
                            owned: lo,
                        } = &mut local.value
                            && *lh == h
                            && *lo
                        {
                            *lo = false;
                            *owned = true;
                        }
                    }
                }
                Value::Str { handle, owned } => {
                    let h = *handle;
                    for local in frame.locals.values_mut() {
                        if let Value::Str {
                            handle: lh,
                            owned: lo,
                        } = &mut local.value
                            && *lh == h
                            && *lo
                        {
                            *lo = false;
                            *owned = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Replace a `T copy` argument in place with a deep copy (PLAN 2.3.1).
    /// Heap arguments (string, list, map, struct, union) are cloned through
    /// the host runtime; the original is freed only when the caller passed an
    /// owned temporary, since a borrow stays owned by the caller's variable.
    /// Non-heap arguments are left as-is.
    fn deep_copy_arg(&self, slot: &mut Slot) {
        let (handle, owned, kind) = match &slot.value {
            Value::Str { handle, owned } => (*handle, *owned, KIND_STRING),
            Value::Heap { handle, owned } => {
                if !matches!(
                    slot.kind & 0xff,
                    KIND_LIST | KIND_MAP | KIND_STRUCT | KIND_UNION
                ) {
                    return;
                }
                (*handle, *owned, slot.kind)
            }
            _ => return,
        };
        // Union payloads need the active member's kind, which only the
        // interpreter can resolve (union layouts are not registered with
        // the host runtime).
        let clone = if kind & 0xff == KIND_UNION {
            self.clone_union_handle(handle, (kind >> 8) as u32)
        } else {
            runtime::yarrow_value_clone(handle, kind)
        };
        if owned {
            free_value(handle, kind);
        }
        slot.value = if kind == KIND_STRING {
            Value::Str {
                handle: clone,
                owned: true,
            }
        } else {
            Value::Heap {
                handle: clone,
                owned: true,
            }
        };
    }

    /// Deep-copy a union block: clone the active member's payload using the
    /// member's kind code. Unions nested inside containers are cloned by the
    /// runtime without member kinds and share the payload handle, matching
    /// `free_value`'s treatment of unregistered unions.
    fn clone_union_handle(&self, handle: u64, uid: u32) -> u64 {
        let active = unsafe {
            std::ptr::read_unaligned(
                (handle as *const u8).add(UNION_TAG_OFFSET as usize) as *const u64
            )
        };
        let payload = unsafe {
            std::ptr::read_unaligned(
                (handle as *const u8).add(UNION_PAYLOAD_OFFSET as usize) as *const u64
            )
        };
        let member_kind = self
            .unions
            .get(uid as usize)
            .and_then(|u| u.members.get(active as usize))
            .and_then(|m| self.member_ty_kind_code(m));
        let payload = match member_kind {
            Some(k) if (k & 0xff) > 15 && (k & 0xff) != KIND_PTR => {
                runtime::yarrow_value_clone(payload, k)
            }
            _ => payload,
        };
        let clone = runtime::yarrow_alloc(16);
        unsafe {
            std::ptr::write_unaligned(
                (clone as *mut u8).add(UNION_TAG_OFFSET as usize) as *mut u64,
                active,
            );
            std::ptr::write_unaligned(
                (clone as *mut u8).add(UNION_PAYLOAD_OFFSET as usize) as *mut u64,
                payload,
            );
        }
        clone
    }

    /// Runtime kind code of a union member type, for payload clone/free.
    fn member_ty_kind_code(&self, m: &MemberTy) -> Option<u64> {
        match m {
            MemberTy::String => Some(KIND_STRING),
            MemberTy::Primitive(k) => Some(*k),
            MemberTy::Named(n) => self.named_type_kind_code(n),
        }
    }

    /// Member type of a runtime slot, for union wrapping. PLAN 3.1: an enum
    /// slot maps back to the named enum type, not its carrier.
    fn slot_member_ty(&self, slot: &Slot) -> MemberTy {
        if (slot.kind & 0xff) == KIND_ENUM
            && let Some(info) = self.enums.get((slot.kind >> 8) as usize)
        {
            return MemberTy::Named(info.name.clone());
        }
        match slot.kind & 0xff {
            KIND_STRING => MemberTy::String,
            k if k <= 15 => MemberTy::Primitive(k),
            _ => MemberTy::Primitive(slot.kind),
        }
    }

    /// Function-scope exit: run deferred bodies in reverse registration order.
    /// Nested scopes already ran their defers at their own exit, so normally
    /// only the function-body scope remains; any leftover scopes run
    /// innermost first.
    fn run_deferred(
        &mut self,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
    ) -> IResult<()> {
        while let Some(scope) = frame.deferred.pop() {
            for body in scope.into_iter().rev() {
                let flow = self.eval_body(&body, stack, frame, module, caller_fq)?;
                if flow == Flow::Return {
                    // Nested return from a defer is ignored for stack shaping; the
                    // outer function already committed its return values.
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Enter a nested defer scope: `if` / `else` / `for` / `match` case /
    /// `handle` bodies register defers separately from the function body.
    fn defer_enter_scope(frame: &mut Frame) {
        frame.deferred.push(Vec::new());
    }

    /// Exit the innermost defer scope, running its bodies in reverse
    /// registration order. A `return` from a defer body stops the remaining
    /// defers of the scope, matching `run_deferred`.
    fn defer_exit_scope(
        &mut self,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
    ) -> IResult<()> {
        let Some(scope) = frame.deferred.pop() else {
            return Ok(());
        };
        for body in scope.into_iter().rev() {
            let flow = self.eval_body(&body, stack, frame, module, caller_fq)?;
            if flow == Flow::Return {
                break;
            }
        }
        Ok(())
    }
    fn eval_body(
        &mut self,
        body: &[Stmt],
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
    ) -> IResult<Flow> {
        for stmt in body {
            let flow = self.eval_stmt(stmt, stack, frame, module, caller_fq)?;
            match flow {
                Flow::Return => return Ok(Flow::Return),
                Flow::Break | Flow::Continue => return Ok(flow),
                Flow::Next => {
                    if let Some(sig) = frame.pending_flow.take() {
                        return Ok(sig);
                    }
                    if let Some(tag) = frame.pending_error_return.take() {
                        while let Some(v) = stack.pop() {
                            v.drop_owned();
                        }
                        stack.push(Slot {
                            value: Value::Int(tag),
                            kind: KIND_ERROR,
                        });
                        return Ok(Flow::Return);
                    }
                }
            }
        }
        Ok(Flow::Next)
    }

    fn eval_stmt(
        &mut self,
        stmt: &Stmt,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
    ) -> IResult<Flow> {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.eval_expr(e, stack, frame, module, caller_fq, stmt.span)?;
                Ok(Flow::Next)
            }
            StmtKind::Require { .. } | StmtKind::Function(_) => Ok(Flow::Next),
            StmtKind::Struct(_)
            | StmtKind::Implement(_)
            | StmtKind::Enum(_)
            | StmtKind::Union(_)
            | StmtKind::Error(_) => Ok(Flow::Next),
            StmtKind::VarDecl {
                name,
                mutability,
                ty,
                value,
            } => {
                // PLAN 5.1.2: record the declared mutability before anything
                // else, so the `self` re-binding below carries it too.
                frame.local_mutability.insert(name.clone(), *mutability);
                // Method receiver already bound at call entry.
                if name == "self" && frame.locals.contains_key("self") {
                    return Ok(Flow::Next);
                }
                let kind = self.type_kind_code(ty).ok_or_else(|| {
                    InterpretError::unsupported(format!("binding type {ty:?}"), stmt.span)
                })?;
                let mut slot = match value {
                    Some(Expr::StructLit(pairs)) => {
                        self.eval_struct_lit(pairs, ty, stack, frame, module, caller_fq, stmt.span)?
                    }
                    Some(Expr::EmptyMapOrStruct) => self.eval_empty_aggregate(ty, stmt.span)?,
                    Some(Expr::List(elems)) => {
                        self.eval_list_lit(elems, ty, stack, frame, module, caller_fq, stmt.span)?
                    }
                    Some(Expr::Map(pairs)) => {
                        self.eval_map_lit(pairs, ty, stack, frame, module, caller_fq, stmt.span)?
                    }
                    Some(Expr::Seq(elems)) => {
                        // Prior words are side effects; trailing literal is the value.
                        if elems.is_empty() {
                            return Err(InterpretError::new(
                                "empty initializer sequence",
                                stmt.span,
                                "E306",
                            ));
                        }
                        for (el, sp) in &elems[..elems.len() - 1] {
                            self.eval_expr(el, stack, frame, module, caller_fq, *sp)?;
                        }
                        let (last, last_span) = &elems[elems.len() - 1];
                        match last {
                            Expr::StructLit(pairs) => self.eval_struct_lit(
                                pairs, ty, stack, frame, module, caller_fq, *last_span,
                            )?,
                            Expr::EmptyMapOrStruct => self.eval_empty_aggregate(ty, *last_span)?,
                            Expr::List(elems) => self.eval_list_lit(
                                elems, ty, stack, frame, module, caller_fq, *last_span,
                            )?,
                            Expr::Map(pairs) => self.eval_map_lit(
                                pairs, ty, stack, frame, module, caller_fq, *last_span,
                            )?,
                            other => {
                                self.eval_expr(other, stack, frame, module, caller_fq, *last_span)?;
                                let mut s = self.pop(stack, stmt.span, "value")?;
                                s = self.coerce_to_binding(s, ty, stmt.span)?;
                                s.kind = kind;
                                s
                            }
                        }
                    }
                    Some(e) => {
                        self.eval_expr(e, stack, frame, module, caller_fq, stmt.span)?;
                        let mut s = self.pop(stack, stmt.span, "value")?;
                        s = self.coerce_to_binding(s, ty, stmt.span)?;
                        s.kind = kind;
                        s
                    }
                    None => {
                        let mut s = self.pop(stack, stmt.span, "value")?;
                        s = self.coerce_to_binding(s, ty, stmt.span)?;
                        s.kind = kind;
                        s
                    }
                };
                // PLAN 1.3: `array<T>` with no declared size infers `N` from
                // a non-empty array literal. An explicit `N` always wins
                // (inference only runs for `size: None`); an empty literal
                // leaves nothing to infer from.
                if let TypeKind::Array { size: None, .. } = &ty.kind
                    && let Value::Array(elems) = &slot.value
                {
                    if elems.is_empty() {
                        return Err(InterpretError::new(
                            "cannot infer the array size: the literal is empty and no size is declared",
                            stmt.span,
                            "E381",
                        ));
                    }
                    slot.kind = kind | ((elems.len() as u64) << 40);
                }
                if let Some(old) = frame.locals.insert(name.clone(), slot) {
                    old.drop_owned();
                }
                Ok(Flow::Next)
            }
            StmtKind::Set { target, value } => {
                if let Some(e) = value {
                    self.eval_expr(e, stack, frame, module, caller_fq, stmt.span)?;
                }
                let mut slot = self.pop(stack, stmt.span, "set")?;
                match target {
                    Expr::Variable { name } => {
                        // PLAN 5.1.2: `set` is restricted to `mutable`
                        // bindings; this backstops the checker for paths
                        // that reach the interpreter unchecked.
                        let mutability = frame
                            .local_mutability
                            .get(name)
                            .copied()
                            .unwrap_or(Mutability::Mutable);
                        if matches!(mutability, Mutability::Const | Mutability::Static) {
                            return Err(InterpretError::new(
                                format!(
                                    "cannot set '{name}': the binding is {}, not mutable",
                                    match mutability {
                                        Mutability::Const => "const",
                                        Mutability::Static => "static",
                                        Mutability::Mutable => unreachable!(),
                                    }
                                ),
                                stmt.span,
                                "E378",
                            ));
                        }
                        let old = frame.locals.get(name).cloned().ok_or_else(|| {
                            InterpretError::new(
                                format!("unknown variable '{name}'"),
                                stmt.span,
                                "E320",
                            )
                        })?;
                        // When assigning into a union variable, wrap the raw member.
                        if (old.kind & 0xff) == KIND_UNION {
                            slot =
                                self.wrap_union_value(slot, (old.kind >> 8) as u32, stmt.span)?;
                        } else {
                            slot.kind = old.kind;
                        }
                        if let Some(old) = frame.locals.insert(name.clone(), slot) {
                            old.drop_owned();
                        }
                        Ok(Flow::Next)
                    }
                    Expr::Member { base, member } => {
                        self.eval_expr(base, stack, frame, module, caller_fq, stmt.span)?;
                        let base_slot = self.pop(stack, stmt.span, "field set target")?;
                        let (handle, sid) = match self.struct_base_addr(&base_slot, stmt.span) {
                            Ok(v) => v,
                            Err(e) => {
                                base_slot.drop_owned();
                                slot.drop_owned();
                                return Err(e);
                            }
                        };
                        let info = self.structs.get(sid as usize).ok_or_else(|| {
                            InterpretError::new(
                                format!("struct has no field '{member}'"),
                                stmt.span,
                                "E340",
                            )
                        })?;
                        let field = info
                            .fields
                            .iter()
                            .find(|f| f.name == *member)
                            .cloned()
                            .ok_or_else(|| {
                                InterpretError::new(
                                    format!("struct has no field '{member}'"),
                                    stmt.span,
                                    "E340",
                                )
                            })?;
                        // PLAN 5.4.2: private fields are set only from the
                        // declaring unit.
                        if let Err(e) = self.check_field_visibility(info, &field, module, stmt.span)
                        {
                            base_slot.drop_owned();
                            slot.drop_owned();
                            return Err(e);
                        }
                        // Free previous union payload before overwrite (matches JIT).
                        if (field.kind & 0xff) == KIND_UNION {
                            let old_bits = read_bits_at(handle, field.offset, field.size);
                            free_value(old_bits, field.kind);
                        }
                        let bits = slot.value.as_bits().unwrap_or(0);
                        write_bits_at(handle, field.offset, field.size, bits);
                        // Field storage owns heap bits; do not drop the new value.
                        match slot.value {
                            Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                            other => other.drop_owned(),
                        }
                        base_slot.drop_owned();
                        Ok(Flow::Next)
                    }
                    _ => {
                        slot.drop_owned();
                        Err(InterpretError::unsupported(
                            "complex 'set' target",
                            stmt.span,
                        ))
                    }
                }
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let pre = stack.len();
                let cond = self.eval_cond(condition, stack, frame, module, caller_fq, stmt.span)?;
                while stack.len() > pre {
                    stack.pop().unwrap().drop_owned();
                }
                let branch = if cond { then_branch } else { else_branch };
                Self::defer_enter_scope(frame);
                let flow = self.eval_body(branch, stack, frame, module, caller_fq)?;
                self.defer_exit_scope(stack, frame, module, caller_fq)?;
                Ok(flow)
            }
            StmtKind::Match {
                value,
                cases,
                else_branch,
            } => self.eval_match(
                value,
                cases,
                else_branch,
                stack,
                frame,
                module,
                caller_fq,
                stmt.span,
            ),
            StmtKind::For { source, body } => {
                self.eval_for(source, body, stack, frame, module, caller_fq, stmt.span)
            }
            StmtKind::Return { value } => {
                if let Some(v) = value {
                    self.eval_expr(v, stack, frame, module, caller_fq, stmt.span)?;
                }
                Ok(Flow::Return)
            }
            StmtKind::Handle { body, fallback } => self.eval_handle(
                body,
                fallback.as_ref(),
                stack,
                frame,
                module,
                caller_fq,
                stmt.span,
            ),
            StmtKind::Defer { body } => {
                if let Some(scope) = frame.deferred.last_mut() {
                    scope.push(body.clone());
                }
                Ok(Flow::Next)
            }
            StmtKind::Unsafe { body } => {
                // Checker already enforced the unsafe boundary; interpret the body.
                self.eval_body(body, stack, frame, module, caller_fq)
            }
            StmtKind::Move { target, source } => {
                let taken = match source {
                    Expr::Variable { name } => {
                        let local = frame.locals.get_mut(name).ok_or_else(|| {
                            InterpretError::new(
                                format!("unknown variable '{name}'"),
                                stmt.span,
                                "E320",
                            )
                        })?;
                        let taken = Slot {
                            value: match &local.value {
                                Value::Heap { handle, .. } => Value::Heap {
                                    handle: *handle,
                                    owned: true,
                                },
                                Value::Str { handle, .. } => Value::Str {
                                    handle: *handle,
                                    owned: true,
                                },
                                other => other.clone(),
                            },
                            kind: local.kind,
                        };
                        match &mut local.value {
                            Value::Heap { owned, .. } | Value::Str { owned, .. } => {
                                *owned = false;
                            }
                            _ => {}
                        }
                        taken
                    }
                    other => {
                        self.eval_expr(other, stack, frame, module, caller_fq, stmt.span)?;
                        self.pop(stack, stmt.span, "'move' source")?
                    }
                };
                if let Some(old) = frame.locals.insert(target.clone(), taken) {
                    old.drop_owned();
                }
                Ok(Flow::Next)
            }
            StmtKind::Fallback { .. } => Ok(Flow::Next),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_match(
        &mut self,
        value: &Expr,
        cases: &[MatchCase],
        else_branch: &[Stmt],
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Flow> {
        let pre = stack.len();
        let has_subject = !matches!(value, Expr::Variable { name } if name.is_empty());
        if has_subject {
            self.eval_expr(value, stack, frame, module, caller_fq, span)?;
        }
        let sub_len = stack.len();

        for case in cases {
            match &case.kind {
                MatchCaseKind::Condition(cond) => {
                    while stack.len() > sub_len {
                        stack.pop().unwrap().drop_owned();
                    }
                    if stack.len() < sub_len {
                        return Err(InterpretError::new("match subject missing", span, "E343"));
                    }
                    let truthy =
                        self.eval_match_cond(cond, stack, frame, module, caller_fq, span)?;
                    if truthy {
                        while stack.len() > sub_len {
                            stack.pop().unwrap().drop_owned();
                        }
                        Self::defer_enter_scope(frame);
                        let flow = self.eval_body(&case.body, stack, frame, module, caller_fq)?;
                        self.defer_exit_scope(stack, frame, module, caller_fq)?;
                        let extras: Vec<Slot> = stack.split_off(sub_len);
                        while stack.len() > pre {
                            stack.pop().unwrap().drop_owned();
                        }
                        stack.extend(extras);
                        return Ok(flow);
                    }
                }
                MatchCaseKind::Type(ty) => {
                    // Error member case (`AppError.NOT_FOUND case`) or union member.
                    if let Some(tag) = self.error_member_tag(ty) {
                        let subject = stack.get(sub_len.saturating_sub(1)).ok_or_else(|| {
                            InterpretError::new("match subject missing", span, "E343")
                        })?;
                        let matched = matches!(subject.value, Value::Int(n) if n as u32 == tag)
                            || matches!(
                                subject.kind,
                                KIND_ERROR if matches!(subject.value, Value::Int(n) if n as u32 == tag)
                            );
                        if matched {
                            while stack.len() > sub_len {
                                stack.pop().unwrap().drop_owned();
                            }
                            Self::defer_enter_scope(frame);
                            let flow =
                                self.eval_body(&case.body, stack, frame, module, caller_fq)?;
                            self.defer_exit_scope(stack, frame, module, caller_fq)?;
                            let extras: Vec<Slot> = stack.split_off(sub_len);
                            while stack.len() > pre {
                                stack.pop().unwrap().drop_owned();
                            }
                            stack.extend(extras);
                            return Ok(flow);
                        }
                        continue;
                    }
                    // Union type dispatch.
                    let subject =
                        stack
                            .get(sub_len.saturating_sub(1))
                            .cloned()
                            .ok_or_else(|| {
                                InterpretError::new("match subject missing", span, "E343")
                            })?;
                    if (subject.kind & 0xff) != KIND_UNION {
                        subject.drop_owned();
                        return Err(InterpretError::new(
                            "match type dispatch requires a union subject",
                            span,
                            "E308",
                        ));
                    }
                    let uid = (subject.kind >> 8) as u32;
                    let want = self.member_ty(ty)?;
                    let idx = self
                        .unions
                        .get(uid as usize)
                        .and_then(|u| u.members.iter().position(|m| m == &want));
                    let Some(idx) = idx else {
                        continue;
                    };
                    let handle = match subject.value {
                        Value::Heap { handle, .. } => handle,
                        other => {
                            other.drop_owned();
                            return Err(InterpretError::new(
                                "union subject must be a heap handle",
                                span,
                                "E308",
                            ));
                        }
                    };
                    let tag = unsafe {
                        std::ptr::read_unaligned(
                            (handle as *const u8).add(UNION_TAG_OFFSET as usize) as *const u64,
                        )
                    };
                    if tag != idx as u64 {
                        continue;
                    }
                    let payload_bits = unsafe {
                        std::ptr::read_unaligned(
                            (handle as *const u8).add(UNION_PAYLOAD_OFFSET as usize) as *const u64,
                        )
                    };
                    while stack.len() > sub_len {
                        stack.pop().unwrap().drop_owned();
                    }
                    // Push payload as a borrowed reference (auto-deref on use).
                    let payload = self.bits_to_slot(payload_bits, &want, false);
                    stack.push(payload);
                    Self::defer_enter_scope(frame);
                    let flow = self.eval_body(&case.body, stack, frame, module, caller_fq)?;
                    self.defer_exit_scope(stack, frame, module, caller_fq)?;
                    // Drop unconsumed borrow of the payload.
                    if stack.len() > sub_len
                        && matches!(
                            stack.get(sub_len).map(|s| &s.value),
                            Some(
                                Value::Heap { owned: false, .. }
                                    | Value::Str { owned: false, .. }
                                    | Value::Int(_)
                                    | Value::Float(_)
                                    | Value::Bool(_)
                            )
                        )
                    {
                        // If the first extra is still the payload we pushed, drop it.
                        let first = &stack[sub_len];
                        let still_payload = match (&first.value, &want) {
                            (Value::Heap { handle: h, .. }, MemberTy::String)
                            | (Value::Str { handle: h, .. }, MemberTy::String) => {
                                *h == payload_bits
                            }
                            (Value::Int(n), MemberTy::Primitive(_)) => *n as u64 == payload_bits,
                            (Value::Heap { handle: h, .. }, _) => *h == payload_bits,
                            _ => false,
                        };
                        if still_payload {
                            stack.remove(sub_len).drop_owned();
                        }
                    }
                    let extras: Vec<Slot> = stack.split_off(sub_len);
                    while stack.len() > pre {
                        stack.pop().unwrap().drop_owned();
                    }
                    stack.extend(extras);
                    return Ok(flow);
                }
            }
        }

        while stack.len() > sub_len {
            stack.pop().unwrap().drop_owned();
        }
        Self::defer_enter_scope(frame);
        let flow = self.eval_body(else_branch, stack, frame, module, caller_fq)?;
        self.defer_exit_scope(stack, frame, module, caller_fq)?;
        let extras: Vec<Slot> = stack.split_off(sub_len);
        while stack.len() > pre {
            stack.pop().unwrap().drop_owned();
        }
        stack.extend(extras);
        Ok(flow)
    }

    /// Runtime dispatch for `for`: evaluate the source expression once and
    /// pick the condition-loop or iterable form from the popped value. A
    /// bool runs the condition form, which re-evaluates the source before
    /// every iteration; an array or list runs the iterable form on the
    /// already-popped value.
    #[allow(clippy::too_many_arguments)]
    fn eval_for(
        &mut self,
        source: &Expr,
        body: &[Stmt],
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Flow> {
        let pre = stack.len();
        self.eval_expr(source, stack, frame, module, caller_fq, span)?;
        let first = self.pop(stack, span, "'for' source")?;
        if frame.pending_flow.take().is_some() {
            first.drop_owned();
            return Ok(Flow::Next);
        }
        match first.value {
            Value::Bool(_) => {
                first.drop_owned();
                while stack.len() > pre {
                    stack.pop().unwrap().drop_owned();
                }
                self.eval_cond_for(source, body, stack, frame, module, caller_fq, span)
            }
            Value::Array(_) | Value::Heap { .. } => {
                self.eval_iter_for(pre, first, body, stack, frame, module, caller_fq, span)
            }
            other => {
                other.drop_owned();
                Err(InterpretError::new(
                    "'for' requires a bool condition or an array or list iterable",
                    span,
                    "E394",
                ))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_cond_for(
        &mut self,
        condition: &Expr,
        body: &[Stmt],
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Flow> {
        let pre = stack.len();
        loop {
            while stack.len() > pre {
                stack.pop().unwrap().drop_owned();
            }
            let cond = self.eval_cond(condition, stack, frame, module, caller_fq, span)?;
            while stack.len() > pre {
                stack.pop().unwrap().drop_owned();
            }
            match frame.pending_flow.take() {
                Some(Flow::Break) => break,
                Some(_) => continue,
                None => {}
            }
            if !cond {
                break;
            }
            frame.loops.push(LoopCtx {
                value: None,
                index: None,
            });
            Self::defer_enter_scope(frame);
            let flow = self.eval_body(body, stack, frame, module, caller_fq)?;
            self.defer_exit_scope(stack, frame, module, caller_fq)?;
            frame.loops.pop();
            match flow {
                Flow::Return => return Ok(Flow::Return),
                Flow::Break => {
                    while stack.len() > pre {
                        stack.pop().unwrap().drop_owned();
                    }
                    break;
                }
                Flow::Continue => continue,
                Flow::Next => {}
            }
            if stack.len() != pre {
                return Err(InterpretError::new(
                    "for body must leave the stack balanced",
                    span,
                    "E325",
                ));
            }
        }
        Ok(Flow::Next)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_iter_for(
        &mut self,
        pre: usize,
        iterable: Slot,
        body: &[Stmt],
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Flow> {
        match iterable.value {
            Value::Array(elems) => {
                for (i, elem) in elems.into_iter().enumerate() {
                    while stack.len() > pre {
                        stack.pop().unwrap().drop_owned();
                    }
                    let elem_slot = Slot {
                        kind: infer_value_kind(&elem),
                        value: elem,
                    };
                    frame.loops.push(LoopCtx {
                        value: Some(elem_slot),
                        index: Some(i as i64),
                    });
                    Self::defer_enter_scope(frame);
                    let flow = self.eval_body(body, stack, frame, module, caller_fq)?;
                    self.defer_exit_scope(stack, frame, module, caller_fq)?;
                    if let Some(ctx) = frame.loops.pop()
                        && let Some(v) = ctx.value
                    {
                        v.drop_owned();
                    }
                    match flow {
                        Flow::Return => return Ok(Flow::Return),
                        Flow::Break => {
                            while stack.len() > pre {
                                stack.pop().unwrap().drop_owned();
                            }
                            break;
                        }
                        Flow::Continue => continue,
                        Flow::Next => {}
                    }
                    if stack.len() != pre {
                        return Err(InterpretError::new(
                            "for body must leave the stack balanced",
                            span,
                            "E325",
                        ));
                    }
                }
            }
            Value::Heap { handle, owned } => {
                if (iterable.kind & 0xff) != KIND_LIST {
                    iterable.drop_owned();
                    return Err(InterpretError::new(
                        "'for' requires an array or list iterable",
                        span,
                        "E394",
                    ));
                }
                let elem_kind = iterable.kind >> 8;
                let len = runtime::yarrow_list_len(handle);
                for i in 0..len {
                    while stack.len() > pre {
                        stack.pop().unwrap().drop_owned();
                    }
                    let bits = runtime::yarrow_list_get(handle, i);
                    let elem_slot = Slot {
                        kind: elem_kind,
                        value: match elem_kind & 0xff {
                            KIND_STRING => Value::Str {
                                handle: bits,
                                owned: false,
                            },
                            KIND_STRUCT | KIND_UNION | KIND_LIST | KIND_MAP => Value::Heap {
                                handle: bits,
                                owned: false,
                            },
                            // PLAN 3.2: a string-carried enum element is a
                            // heap string handle.
                            t if t == KIND_ENUM && self.enum_carrier(elem_kind) == KIND_STRING => {
                                Value::Str {
                                    handle: bits,
                                    owned: false,
                                }
                            }
                            _ => Value::Int(bits as i64),
                        },
                    };
                    frame.loops.push(LoopCtx {
                        value: Some(elem_slot),
                        index: Some(i as i64),
                    });
                    Self::defer_enter_scope(frame);
                    let flow = self.eval_body(body, stack, frame, module, caller_fq)?;
                    self.defer_exit_scope(stack, frame, module, caller_fq)?;
                    if let Some(ctx) = frame.loops.pop()
                        && let Some(v) = ctx.value
                    {
                        v.drop_owned();
                    }
                    match flow {
                        Flow::Return => {
                            if owned {
                                free_value(handle, KIND_LIST);
                            }
                            return Ok(Flow::Return);
                        }
                        Flow::Break => {
                            while stack.len() > pre {
                                stack.pop().unwrap().drop_owned();
                            }
                            break;
                        }
                        Flow::Continue => continue,
                        Flow::Next => {}
                    }
                    if stack.len() != pre {
                        if owned {
                            free_value(handle, KIND_LIST);
                        }
                        return Err(InterpretError::new(
                            "for body must leave the stack balanced",
                            span,
                            "E325",
                        ));
                    }
                }
                if owned {
                    free_value(handle, KIND_LIST);
                }
            }
            other => {
                other.drop_owned();
                return Err(InterpretError::new(
                    "'for' requires an array or list iterable",
                    span,
                    "E394",
                ));
            }
        }
        Ok(Flow::Next)
    }

    fn eval_cond(
        &mut self,
        e: &Expr,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<bool> {
        let before = stack.len();
        self.eval_expr(e, stack, frame, module, caller_fq, span)?;
        if stack.len() != before + 1 {
            return Err(InterpretError::new(
                "condition must evaluate to a single value",
                span,
                "E324",
            ));
        }
        let slot = stack.pop().unwrap();
        match slot.value {
            Value::Bool(b) => Ok(b),
            other => {
                other.drop_owned();
                Err(InterpretError::new("condition must be bool", span, "E324"))
            }
        }
    }

    fn eval_match_cond(
        &mut self,
        e: &Expr,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<bool> {
        self.eval_expr(e, stack, frame, module, caller_fq, span)?;
        let slot = self.pop(stack, span, "match condition")?;
        match slot.value {
            Value::Bool(b) => Ok(b),
            other => {
                other.drop_owned();
                Err(InterpretError::new(
                    "match condition must be bool",
                    span,
                    "E324",
                ))
            }
        }
    }
    fn eval_expr(
        &mut self,
        expr: &Expr,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<()> {
        match expr {
            Expr::Integer { value } => {
                let n =
                    decode_int_literal(value).map_err(|m| InterpretError::new(m, span, "E363"))?;
                let kind = int_literal_kind(n);
                stack.push(Slot {
                    value: Value::Int(n as i64),
                    kind,
                });
                Ok(())
            }
            Expr::Float { value } => {
                let f = decode_float_literal(value)
                    .map_err(|m| InterpretError::new(m, span, "E363"))?;
                let kind = float_literal_kind(value, f);
                stack.push(Slot {
                    value: Value::Float(f),
                    kind,
                });
                Ok(())
            }
            Expr::Bool { value } => {
                stack.push(Slot {
                    value: Value::Bool(*value),
                    kind: 0,
                });
                Ok(())
            }
            Expr::String { value } => {
                let bytes = decode_string_literal(value)
                    .map_err(|m| InterpretError::new(m, span, "E363"))?;
                let handle = runtime::yarrow_str_new(bytes.as_ptr() as u64, bytes.len() as u64);
                stack.push(Slot {
                    value: Value::Str {
                        handle,
                        owned: true,
                    },
                    kind: 16,
                });
                Ok(())
            }
            Expr::Rune { value } => {
                let cp =
                    decode_rune_literal(value).map_err(|m| InterpretError::new(m, span, "E363"))?;
                stack.push(Slot {
                    value: Value::Int(i64::from(cp)),
                    kind: 11,
                });
                Ok(())
            }
            Expr::Array(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    self.eval_expr(e, stack, frame, module, caller_fq, span)?;
                    out.push(self.pop(stack, span, "array element")?.value);
                }
                let count = out.len() as u64;
                let elem_kind = out.first().map(infer_value_kind).unwrap_or(4);
                let kind = 0x60 | (elem_kind << 8) | (count << 40);
                stack.push(Slot {
                    value: Value::Array(out),
                    kind,
                });
                Ok(())
            }
            Expr::Variable { name } => {
                let Some(local) = frame.locals.get(name) else {
                    // A bare name with no local binding but a registered type
                    // is a type value (e.g. the `Point` in `p typeof Point ==`).
                    if let Some(kind) = self.named_type_kind_code(name) {
                        stack.push(Slot {
                            value: Value::Int(kind as i64),
                            kind: 4,
                        });
                        return Ok(());
                    }
                    return Err(InterpretError::new(
                        format!("unknown variable '{name}'"),
                        span,
                        "E320",
                    ));
                };
                stack.push(Slot {
                    value: local.value.clone_for_stack(),
                    kind: local.kind,
                });
                Ok(())
            }
            Expr::Member { base, member } => {
                if let Expr::Variable { name } = base.as_ref()
                    && self.is_std_loop_alias(frame, name)
                {
                    match member.as_str() {
                        "break" | "continue" => {
                            if frame.loops.is_empty() {
                                return Err(InterpretError::new(
                                    "loop.break / loop.continue outside a for",
                                    span,
                                    "E393",
                                ));
                            }
                            frame.pending_flow = Some(if member == "break" {
                                Flow::Break
                            } else {
                                Flow::Continue
                            });
                            return Ok(());
                        }
                        "value" => {
                            let loop_ctx = frame.loops.last().ok_or_else(|| {
                                InterpretError::new(
                                    "loop.value / loop.index outside a for",
                                    span,
                                    "E393",
                                )
                            })?;
                            let slot = loop_ctx.value.as_ref().ok_or_else(|| {
                                InterpretError::new(
                                    "loop.value is not available in this for",
                                    span,
                                    "E393",
                                )
                            })?;
                            stack.push(Slot {
                                value: slot.value.clone_for_stack(),
                                kind: slot.kind,
                            });
                            return Ok(());
                        }
                        "index" => {
                            let loop_ctx = frame.loops.last().ok_or_else(|| {
                                InterpretError::new(
                                    "loop.value / loop.index outside a for",
                                    span,
                                    "E393",
                                )
                            })?;
                            let idx = loop_ctx.index.ok_or_else(|| {
                                InterpretError::new(
                                    "loop.index is not available in this for",
                                    span,
                                    "E393",
                                )
                            })?;
                            stack.push(Slot {
                                value: Value::Int(idx),
                                kind: 4,
                            });
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                // `Color.GREEN` / `AppError.NOT_FOUND` / `error.TAG`
                if let Expr::Variable { name } = base.as_ref() {
                    if name == "error" {
                        let tag = self.error_tags.get(member).copied().ok_or_else(|| {
                            InterpretError::new(
                                format!("unknown error member '{member}'"),
                                span,
                                "E320",
                            )
                        })?;
                        stack.push(Slot {
                            value: Value::Int(i64::from(tag)),
                            kind: KIND_ERROR,
                        });
                        return Ok(());
                    }
                    if let Some(tag) = self
                        .error_type_ids
                        .get(name)
                        .and_then(|id| self.error_types.get(*id as usize))
                        .and_then(|(_, ms)| ms.iter().find(|(n, _)| n == member).map(|(_, t)| *t))
                    {
                        stack.push(Slot {
                            value: Value::Int(i64::from(tag)),
                            kind: KIND_ERROR,
                        });
                        return Ok(());
                    }
                    if let Some(v) = self.enum_members.get(&(name.clone(), member.clone())) {
                        // PLAN 3.1: the slot's kind identifies the enum, not
                        // the carrier; the stored bits stay the discriminant.
                        let kind = self
                            .enum_ids
                            .get(name)
                            .map_or(4, |id| KIND_ENUM | ((*id as u64) << 8));
                        stack.push(Slot {
                            value: Value::Int(*v),
                            kind,
                        });
                        return Ok(());
                    }
                    // PLAN 3.2: a string-carried enum member pushes an owned
                    // clone of its interned member-name string, with the
                    // enum's own kind code on the slot.
                    if let Some(id) = self.enum_ids.get(name).copied()
                        && let Some(strings) =
                            self.enums.get(id as usize).and_then(|e| e.strings.as_ref())
                    {
                        let interned = strings
                            .iter()
                            .find(|(n, _)| n == member)
                            .map(|(_, h)| *h)
                            .ok_or_else(|| {
                            InterpretError::new(
                                format!("unknown enum member '{name}.{member}'"),
                                span,
                                "E320",
                            )
                        })?;
                        let handle = runtime::yarrow_value_clone(interned, KIND_STRING);
                        stack.push(Slot {
                            value: Value::Str {
                                handle,
                                owned: true,
                            },
                            kind: KIND_ENUM | ((id as u64) << 8),
                        });
                        return Ok(());
                    }
                }
                // Struct field access: evaluate base, load field (heap struct or
                // `pointer<Struct>` raw address).
                self.eval_expr(base, stack, frame, module, caller_fq, span)?;
                let base_slot = self.pop(stack, span, "field access")?;
                let (handle, sid) = match self.struct_base_addr(&base_slot, span) {
                    Ok(v) => v,
                    Err(e) => {
                        base_slot.drop_owned();
                        return Err(e);
                    }
                };
                let info = self.structs.get(sid as usize).ok_or_else(|| {
                    InterpretError::new(format!("struct has no field '{member}'"), span, "E340")
                })?;
                let field = info
                    .fields
                    .iter()
                    .find(|f| f.name == *member)
                    .cloned()
                    .ok_or_else(|| {
                        InterpretError::new(format!("struct has no field '{member}'"), span, "E340")
                    })?;
                // PLAN 5.4.2: private fields are readable only from the
                // declaring unit.
                if let Err(e) = self.check_field_visibility(info, &field, module, span) {
                    base_slot.drop_owned();
                    return Err(e);
                }
                let bits = read_bits_at(handle, field.offset, field.size);
                base_slot.drop_owned();
                stack.push(self.bits_to_slot_kind(bits, field.kind, false));
                Ok(())
            }
            Expr::TypeValue { name } => {
                // Primitives keep their direct kind code; other names resolve
                // through the interpreter tables (struct, union, enum, error).
                let kind = self.named_type_kind_code(name).ok_or_else(|| {
                    InterpretError::new(format!("unknown type value '{name}'"), span, "E302")
                })?;
                stack.push(Slot {
                    value: Value::Int(kind as i64),
                    kind: 4,
                });
                Ok(())
            }
            Expr::Typeof { inner } => {
                self.eval_expr(inner, stack, frame, module, caller_fq, span)?;
                self.apply_typeof(stack, span)
            }
            Expr::ApplyTypeof => self.apply_typeof(stack, span),
            Expr::Seq(elems) => {
                for (e, s) in elems {
                    self.eval_expr(e, stack, frame, module, caller_fq, *s)?;
                }
                Ok(())
            }
            Expr::ApplyBin(op) => {
                let r = self.pop(stack, span, "operator")?;
                let l = self.pop(stack, span, "operator")?;
                let out = self.eval_bin(*op, l, r, span)?;
                stack.push(out);
                Ok(())
            }
            Expr::Binary { op, left, right } => {
                self.eval_expr(left, stack, frame, module, caller_fq, span)?;
                self.eval_expr(right, stack, frame, module, caller_fq, span)?;
                let r = self.pop(stack, span, "operator")?;
                let l = self.pop(stack, span, "operator")?;
                let out = self.eval_bin(*op, l, r, span)?;
                stack.push(out);
                Ok(())
            }
            Expr::ApplyUn(op) => {
                let v = self.pop(stack, span, "unary")?;
                self.eval_un(*op, v, stack, span)
            }
            Expr::Unary { op, operand } => {
                self.eval_expr(operand, stack, frame, module, caller_fq, span)?;
                let v = self.pop(stack, span, "unary")?;
                self.eval_un(*op, v, stack, span)
            }
            Expr::StackOp(op) => self.eval_stack_op(*op, stack, span),
            Expr::Builtin { name } => self.eval_builtin(name, stack, span),
            Expr::Call { target } => self.eval_call(target, stack, frame, module, caller_fq, span),
            Expr::List(elems) => {
                // Untyped list literal: element kind from first element.
                let mut values = Vec::with_capacity(elems.len());
                for e in elems {
                    self.eval_expr(e, stack, frame, module, caller_fq, span)?;
                    values.push(self.pop(stack, span, "list element")?);
                }
                let elem_kind = values.first().map(|s| s.kind).unwrap_or(4);
                let elem_size = kind_elem_size(elem_kind);
                let handle = runtime::yarrow_list_new(u64::from(elem_size));
                for v in values {
                    let bits = v.value.as_bits().unwrap_or(0);
                    // Transfer ownership into the list; don't free the slot.
                    match v.value {
                        Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                        Value::Array(_) => {}
                        other => other.drop_owned(),
                    }
                    runtime::yarrow_list_push(handle, bits);
                }
                stack.push(Slot {
                    value: Value::Heap {
                        handle,
                        owned: true,
                    },
                    kind: KIND_LIST | (elem_kind << 8),
                });
                Ok(())
            }
            Expr::Map(pairs) => {
                let mut evaluated = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    self.eval_expr(k, stack, frame, module, caller_fq, span)?;
                    let key = self.pop(stack, span, "map key")?;
                    self.eval_expr(v, stack, frame, module, caller_fq, span)?;
                    let val = self.pop(stack, span, "map value")?;
                    evaluated.push((key, val));
                }
                let key_kind = evaluated
                    .first()
                    .map(|(k, _)| k.kind)
                    .unwrap_or(KIND_STRING);
                let val_kind = evaluated.first().map(|(_, v)| v.kind).unwrap_or(4);
                let keys_string = u64::from(key_kind == KIND_STRING);
                let handle = runtime::yarrow_map_new(keys_string);
                for (k, v) in evaluated {
                    let kb = k.value.as_bits().unwrap_or(0);
                    let vb = v.value.as_bits().unwrap_or(0);
                    match k.value {
                        Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                        other => other.drop_owned(),
                    }
                    match v.value {
                        Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                        other => other.drop_owned(),
                    }
                    runtime::yarrow_map_insert(handle, kb, vb);
                }
                stack.push(Slot {
                    value: Value::Heap {
                        handle,
                        owned: true,
                    },
                    kind: KIND_MAP | (key_kind << 8) | (val_kind << 40),
                });
                Ok(())
            }
            Expr::EmptyMapOrStruct => Err(InterpretError::new(
                "empty `{}` requires a typed context (e.g., variable declaration)",
                span,
                "E395",
            )),
            Expr::ApplyBorrow | Expr::Borrow { .. } => {
                let s = self.pop(stack, span, "'borrow'")?;
                // Borrow: keep handle, mark unowned so drop does not free.
                let borrowed = Slot {
                    value: match s.value {
                        Value::Heap { handle, .. } => Value::Heap {
                            handle,
                            owned: false,
                        },
                        Value::Str { handle, .. } => Value::Str {
                            handle,
                            owned: false,
                        },
                        Value::Array(elems) => {
                            Value::Array(elems.iter().map(|e| e.clone_for_stack()).collect())
                        }
                        other => other,
                    },
                    kind: s.kind,
                };
                // Original owned value stays in its variable; stack borrow is unowned.
                stack.push(borrowed);
                Ok(())
            }
            Expr::ApplyLoad => self.eval_ptr_load(stack, span),
            Expr::Load { inner } => {
                self.eval_expr(inner, stack, frame, module, caller_fq, span)?;
                self.eval_ptr_load(stack, span)
            }
            Expr::Store { addr, value } => {
                self.eval_expr(addr, stack, frame, module, caller_fq, span)?;
                self.eval_expr(value, stack, frame, module, caller_fq, span)?;
                let val = self.pop(stack, span, "'store'")?;
                let addr_slot = self.pop(stack, span, "'store'")?;
                if (addr_slot.kind & 0xff) != KIND_PTR {
                    addr_slot.drop_owned();
                    val.drop_owned();
                    return Err(InterpretError::new(
                        "'store' requires a pointer target",
                        span,
                        "E341",
                    ));
                }
                let pointee = addr_slot.kind >> 8;
                let addr = match addr_slot.value.as_bits() {
                    Some(a) => a,
                    None => {
                        addr_slot.drop_owned();
                        val.drop_owned();
                        return Err(InterpretError::new(
                            "'store' requires a pointer address",
                            span,
                            "E341",
                        ));
                    }
                };
                let bits = val.value.as_bits().unwrap_or(0);
                write_bits_at(addr, 0, self.pointee_size(pointee), bits);
                match val.value {
                    Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                    other => other.drop_owned(),
                }
                addr_slot.drop_owned();
                Ok(())
            }
            Expr::Unwrap { inner } => {
                self.eval_expr(inner, stack, frame, module, caller_fq, span)?;
                self.eval_unwrap(stack, frame, span)
            }
            _ => Err(InterpretError::unsupported(
                format!("expression {expr:?}"),
                span,
            )),
        }
    }

    fn apply_typeof(&self, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        let slot = self.pop(stack, span, "'typeof'")?;
        let code = slot.kind;
        slot.drop_owned();
        stack.push(Slot {
            value: Value::Int(code as i64),
            kind: 4,
        });
        Ok(())
    }

    /// Byte size of a pointee value. PLAN 3.1: an enum pointee is sized by
    /// its carrier, which only the interpreter can resolve from the enum id.
    fn pointee_size(&self, kind: u64) -> u32 {
        if (kind & 0xff) == KIND_ENUM {
            let carrier = self
                .enums
                .get((kind >> 8) as usize)
                .map_or(3, |e| e.carrier);
            return kind_elem_size(carrier);
        }
        pointee_byte_size(kind)
    }

    /// `pointer<T> load`: pop a typed pointer and push the pointee bits.
    fn eval_ptr_load(&self, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        let addr_slot = self.pop(stack, span, "'load'")?;
        if (addr_slot.kind & 0xff) != KIND_PTR {
            addr_slot.drop_owned();
            return Err(InterpretError::new(
                "'load' requires a pointer",
                span,
                "E341",
            ));
        }
        let pointee = addr_slot.kind >> 8;
        let addr = match addr_slot.value.as_bits() {
            Some(a) => a,
            None => {
                addr_slot.drop_owned();
                return Err(InterpretError::new(
                    "'load' requires a pointer address",
                    span,
                    "E341",
                ));
            }
        };
        let bits = read_bits_at(addr, 0, self.pointee_size(pointee));
        addr_slot.drop_owned();
        stack.push(self.bits_to_slot_kind(bits, pointee, false));
        Ok(())
    }

    /// Field-visibility gate (PLAN 5.4): a non-`public` field is only
    /// accessible from the unit that declared the struct. Same-unit access
    /// stays legal.
    fn check_field_visibility(
        &self,
        info: &StructInfo,
        field: &FieldInfo,
        module: Option<&str>,
        span: Span,
    ) -> IResult<()> {
        if field.public || info.module.as_deref() == module {
            return Ok(());
        }
        Err(InterpretError::new(
            format!(
                "field '{}.{}' is private to {}",
                info.name,
                field.name,
                Self::unit_label(info.module.as_deref()),
            ),
            span,
            "E397",
        ))
    }

    /// Address and struct id for field access/`set` on a heap struct or
    /// `pointer<Struct>` raw address.
    fn struct_base_addr(&self, base: &Slot, span: Span) -> IResult<(u64, u32)> {
        match base.kind & 0xff {
            KIND_STRUCT => {
                let sid = (base.kind >> 8) as u32;
                let handle = match &base.value {
                    Value::Heap { handle, .. } => *handle,
                    _ => {
                        return Err(InterpretError::new(
                            "struct field access requires a heap handle",
                            span,
                            "E340",
                        ));
                    }
                };
                Ok((handle, sid))
            }
            KIND_UNION => {
                let uid = (base.kind >> 8) as u32;
                let handle = match &base.value {
                    Value::Heap { handle, .. } => *handle,
                    _ => {
                        return Err(InterpretError::new(
                            "union field access requires a heap handle",
                            span,
                            "E340",
                        ));
                    }
                };
                Ok((handle, uid))
            }
            KIND_PTR => {
                let pointee = base.kind >> 8;
                if (pointee & 0xff) != KIND_STRUCT && (pointee & 0xff) != KIND_UNION {
                    return Err(InterpretError::new(
                        "cannot access field through a non-struct/non-union pointer",
                        span,
                        "E340",
                    ));
                }
                let sid = (pointee >> 8) as u32;
                let addr = match base.value.as_bits() {
                    Some(a) => a,
                    None => {
                        return Err(InterpretError::new(
                            "pointer field access requires an address",
                            span,
                            "E340",
                        ));
                    }
                };
                Ok((addr, sid))
            }
            _ => Err(InterpretError::new(
                "cannot access field on non-struct/non-union",
                span,
                "E340",
            )),
        }
    }

    fn eval_call(
        &mut self,
        target: &Expr,
        stack: &mut Vec<Slot>,
        frame: &Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<()> {
        let name = self.resolve_call_name(target, frame, module, caller_fq, span)?;
        if self.is_std_intrinsic(&name) {
            return self.eval_std_intrinsic(&name, stack, span);
        }
        let entry = self.funcs.get(&name).cloned().ok_or_else(|| {
            InterpretError::new(format!("unknown function '{name}'"), span, "E330")
        })?;
        // Privacy gate (PLAN 2.1.2 / 2.1.4): a function or method is only
        // reachable from outside its own unit when it is `public`. Same-unit
        // calls (including private helpers and private methods) pass.
        if entry.module.as_deref() != module && !self.public_funcs.contains(&entry.fq) {
            return Err(InterpretError::new(
                format!(
                    "function '{}' is private to its unit and not exported",
                    entry.fq
                ),
                span,
                "E330",
            ));
        }
        let n = entry.function.params.len();
        let arg_start = stack.len().saturating_sub(n);
        let mut to_free = Vec::new();
        for s in &stack[arg_start..] {
            if let Value::Str {
                handle,
                owned: true,
            } = &s.value
            {
                to_free.push(*handle);
            }
        }
        for s in &mut stack[arg_start..] {
            if let Value::Str { owned, .. } = &mut s.value {
                *owned = false;
            }
        }
        self.call_function(&entry, stack)?;
        for handle in to_free {
            free_value(handle, KIND_STRING);
        }
        Ok(())
    }

    fn resolve_call_name(
        &self,
        target: &Expr,
        frame: &Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<String> {
        match target {
            Expr::Variable { name } => {
                let nested = format!("{caller_fq}::{name}");
                if self.funcs.contains_key(&nested) {
                    return Ok(nested);
                }
                if let Some(mod_path) = module {
                    let fq = format!("{mod_path}::{name}");
                    if self.funcs.contains_key(&fq) {
                        return Ok(fq);
                    }
                }
                if let Some(plain) = frame.local_plain_funcs.get(name) {
                    return Ok(plain.clone());
                }
                if let Some(plain) = self.plain_funcs.get(name) {
                    return Ok(plain.clone());
                }
                Ok(name.clone())
            }
            Expr::Member { base, member } => {
                if let Expr::Variable { name } = base.as_ref() {
                    // Function-scoped `require` bindings shadow the global
                    // ones (PLAN 2.6).
                    let path = frame
                        .local_aliases
                        .get(name)
                        .or_else(|| self.aliases.get(name));
                    if let Some(path) = path {
                        let item = if frame.local_aliases.contains_key(name) {
                            frame.local_item_aliases.get(name)
                        } else {
                            self.item_aliases.get(name)
                        };
                        if let Some(item) = item
                            && item != member
                        {
                            return Err(InterpretError::new(
                                format!("module '{path}' only exports '{item}' (not '{member}')"),
                                span,
                                "E330",
                            ));
                        }
                        return Ok(format!("{path}::{member}"));
                    }
                }
                // Method call: resolve from the base variable's struct type.
                if let Expr::Variable { name } = base.as_ref()
                    && let Some(slot) = frame.locals.get(name)
                    && (slot.kind & 0xff) == KIND_STRUCT
                {
                    let sid = (slot.kind >> 8) as u32;
                    if let Some(info) = self.structs.get(sid as usize) {
                        let method = format!("{}::{member}", info.name);
                        if self.funcs.contains_key(&method) {
                            return Ok(method);
                        }
                    }
                }
                Err(InterpretError::unsupported(
                    "method call / complex call target",
                    span,
                ))
            }
            _ => Err(InterpretError::new(
                "'call' target must be a function name",
                span,
                "E329",
            )),
        }
    }

    fn is_std_intrinsic(&self, fq: &str) -> bool {
        matches!(
            fq,
            "std.region::create"
                | "std.region::put"
                | "std.region::free"
                | "std.list::push_last"
                | "std.list::len"
                | "std.list::get"
                | "std.list::put"
                | "std.map::len"
                | "std.map::get"
                | "std.map::put"
        )
    }

    fn eval_std_intrinsic(&mut self, fq: &str, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        match fq {
            "std.region::create" => {
                let handle = runtime::yarrow_region_new();
                stack.push(Slot {
                    value: Value::Int(handle as i64),
                    kind: 4,
                });
                Ok(())
            }
            "std.region::free" => {
                let region = self.pop(stack, span, "region.free")?;
                let handle = match region.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'region.free' requires a region handle",
                            span,
                            "E372",
                        ));
                    }
                };
                runtime::yarrow_region_free(handle);
                Ok(())
            }
            "std.region::put" => {
                let region = self.pop(stack, span, "region.put")?;
                let value = self.pop(stack, span, "region.put")?;
                let region_handle = match region.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'region.put' requires a region handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let (val_handle, kind) = match &value.value {
                    Value::Str { handle, .. } => (*handle, KIND_STRING),
                    Value::Heap { handle, .. } => (*handle, value.kind),
                    other => {
                        let msg = format!(
                            "'region.put' requires a reference, struct, array, string or container, got {other:?}"
                        );
                        value.drop_owned();
                        return Err(InterpretError::new(msg, span, "E372"));
                    }
                };
                // Region takes ownership; stack must not free the value.
                match &value.value {
                    Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                    other => {
                        // Non-owned borrow from a local: still attach the handle.
                        let _ = other;
                    }
                }
                runtime::yarrow_region_register(val_handle, kind, region_handle);
                // Push a borrow of the attached value (matches JIT put_region).
                stack.push(Slot {
                    value: match value.value {
                        Value::Str { handle, .. } => Value::Str {
                            handle,
                            owned: false,
                        },
                        Value::Heap { handle, .. } => Value::Heap {
                            handle,
                            owned: false,
                        },
                        other => other,
                    },
                    kind: value.kind,
                });
                Ok(())
            }
            "std.list::push_last" => {
                let value = self.pop(stack, span, "list.push_last")?;
                let list = self.pop(stack, span, "list.push_last")?;
                if (list.kind & 0xff) != KIND_LIST {
                    list.drop_owned();
                    value.drop_owned();
                    return Err(InterpretError::new(
                        "'list.push_last' requires a list",
                        span,
                        "E372",
                    ));
                }
                let handle = match list.value {
                    Value::Heap { handle, .. } => handle,
                    _ => {
                        list.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'list.push_last' requires a list handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let bits = value.value.as_bits().unwrap_or(0);
                // Transfer ownership of heap bits into the list storage.
                match value.value {
                    Value::Str { owned: true, .. }
                    | Value::Heap { owned: true, .. }
                    | Value::Array(_) => {}
                    other => other.drop_owned(),
                }
                runtime::yarrow_list_push(handle, bits);
                stack.push(list);
                Ok(())
            }
            "std.list::len" => {
                let list = self.pop(stack, span, "list.len")?;
                let handle = match list.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'list.len' requires a list",
                            span,
                            "E372",
                        ));
                    }
                };
                let len = runtime::yarrow_list_len(handle) as i64;
                list.drop_owned();
                stack.push(Slot {
                    value: Value::Int(len),
                    kind: 4,
                });
                Ok(())
            }
            "std.list::get" => {
                let index = self.pop(stack, span, "list.get")?;
                let list = self.pop(stack, span, "list.get")?;
                if (list.kind & 0xff) != KIND_LIST {
                    list.drop_owned();
                    index.drop_owned();
                    return Err(InterpretError::new(
                        "'list.get' requires a list",
                        span,
                        "E372",
                    ));
                }
                let handle = match list.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        index.drop_owned();
                        return Err(InterpretError::new(
                            "'list.get' requires a list handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let idx = match index.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        list.drop_owned();
                        return Err(InterpretError::new(
                            "'list.get' requires an integer index",
                            span,
                            "E372",
                        ));
                    }
                };
                let len = runtime::yarrow_list_len(handle) as i64;
                if idx < 0 || idx >= len {
                    list.drop_owned();
                    return Err(InterpretError::new(
                        format!("'list.get' index {idx} out of range (list length {len})"),
                        span,
                        "E372",
                    ));
                }
                let elem_kind = list.kind >> 8;
                let bits = runtime::yarrow_list_get(handle, idx as u64);
                list.drop_owned();
                stack.push(Slot {
                    kind: elem_kind,
                    value: match elem_kind & 0xff {
                        KIND_STRING => Value::Str {
                            handle: bits,
                            owned: false,
                        },
                        KIND_STRUCT | KIND_UNION | KIND_LIST | KIND_MAP => Value::Heap {
                            handle: bits,
                            owned: false,
                        },
                        _ => Value::Int(bits as i64),
                    },
                });
                Ok(())
            }
            "std.list::put" => {
                let value = self.pop(stack, span, "list.put")?;
                let index = self.pop(stack, span, "list.put")?;
                let list = self.pop(stack, span, "list.put")?;
                if (list.kind & 0xff) != KIND_LIST {
                    list.drop_owned();
                    index.drop_owned();
                    value.drop_owned();
                    return Err(InterpretError::new(
                        "'list.put' requires a list",
                        span,
                        "E372",
                    ));
                }
                let handle = match list.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        index.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'list.put' requires a list handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let idx = match index.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        list.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'list.put' requires an integer index",
                            span,
                            "E372",
                        ));
                    }
                };
                let len = runtime::yarrow_list_len(handle) as i64;
                if idx < 0 || idx >= len {
                    list.drop_owned();
                    value.drop_owned();
                    return Err(InterpretError::new(
                        format!("'list.put' index {idx} out of range (list length {len})"),
                        span,
                        "E372",
                    ));
                }
                let elem_kind = list.kind >> 8;
                let bits = value.value.as_bits().unwrap_or(0);
                // The list owns its elements: release the one being replaced,
                // unless the new value is that same element (borrow put back).
                let old = runtime::yarrow_list_get(handle, idx as u64);
                if old != 0 && old != bits {
                    free_value(old, elem_kind);
                }
                // Transfer ownership of heap bits into the list storage.
                match value.value {
                    Value::Str { owned: true, .. }
                    | Value::Heap { owned: true, .. }
                    | Value::Array(_) => {}
                    other => other.drop_owned(),
                }
                runtime::yarrow_list_set(handle, idx as u64, bits);
                stack.push(list);
                Ok(())
            }
            "std.map::len" => {
                let map = self.pop(stack, span, "map.len")?;
                if (map.kind & 0xff) != KIND_MAP {
                    map.drop_owned();
                    return Err(InterpretError::new(
                        "'map.len' requires a hashmap",
                        span,
                        "E372",
                    ));
                }
                let handle = match map.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'map.len' requires a hashmap handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let len = runtime::yarrow_map_len(handle) as i64;
                map.drop_owned();
                stack.push(Slot {
                    value: Value::Int(len),
                    kind: 4,
                });
                Ok(())
            }
            "std.map::get" => {
                let key = self.pop(stack, span, "map.get")?;
                let map = self.pop(stack, span, "map.get")?;
                if (map.kind & 0xff) != KIND_MAP {
                    map.drop_owned();
                    key.drop_owned();
                    return Err(InterpretError::new(
                        "'map.get' requires a hashmap",
                        span,
                        "E372",
                    ));
                }
                let handle = match map.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        key.drop_owned();
                        return Err(InterpretError::new(
                            "'map.get' requires a hashmap handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let key_kind = (map.kind >> 8) & 0xff;
                let key_bits = match (&key.value, key_kind) {
                    (Value::Str { handle, .. }, KIND_STRING) => *handle,
                    (Value::Int(n), k) if k != KIND_STRING => *n as u64,
                    _ => {
                        map.drop_owned();
                        key.drop_owned();
                        return Err(InterpretError::new(
                            "'map.get' key does not match the map key type",
                            span,
                            "E372",
                        ));
                    }
                };
                let mut found = 0u8;
                let bits = unsafe { runtime::yarrow_map_get(handle, key_bits, &mut found) };
                if found == 0 {
                    map.drop_owned();
                    key.drop_owned();
                    return Err(InterpretError::new(
                        "'map.get' key not found in map",
                        span,
                        "E372",
                    ));
                }
                let val_kind = map.kind >> 40;
                map.drop_owned();
                key.drop_owned();
                stack.push(Slot {
                    kind: val_kind,
                    value: match val_kind & 0xff {
                        KIND_STRING => Value::Str {
                            handle: bits,
                            owned: false,
                        },
                        KIND_STRUCT | KIND_UNION | KIND_LIST | KIND_MAP => Value::Heap {
                            handle: bits,
                            owned: false,
                        },
                        // PLAN 3.2: a string-carried enum value is a heap
                        // string handle.
                        t if t == KIND_ENUM && self.enum_carrier(val_kind) == KIND_STRING => {
                            Value::Str {
                                handle: bits,
                                owned: false,
                            }
                        }
                        _ => Value::Int(bits as i64),
                    },
                });
                Ok(())
            }
            "std.map::put" => {
                let value = self.pop(stack, span, "map.put")?;
                let key = self.pop(stack, span, "map.put")?;
                let map = self.pop(stack, span, "map.put")?;
                if (map.kind & 0xff) != KIND_MAP {
                    map.drop_owned();
                    key.drop_owned();
                    value.drop_owned();
                    return Err(InterpretError::new(
                        "'map.put' requires a hashmap",
                        span,
                        "E372",
                    ));
                }
                let handle = match map.value {
                    Value::Heap { handle, .. } => handle,
                    other => {
                        other.drop_owned();
                        key.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'map.put' requires a hashmap handle",
                            span,
                            "E372",
                        ));
                    }
                };
                let key_kind = (map.kind >> 8) & 0xff;
                let val_kind = map.kind >> 40;
                let key_bits = match (&key.value, key_kind) {
                    (Value::Str { handle, .. }, KIND_STRING) => *handle,
                    (Value::Int(n), k) if k != KIND_STRING => *n as u64,
                    _ => {
                        map.drop_owned();
                        key.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'map.put' key does not match the map key type",
                            span,
                            "E372",
                        ));
                    }
                };
                let val_bits = value.value.as_bits().unwrap_or(0);
                // The map owns its values: release the one being replaced,
                // unless the new value is that same value (borrow put back).
                let mut found = 0u8;
                let old = unsafe { runtime::yarrow_map_get(handle, key_bits, &mut found) };
                if found != 0 && old != 0 && old != val_bits {
                    free_value(old, val_kind);
                }
                // Transfer ownership of heap bits into the map storage.
                match key.value {
                    Value::Str { owned: true, .. }
                    | Value::Heap { owned: true, .. }
                    | Value::Array(_) => {}
                    other => other.drop_owned(),
                }
                match value.value {
                    Value::Str { owned: true, .. }
                    | Value::Heap { owned: true, .. }
                    | Value::Array(_) => {}
                    other => other.drop_owned(),
                }
                runtime::yarrow_map_insert(handle, key_bits, val_bits);
                stack.push(map);
                Ok(())
            }
            other => Err(InterpretError::unsupported(
                format!("std intrinsic {other}"),
                span,
            )),
        }
    }

    fn is_std_loop_alias(&self, frame: &Frame, alias: &str) -> bool {
        frame
            .local_aliases
            .get(alias)
            .or_else(|| self.aliases.get(alias))
            .is_some_and(|p| p == "std.loop")
    }

    fn eval_builtin(&mut self, name: &str, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        match name {
            "print" => {
                let v = self.pop(stack, span, "@print")?;
                match v.value {
                    Value::Str { handle, owned } => {
                        runtime::yarrow_print_str(handle);
                        if owned {
                            free_value(handle, KIND_STRING);
                        }
                    }
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@print' requires a string",
                            span,
                            "E372",
                        ));
                    }
                }
                Ok(())
            }
            "print_newline" => {
                runtime::yarrow_print_newline();
                Ok(())
            }
            "alloc" => {
                let size = self.pop(stack, span, "@alloc")?;
                let n = match size.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@alloc' requires an integer size",
                            span,
                            "E372",
                        ));
                    }
                };
                let ptr = runtime::yarrow_alloc(n);
                stack.push(Slot {
                    value: Value::Int(ptr as i64),
                    kind: 4,
                });
                Ok(())
            }
            "free" => {
                let ptr = self.pop(stack, span, "@free")?;
                let addr = match ptr.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@free' requires a pointer address",
                            span,
                            "E372",
                        ));
                    }
                };
                runtime::yarrow_free(addr);
                Ok(())
            }
            "load" => {
                // Raw `@load`: read a 64-bit word at an address.
                let addr = self.pop(stack, span, "@load")?;
                let a = match addr.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@load' requires a pointer address",
                            span,
                            "E372",
                        ));
                    }
                };
                let bits = read_bits_at(a, 0, 8);
                stack.push(Slot {
                    value: Value::Int(bits as i64),
                    kind: 4,
                });
                Ok(())
            }
            "store" => {
                // Raw `@store`: write a 64-bit word at an address.
                let value = self.pop(stack, span, "@store")?;
                let addr = self.pop(stack, span, "@store")?;
                let a = match addr.value {
                    Value::Int(n) => n as u64,
                    other => {
                        other.drop_owned();
                        value.drop_owned();
                        return Err(InterpretError::new(
                            "'@store' requires a pointer address",
                            span,
                            "E372",
                        ));
                    }
                };
                let bits = value.value.as_bits().unwrap_or(0);
                write_bits_at(a, 0, 8, bits);
                value.drop_owned();
                Ok(())
            }
            "fs_open" => {
                let mode = self.pop(stack, span, "@fs_open")?;
                let path = self.pop(stack, span, "@fs_open")?;
                let mode_n = match mode.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        path.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_open' requires a rune mode",
                            span,
                            "E372",
                        ));
                    }
                };
                let (handle, owned) = match path.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_open' requires a string path",
                            span,
                            "E372",
                        ));
                    }
                };
                let fd = runtime::yarrow_fs_open(handle, mode_n);
                if owned {
                    free_value(handle, KIND_STRING);
                }
                stack.push(Slot {
                    value: Value::Int(fd),
                    kind: 4,
                });
                Ok(())
            }
            "fs_close" => {
                let fd = self.pop(stack, span, "@fs_close")?;
                let n = match fd.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_close' requires an integer fd",
                            span,
                            "E372",
                        ));
                    }
                };
                let rc = runtime::yarrow_fs_close(n);
                stack.push(Slot {
                    value: Value::Int(rc),
                    kind: 4,
                });
                Ok(())
            }
            "fs_write" => {
                let content = self.pop(stack, span, "@fs_write")?;
                let fd = self.pop(stack, span, "@fs_write")?;
                let n = match fd.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        content.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_write' requires an integer fd",
                            span,
                            "E372",
                        ));
                    }
                };
                let (handle, owned) = match content.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_write' requires a string",
                            span,
                            "E372",
                        ));
                    }
                };
                let rc = runtime::yarrow_fs_write(n, handle);
                if owned {
                    free_value(handle, KIND_STRING);
                }
                stack.push(Slot {
                    value: Value::Int(rc),
                    kind: 4,
                });
                Ok(())
            }
            "fs_read" => {
                let fd = self.pop(stack, span, "@fs_read")?;
                let n = match fd.value {
                    Value::Int(n) => n,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@fs_read' requires an integer fd",
                            span,
                            "E372",
                        ));
                    }
                };
                let handle = runtime::yarrow_fs_read(n);
                stack.push(Slot {
                    value: Value::Str {
                        handle,
                        owned: true,
                    },
                    kind: KIND_STRING,
                });
                Ok(())
            }
            "fs_last_error" => {
                let code = runtime::yarrow_fs_last_error();
                stack.push(Slot {
                    value: Value::Int(code),
                    kind: 4,
                });
                Ok(())
            }
            "print_int" => {
                let v = self.pop(stack, span, "@print_int")?;
                let n = match v.value {
                    Value::Int(n) => n,
                    Value::Bool(b) => b as i64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@print_int' requires an integer",
                            span,
                            "E372",
                        ));
                    }
                };
                runtime::yarrow_print_int(n);
                Ok(())
            }
            "print_float" => {
                let v = self.pop(stack, span, "@print_float")?;
                let f = match v.value {
                    Value::Float(f) => f,
                    Value::Int(n) => n as f64,
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@print_float' requires a float",
                            span,
                            "E372",
                        ));
                    }
                };
                runtime::yarrow_print_float(f);
                Ok(())
            }
            "string_len" | "str_len" => {
                let v = self.pop(stack, span, "@string_len")?;
                let (handle, owned) = match v.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        return Err(InterpretError::new(
                            "'@string_len' requires a string",
                            span,
                            "E372",
                        ));
                    }
                };
                let len = runtime::yarrow_str_len(handle) as i64;
                if owned {
                    free_value(handle, KIND_STRING);
                }
                stack.push(Slot {
                    value: Value::Int(len),
                    kind: 4,
                });
                Ok(())
            }
            "str_join" => {
                let right = self.pop(stack, span, "@str_join")?;
                let left = self.pop(stack, span, "@str_join")?;
                let (lh, lo) = match left.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        right.drop_owned();
                        return Err(InterpretError::new(
                            "'@str_join' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let (rh, ro) = match right.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        if lo {
                            free_value(lh, KIND_STRING);
                        }
                        return Err(InterpretError::new(
                            "'@str_join' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let out = runtime::yarrow_str_join(lh, rh);
                if lo {
                    free_value(lh, KIND_STRING);
                }
                if ro {
                    free_value(rh, KIND_STRING);
                }
                stack.push(Slot {
                    value: Value::Str {
                        handle: out,
                        owned: true,
                    },
                    kind: KIND_STRING,
                });
                Ok(())
            }
            "string_join" => {
                let sep = self.pop(stack, span, "@string_join")?;
                let right = self.pop(stack, span, "@string_join")?;
                let left = self.pop(stack, span, "@string_join")?;
                let free_owned = |handle: u64, owned: bool| {
                    if owned {
                        free_value(handle, KIND_STRING);
                    }
                };
                let (lh, lo) = match left.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        right.drop_owned();
                        sep.drop_owned();
                        return Err(InterpretError::new(
                            "'@string_join' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let (rh, ro) = match right.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        free_owned(lh, lo);
                        sep.drop_owned();
                        return Err(InterpretError::new(
                            "'@string_join' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let (sh, so) = match sep.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        free_owned(lh, lo);
                        free_owned(rh, ro);
                        return Err(InterpretError::new(
                            "'@string_join' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let mid = runtime::yarrow_str_join(lh, sh);
                let out = runtime::yarrow_str_join(mid, rh);
                free_value(mid, KIND_STRING);
                free_owned(lh, lo);
                free_owned(rh, ro);
                free_owned(sh, so);
                stack.push(Slot {
                    value: Value::Str {
                        handle: out,
                        owned: true,
                    },
                    kind: KIND_STRING,
                });
                Ok(())
            }
            "str_cmp" => {
                let right = self.pop(stack, span, "@str_cmp")?;
                let left = self.pop(stack, span, "@str_cmp")?;
                let (lh, lo) = match left.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        right.drop_owned();
                        return Err(InterpretError::new(
                            "'@str_cmp' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let (rh, ro) = match right.value {
                    Value::Str { handle, owned } => (handle, owned),
                    other => {
                        other.drop_owned();
                        if lo {
                            free_value(lh, KIND_STRING);
                        }
                        return Err(InterpretError::new(
                            "'@str_cmp' requires string operands",
                            span,
                            "E372",
                        ));
                    }
                };
                let cmp = runtime::yarrow_str_cmp(lh, rh);
                if lo {
                    free_value(lh, KIND_STRING);
                }
                if ro {
                    free_value(rh, KIND_STRING);
                }
                stack.push(Slot {
                    value: Value::Int(cmp),
                    kind: 4,
                });
                Ok(())
            }
            other => Err(InterpretError::unsupported(
                format!("builtin @{other}"),
                span,
            )),
        }
    }

    fn eval_un(&self, op: UnOp, v: Slot, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        match op {
            UnOp::Not => match v.value {
                Value::Bool(b) => stack.push(Slot {
                    value: Value::Bool(!b),
                    kind: 0,
                }),
                Value::Int(n) => stack.push(Slot {
                    value: Value::Int(!n),
                    kind: v.kind,
                }),
                other => {
                    other.drop_owned();
                    return Err(InterpretError::new(
                        "'not' requires bool or integer",
                        span,
                        "E336",
                    ));
                }
            },
        }
        Ok(())
    }

    fn eval_stack_op(&mut self, op: StackOp, stack: &mut Vec<Slot>, span: Span) -> IResult<()> {
        match op {
            StackOp::Dup => {
                let v = self.peek(stack, span)?;
                // Audit-3 PLAN 1.1.2: `dup` duplicates copy values only.
                // An owned heap value (string, list, hashmap, struct,
                // union, string-carried enum) is rejected; an unowned
                // handle is a reference, which stays legal to duplicate.
                if matches!(
                    v.value,
                    Value::Str { owned: true, .. } | Value::Heap { owned: true, .. }
                ) {
                    return Err(InterpretError::new(
                        "dup applies to copy types only: borrow the value instead of duplicating it",
                        span,
                        "E371",
                    ));
                }
                let cloned = Slot {
                    value: v.value.clone_for_stack(),
                    kind: v.kind,
                };
                stack.push(cloned);
                Ok(())
            }
            StackOp::Swap => {
                if stack.len() < 2 {
                    return Err(InterpretError::new("swap requires 2 values", span, "E362"));
                }
                let n = stack.len();
                stack.swap(n - 1, n - 2);
                Ok(())
            }
            StackOp::Rot => {
                if stack.len() < 3 {
                    return Err(InterpretError::new("rot requires 3 values", span, "E362"));
                }
                let c = stack.pop().unwrap();
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(b);
                stack.push(c);
                stack.push(a);
                Ok(())
            }
            StackOp::Unrot => {
                if stack.len() < 3 {
                    return Err(InterpretError::new("unrot requires 3 values", span, "E362"));
                }
                let c = stack.pop().unwrap();
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(c);
                stack.push(a);
                stack.push(b);
                Ok(())
            }
            StackOp::Pop => {
                let v = self.pop(stack, span, "pop")?;
                v.drop_owned();
                Ok(())
            }
            StackOp::Drop => {
                while let Some(v) = stack.pop() {
                    v.drop_owned();
                }
                Ok(())
            }
        }
    }
    fn eval_bin(&mut self, op: BinOp, l: Slot, r: Slot, span: Span) -> IResult<Slot> {
        use BinOp::*;
        if op == Concat {
            let (lh, lo) = match l.value {
                Value::Str { handle, owned } => (handle, owned),
                other => {
                    other.drop_owned();
                    r.drop_owned();
                    return Err(InterpretError::new(
                        "'~' requires string operands",
                        span,
                        "E335",
                    ));
                }
            };
            let (rh, ro) = match r.value {
                Value::Str { handle, owned } => (handle, owned),
                other => {
                    other.drop_owned();
                    if lo {
                        free_value(lh, KIND_STRING);
                    }
                    return Err(InterpretError::new(
                        "'~' requires string operands",
                        span,
                        "E335",
                    ));
                }
            };
            let out = runtime::yarrow_str_join(lh, rh);
            if lo {
                free_value(lh, KIND_STRING);
            }
            if ro {
                free_value(rh, KIND_STRING);
            }
            return Ok(Slot {
                value: Value::Str {
                    handle: out,
                    owned: true,
                },
                kind: 16,
            });
        }

        if matches!(op, Eq | Ne)
            && matches!(l.value, Value::Str { .. })
            && matches!(r.value, Value::Str { .. })
        {
            let (lh, lo) = match l.value {
                Value::Str { handle, owned } => (handle, owned),
                _ => unreachable!(),
            };
            let (rh, ro) = match r.value {
                Value::Str { handle, owned } => (handle, owned),
                _ => unreachable!(),
            };
            let eq = runtime::yarrow_str_cmp(lh, rh) == 0;
            if lo {
                free_value(lh, KIND_STRING);
            }
            if ro {
                free_value(rh, KIND_STRING);
            }
            return Ok(Slot {
                value: Value::Bool(if op == Eq { eq } else { !eq }),
                kind: 0,
            });
        }

        match (&l.value, &r.value, op) {
            (Value::Float(_), Value::Float(_), _)
            | (Value::Float(_), Value::Int(_), _)
            | (Value::Int(_), Value::Float(_), _) => {
                let a = match l.value {
                    Value::Float(f) => f,
                    Value::Int(n) => n as f64,
                    _ => unreachable!(),
                };
                let b = match r.value {
                    Value::Float(f) => f,
                    Value::Int(n) => n as f64,
                    _ => unreachable!(),
                };
                self.eval_float_bin(op, a, b, span)
            }
            (Value::Int(a), Value::Int(b), Div) => Ok(Slot {
                value: Value::Float((*a as f64) / (*b as f64)),
                kind: 14,
            }),
            (Value::Int(a), Value::Int(b), _) => self.eval_int_bin(op, *a, *b, l.kind, span),
            (Value::Bool(a), Value::Bool(b), _) => self.eval_bool_bin(op, *a, *b, span),
            (Value::Bool(a), Value::Int(b), And | Or | Xor) => {
                self.eval_int_bin(op, *a as i64, *b, 4, span)
            }
            (Value::Int(a), Value::Bool(b), And | Or | Xor) => {
                self.eval_int_bin(op, *a, *b as i64, l.kind, span)
            }
            _ => {
                l.drop_owned();
                r.drop_owned();
                Err(InterpretError::new(
                    format!("incompatible operands for {op:?}"),
                    span,
                    "E333",
                ))
            }
        }
    }

    fn eval_int_bin(&self, op: BinOp, a: i64, b: i64, kind: u64, span: Span) -> IResult<Slot> {
        use BinOp::*;
        Ok(match op {
            Plus => Slot {
                value: Value::Int(a.wrapping_add(b)),
                kind,
            },
            Minus => Slot {
                value: Value::Int(a.wrapping_sub(b)),
                kind,
            },
            Mul => Slot {
                value: Value::Int(a.wrapping_mul(b)),
                kind,
            },
            Mod => {
                if b == 0 {
                    return Err(InterpretError::new(
                        "integer remainder '%' by zero",
                        span,
                        "E337",
                    ));
                }
                let r = a.wrapping_rem(b);
                // Remainder consistent with floor division: it takes the
                // divisor's sign, so `a == (a // b) * b + a % b` holds.
                let r = if r != 0 && ((r < 0) != (b < 0)) {
                    r.wrapping_add(b)
                } else {
                    r
                };
                Slot {
                    value: Value::Int(r),
                    kind,
                }
            }
            Fdiv => {
                if b == 0 {
                    return Err(InterpretError::new(
                        "integer floor division '//' by zero",
                        span,
                        "E337",
                    ));
                }
                let (q, r) = (a.wrapping_div(b), a.wrapping_rem(b));
                // Floor division: round the truncated quotient toward negative
                // infinity when signs differ and the remainder is nonzero.
                let q = if r != 0 && ((r < 0) != (b < 0)) {
                    q.wrapping_sub(1)
                } else {
                    q
                };
                Slot {
                    value: Value::Int(q),
                    kind,
                }
            }
            Pow => {
                if b < 0 {
                    return Err(InterpretError::new(
                        "negative exponent in integer '^'",
                        span,
                        "E334",
                    ));
                }
                Slot {
                    value: Value::Int(int_pow(a, b as u32)),
                    kind,
                }
            }
            Div => Slot {
                value: Value::Float((a as f64) / (b as f64)),
                kind: 14,
            },
            Eq => Slot {
                value: Value::Bool(a == b),
                kind: 0,
            },
            Ne => Slot {
                value: Value::Bool(a != b),
                kind: 0,
            },
            Gt => Slot {
                value: Value::Bool(a > b),
                kind: 0,
            },
            Gte => Slot {
                value: Value::Bool(a >= b),
                kind: 0,
            },
            Lt => Slot {
                value: Value::Bool(a < b),
                kind: 0,
            },
            Lte => Slot {
                value: Value::Bool(a <= b),
                kind: 0,
            },
            And => Slot {
                value: Value::Int(a & b),
                kind,
            },
            Or => Slot {
                value: Value::Int(a | b),
                kind,
            },
            Xor => Slot {
                value: Value::Int(a ^ b),
                kind,
            },
            Lshift => Slot {
                value: Value::Int(a.wrapping_shl(b as u32)),
                kind,
            },
            Rshift => Slot {
                value: Value::Int(a.wrapping_shr(b as u32)),
                kind,
            },
            Concat => unreachable!(),
        })
    }

    fn eval_float_bin(&self, op: BinOp, a: f64, b: f64, span: Span) -> IResult<Slot> {
        use BinOp::*;
        Ok(match op {
            Plus => Slot {
                value: Value::Float(a + b),
                kind: 14,
            },
            Minus => Slot {
                value: Value::Float(a - b),
                kind: 14,
            },
            Mul => Slot {
                value: Value::Float(a * b),
                kind: 14,
            },
            Div => Slot {
                value: Value::Float(a / b),
                kind: 14,
            },
            Eq => Slot {
                value: Value::Bool(a == b),
                kind: 0,
            },
            Ne => Slot {
                value: Value::Bool(a != b),
                kind: 0,
            },
            Gt => Slot {
                value: Value::Bool(a > b),
                kind: 0,
            },
            Gte => Slot {
                value: Value::Bool(a >= b),
                kind: 0,
            },
            Lt => Slot {
                value: Value::Bool(a < b),
                kind: 0,
            },
            Lte => Slot {
                value: Value::Bool(a <= b),
                kind: 0,
            },
            Mod | Pow => {
                let sym = if matches!(op, Mod) { "%" } else { "^" };
                let what = if matches!(op, Mod) {
                    "remainder"
                } else {
                    "exponentiation"
                };
                return Err(InterpretError::new(
                    format!("'{sym}' is integer {what}, not defined on floats"),
                    span,
                    "E334",
                ));
            }
            Fdiv => {
                return Err(InterpretError::new(
                    "integer floor division '//' is not defined on floats",
                    span,
                    "E334",
                ));
            }
            And | Or | Xor | Lshift | Rshift | Concat => {
                return Err(InterpretError::new(
                    format!("invalid float op {op:?}"),
                    span,
                    "E333",
                ));
            }
        })
    }

    fn eval_bool_bin(&self, op: BinOp, a: bool, b: bool, span: Span) -> IResult<Slot> {
        use BinOp::*;
        Ok(match op {
            And => Slot {
                value: Value::Bool(a & b),
                kind: 0,
            },
            Or => Slot {
                value: Value::Bool(a | b),
                kind: 0,
            },
            Xor => Slot {
                value: Value::Bool(a ^ b),
                kind: 0,
            },
            Eq => Slot {
                value: Value::Bool(a == b),
                kind: 0,
            },
            Ne => Slot {
                value: Value::Bool(a != b),
                kind: 0,
            },
            _ => {
                return Err(InterpretError::new(
                    format!("invalid bool op {op:?}"),
                    span,
                    "E336",
                ));
            }
        })
    }

    fn eval_unwrap(&mut self, stack: &mut Vec<Slot>, frame: &mut Frame, span: Span) -> IResult<()> {
        if !matches!(stack.last().map(|s| s.kind), Some(KIND_ERROR)) {
            // Identity on non-error values (e.g. list.push_last result).
            return Ok(());
        }
        if frame.error_payload.is_none() {
            return Err(InterpretError::new(
                "'unwrap' requires the caller to declare a fallible return (|T Err|)",
                span,
                "E308",
            ));
        }
        let env = self.pop(stack, span, "unwrap")?;
        let payload = self.pop(stack, span, "unwrap")?;
        let tag = match env.value {
            Value::Int(n) => n,
            other => {
                other.drop_owned();
                payload.drop_owned();
                return Err(InterpretError::new(
                    "unwrap envelope must be an int tag",
                    span,
                    "E308",
                ));
            }
        };
        if tag == 0 {
            stack.push(payload);
            Ok(())
        } else {
            payload.drop_owned();
            frame.pending_error_return = Some(tag);
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_handle(
        &mut self,
        body: &[Stmt],
        fallback: Option<&Expr>,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Flow> {
        if !matches!(stack.last().map(|s| s.kind), Some(KIND_ERROR)) {
            return Ok(Flow::Next);
        }
        let env = self.pop(stack, span, "handle")?;
        let payload = self.pop(stack, span, "handle")?;
        let tag = match env.value {
            Value::Int(n) => n,
            other => {
                other.drop_owned();
                payload.drop_owned();
                return Err(InterpretError::new(
                    "handle envelope must be an int tag",
                    span,
                    "E308",
                ));
            }
        };
        let pre = stack.len();
        if tag == 0 {
            stack.push(payload);
            return Ok(Flow::Next);
        }
        payload.drop_owned();
        stack.push(Slot {
            value: Value::Int(tag),
            kind: KIND_ERROR,
        });
        let err_idx = stack.len() - 1;
        Self::defer_enter_scope(frame);
        let flow = self.eval_body(body, stack, frame, module, caller_fq)?;
        self.defer_exit_scope(stack, frame, module, caller_fq)?;
        if flow == Flow::Return {
            return Ok(Flow::Return);
        }
        if stack.len() > err_idx && stack[err_idx].kind == KIND_ERROR {
            stack.remove(err_idx).drop_owned();
        } else if let Some(i) = stack.iter().rposition(|s| s.kind == KIND_ERROR)
            && i >= pre
        {
            stack.remove(i).drop_owned();
        }
        if matches!(flow, Flow::Break | Flow::Continue) {
            return Ok(flow);
        }
        if let Some(fb) = fallback {
            self.eval_expr(fb, stack, frame, module, caller_fq, span)?;
        }
        Ok(Flow::Next)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_struct_lit(
        &mut self,
        pairs: &[(String, Expr)],
        ty: &Type,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Slot> {
        let name = match &ty.kind {
            TypeKind::Named(n) => n.as_str(),
            TypeKind::Reference { inner } => match &inner.kind {
                TypeKind::Named(n) => n.as_str(),
                _ => {
                    return Err(InterpretError::unsupported(
                        format!("struct literal type {ty:?}"),
                        span,
                    ));
                }
            },
            _ => {
                return Err(InterpretError::unsupported(
                    format!("struct literal type {ty:?}"),
                    span,
                ));
            }
        };
        let sid = *self
            .struct_ids
            .get(name)
            .ok_or_else(|| InterpretError::new(format!("unknown struct '{name}'"), span, "E340"))?;
        let info = self.structs[sid as usize].clone();
        let handle = runtime::yarrow_alloc(u64::from(info.size));
        unsafe {
            std::ptr::write_bytes(handle as *mut u8, 0, info.size as usize);
        }
        for (fname, expr) in pairs {
            let field = info
                .fields
                .iter()
                .find(|f| f.name == *fname)
                .ok_or_else(|| {
                    InterpretError::new(
                        format!("struct '{name}' has no field '{fname}'"),
                        span,
                        "E340",
                    )
                })?;
            self.eval_expr(expr, stack, frame, module, caller_fq, span)?;
            let val = self.pop(stack, span, "struct field")?;
            let bits = val.value.as_bits().unwrap_or(0);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    &bits as *const u64 as *const u8,
                    (handle as *mut u8).add(field.offset as usize),
                    field.size as usize,
                );
            }
            // Field storage owns heap bits.
            match val.value {
                Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                other => other.drop_owned(),
            }
        }
        Ok(Slot {
            value: Value::Heap {
                handle,
                owned: true,
            },
            kind: KIND_STRUCT | ((sid as u64) << 8),
        })
    }

    fn eval_empty_aggregate(&self, ty: &Type, span: Span) -> IResult<Slot> {
        match &ty.kind {
            TypeKind::Named(n) => {
                if let Some(sid) = self.struct_ids.get(n) {
                    let size = self.structs[*sid as usize].size;
                    let handle = runtime::yarrow_alloc(u64::from(size));
                    unsafe {
                        std::ptr::write_bytes(handle as *mut u8, 0, size as usize);
                    }
                    return Ok(Slot {
                        value: Value::Heap {
                            handle,
                            owned: true,
                        },
                        kind: KIND_STRUCT | ((*sid as u64) << 8),
                    });
                }
                Err(InterpretError::unsupported(
                    format!("empty `{{}}` for type '{n}'"),
                    span,
                ))
            }
            TypeKind::List { element } => {
                let (ek, esize, _) = self.field_layout(element)?;
                let handle = runtime::yarrow_list_new(u64::from(esize));
                Ok(Slot {
                    value: Value::Heap {
                        handle,
                        owned: true,
                    },
                    kind: KIND_LIST | (ek << 8),
                })
            }
            TypeKind::Hashmap { key, value } => {
                let (kk, _, _) = self.field_layout(key)?;
                let (vk, _, _) = self.field_layout(value)?;
                let keys_string = u64::from(kk == KIND_STRING);
                let handle = runtime::yarrow_map_new(keys_string);
                Ok(Slot {
                    value: Value::Heap {
                        handle,
                        owned: true,
                    },
                    kind: KIND_MAP | (kk << 8) | (vk << 40),
                })
            }
            _ => Err(InterpretError::unsupported(
                format!("empty aggregate for {ty:?}"),
                span,
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_list_lit(
        &mut self,
        elems: &[Expr],
        ty: &Type,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Slot> {
        let elem_ty = match &ty.kind {
            TypeKind::List { element } => element.as_ref(),
            _ => {
                return Err(InterpretError::unsupported(
                    format!("list literal type {ty:?}"),
                    span,
                ));
            }
        };
        let (ek, esize, _) = self.field_layout(elem_ty)?;
        let handle = runtime::yarrow_list_new(u64::from(esize));
        for e in elems {
            self.eval_expr(e, stack, frame, module, caller_fq, span)?;
            let val = self.pop(stack, span, "list element")?;
            let bits = val.value.as_bits().unwrap_or(0);
            match val.value {
                Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                other => other.drop_owned(),
            }
            runtime::yarrow_list_push(handle, bits);
        }
        Ok(Slot {
            value: Value::Heap {
                handle,
                owned: true,
            },
            kind: KIND_LIST | (ek << 8),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_map_lit(
        &mut self,
        pairs: &[(Expr, Expr)],
        ty: &Type,
        stack: &mut Vec<Slot>,
        frame: &mut Frame,
        module: Option<&str>,
        caller_fq: &str,
        span: Span,
    ) -> IResult<Slot> {
        let (key_ty, val_ty) = match &ty.kind {
            TypeKind::Hashmap { key, value } => (key.as_ref(), value.as_ref()),
            _ => {
                return Err(InterpretError::unsupported(
                    format!("map literal type {ty:?}"),
                    span,
                ));
            }
        };
        let (kk, _, _) = self.field_layout(key_ty)?;
        let (vk, _, _) = self.field_layout(val_ty)?;
        let handle = runtime::yarrow_map_new(u64::from(kk == KIND_STRING));
        for (k, v) in pairs {
            self.eval_expr(k, stack, frame, module, caller_fq, span)?;
            let key = self.pop(stack, span, "map key")?;
            self.eval_expr(v, stack, frame, module, caller_fq, span)?;
            let val = self.pop(stack, span, "map value")?;
            let kb = key.value.as_bits().unwrap_or(0);
            let vb = val.value.as_bits().unwrap_or(0);
            match key.value {
                Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                other => other.drop_owned(),
            }
            match val.value {
                Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
                other => other.drop_owned(),
            }
            runtime::yarrow_map_insert(handle, kb, vb);
        }
        Ok(Slot {
            value: Value::Heap {
                handle,
                owned: true,
            },
            kind: KIND_MAP | (kk << 8) | (vk << 40),
        })
    }

    fn coerce_to_binding(&mut self, slot: Slot, ty: &Type, span: Span) -> IResult<Slot> {
        if let TypeKind::Named(n) = &ty.kind
            && let Some(uid) = self.union_ids.get(n).copied()
        {
            return self.wrap_union_value(slot, uid, span);
        }
        Ok(slot)
    }

    fn wrap_union_value(&self, slot: Slot, uid: u32, span: Span) -> IResult<Slot> {
        // A value that is already this union type binds as-is (union
        // parameters and same-type assignments must not re-wrap).
        if (slot.kind & 0xff) == KIND_UNION
            && (slot.kind >> 8) as u32 == uid
            && matches!(slot.value, Value::Heap { .. })
        {
            return Ok(slot);
        }
        let info = self
            .unions
            .get(uid as usize)
            .ok_or_else(|| InterpretError::new(format!("unknown union id {uid}"), span, "E346"))?;
        let from = self.slot_member_ty(&slot);
        let idx = info
            .members
            .iter()
            .position(|m| members_compatible(m, &from))
            .ok_or_else(|| {
                InterpretError::new(
                    format!("cannot convert into union '{}'", info.name),
                    span,
                    "E309",
                )
            })?;
        let bits = slot.value.as_bits().unwrap_or(0);
        // Transfer ownership into the union payload.
        match slot.value {
            Value::Str { owned: true, .. } | Value::Heap { owned: true, .. } => {}
            other => other.drop_owned(),
        }
        let handle = runtime::yarrow_alloc(16);
        unsafe {
            std::ptr::write_unaligned(
                (handle as *mut u8).add(UNION_TAG_OFFSET as usize) as *mut u64,
                idx as u64,
            );
            std::ptr::write_unaligned(
                (handle as *mut u8).add(UNION_PAYLOAD_OFFSET as usize) as *mut u64,
                bits,
            );
        }
        Ok(Slot {
            value: Value::Heap {
                handle,
                owned: true,
            },
            kind: KIND_UNION | ((uid as u64) << 8),
        })
    }

    fn type_kind_code(&self, ty: &Type) -> Option<u64> {
        match &ty.kind {
            TypeKind::Primitive(p) => primitive_to_kind(*p),
            TypeKind::Array { element, size } => {
                let elem = self.type_kind_code(element)?;
                let count = size.unwrap_or(0);
                Some(0x60 | (elem << 8) | (count << 40))
            }
            TypeKind::List { element } => {
                let elem = self.type_kind_code(element)?;
                Some(KIND_LIST | (elem << 8))
            }
            TypeKind::Hashmap { key, value } => {
                let k = self.type_kind_code(key)?;
                let v = self.type_kind_code(value)?;
                Some(KIND_MAP | (k << 8) | (v << 40))
            }
            TypeKind::Reference { inner } => self.type_kind_code(inner),
            TypeKind::Pointer { inner } => {
                let pk = self.type_kind_code(inner)?;
                Some(KIND_PTR | (pk << 8))
            }
            TypeKind::Named(name) => self.named_type_kind_code(name),
            TypeKind::Union(_) => Some(KIND_ERROR), // fallible binding rare
        }
    }

    /// Kind code for a name that may be a primitive or a registered named
    /// type (struct, union, enum, error type).
    fn named_type_kind_code(&self, name: &str) -> Option<u64> {
        if let Some(k) = primitive_kind_code(name) {
            return Some(k);
        }
        if let Some(id) = self.struct_ids.get(name) {
            return Some(KIND_STRUCT | ((*id as u64) << 8));
        }
        if let Some(id) = self.union_ids.get(name) {
            return Some(KIND_UNION | ((*id as u64) << 8));
        }
        if let Some(id) = self.enum_ids.get(name) {
            // PLAN 3.1: the type value of an enum is the enum's own kind
            // code, so `c typeof Color ==` compares enum against enum.
            return Some(KIND_ENUM | ((*id as u64) << 8));
        }
        if self.error_type_ids.contains_key(name) {
            return Some(KIND_ERROR);
        }
        None
    }

    fn error_member_tag(&self, ty: &Type) -> Option<u32> {
        let TypeKind::Named(path) = &ty.kind else {
            return None;
        };
        if let Some(member) = path.strip_prefix("error.") {
            return self.error_tags.get(member).copied();
        }
        let (type_name, member) = path.rsplit_once('.')?;
        let id = self.error_type_ids.get(type_name)?;
        self.error_types
            .get(*id as usize)?
            .1
            .iter()
            .find(|(n, _)| n == member)
            .map(|(_, t)| *t)
    }

    fn bits_to_slot(&self, bits: u64, ty: &MemberTy, owned: bool) -> Slot {
        match ty {
            MemberTy::String => Slot {
                value: Value::Str {
                    handle: bits,
                    owned,
                },
                kind: KIND_STRING,
            },
            MemberTy::Primitive(k) => self.bits_to_slot_kind(bits, *k, owned),
            MemberTy::Named(n) => {
                if let Some(id) = self.struct_ids.get(n) {
                    Slot {
                        value: Value::Heap {
                            handle: bits,
                            owned,
                        },
                        kind: KIND_STRUCT | ((*id as u64) << 8),
                    }
                } else if let Some(id) = self.union_ids.get(n) {
                    Slot {
                        value: Value::Heap {
                            handle: bits,
                            owned,
                        },
                        kind: KIND_UNION | ((*id as u64) << 8),
                    }
                } else if let Some(id) = self.enum_ids.get(n) {
                    // PLAN 3.1: an enum payload reads back as the enum type.
                    // PLAN 3.2: a string carrier reads back as its heap
                    // string handle.
                    if self.enums.get(*id as usize).map_or(3, |e| e.carrier) == KIND_STRING {
                        Slot {
                            value: Value::Str {
                                handle: bits,
                                owned,
                            },
                            kind: KIND_ENUM | ((*id as u64) << 8),
                        }
                    } else {
                        Slot {
                            value: Value::Int(bits as i64),
                            kind: KIND_ENUM | ((*id as u64) << 8),
                        }
                    }
                } else {
                    Slot {
                        value: Value::Int(bits as i64),
                        kind: 4,
                    }
                }
            }
        }
    }

    /// Carrier kind code of an enum slot / field / element kind, defaulting
    /// to `i32` for unregistered ids.
    fn enum_carrier(&self, kind: u64) -> u64 {
        self.enums
            .get((kind >> 8) as usize)
            .map_or(3, |e| e.carrier)
    }

    fn bits_to_slot_kind(&self, bits: u64, kind: u64, owned: bool) -> Slot {
        let tag = kind & 0xff;
        match tag {
            0 => Slot {
                value: Value::Bool(bits != 0),
                kind,
            },
            16 => Slot {
                value: Value::Str {
                    handle: bits,
                    owned,
                },
                kind,
            },
            0x20 | 0x30 | 0x40 | 0x70 => Slot {
                value: Value::Heap {
                    handle: bits,
                    owned,
                },
                kind,
            },
            12..=15 => Slot {
                value: Value::Float(f64::from_bits(bits)),
                kind,
            },
            // PLAN 3.2: a string-carried enum reads back as its heap string
            // handle; integer carriers stay inline bits.
            t if t == KIND_ENUM && self.enum_carrier(kind) == KIND_STRING => Slot {
                value: Value::Str {
                    handle: bits,
                    owned,
                },
                kind,
            },
            _ => Slot {
                value: Value::Int(bits as i64),
                kind,
            },
        }
    }

    fn pop(&self, stack: &mut Vec<Slot>, span: Span, what: &str) -> IResult<Slot> {
        stack
            .pop()
            .ok_or_else(|| InterpretError::new(format!("missing operand for {what}"), span, "E362"))
    }

    fn peek<'a>(&self, stack: &'a [Slot], span: Span) -> IResult<&'a Slot> {
        stack
            .last()
            .ok_or_else(|| InterpretError::new("missing operand for dup", span, "E362"))
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

fn int_pow(base: i64, mut exp: u32) -> i64 {
    let mut result = 1i64;
    let mut b = base;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        exp >>= 1;
    }
    result
}

/// Inclusive value range of an integer carrier kind, or `None` for
/// non-integer kinds. `u64` discriminants above `i64::MAX` are stored as their
/// bit pattern; the runtime value representation is `i64`.
pub(crate) fn int_carrier_range(kind: u64) -> Option<(i128, i128)> {
    Some(match kind {
        1 => (-128, 127),
        2 => (-32768, 32767),
        3 => (-2147483648, 2147483647),
        4 => (i64::MIN as i128, i64::MAX as i128),
        6 => (0, 255),
        7 => (0, 65535),
        8 => (0, 4294967295),
        9 => (0, u64::MAX as i128),
        _ => return None,
    })
}

/// Runtime kind codes aligned with `compiler::types::kind_code` for primitives.
/// Shared with the checker (PLAN 6.1), which reuses the same scheme.
pub(crate) fn primitive_kind_code(name: &str) -> Option<u64> {
    Some(match name {
        "bool" => 0,
        "i8" => 1,
        "i16" => 2,
        "i32" => 3,
        "i64" => 4,
        "u8" => 6,
        "u16" => 7,
        "u32" => 8,
        "u64" => 9,
        "rune" => 11,
        "f16" => 12,
        "f32" => 13,
        "f64" => 14,
        "string" => 16,
        "void" => 4,
        _ => return None,
    })
}

pub(crate) fn primitive_to_kind(p: Primitive) -> Option<u64> {
    use Primitive::*;
    Some(match p {
        Bool => 0,
        I8 => 1,
        I16 => 2,
        I32 => 3,
        I64 => 4,
        U8 => 6,
        U16 => 7,
        U32 => 8,
        U64 => 9,
        Rune => 11,
        F16 => 12,
        F32 => 13,
        F64 => 14,
        String => 16,
        Void => 4,
        Error => KIND_ERROR,
        _ => return None,
    })
}

fn kind_elem_size(kind: u64) -> u32 {
    match kind & 0xff {
        0 | 1 | 6 => 1,
        2 | 7 | 12 => 2,
        3 | 8 | 11 | 13 => 4,
        _ => 8,
    }
}

fn pointee_byte_size(kind: u64) -> u32 {
    match kind & 0xff {
        KIND_STRUCT | KIND_UNION | KIND_LIST | KIND_MAP | KIND_STRING | KIND_PTR | KIND_ERROR => 8,
        _ => kind_elem_size(kind),
    }
}

fn read_bits_at(base: u64, offset: i32, size: u32) -> u64 {
    unsafe {
        let mut buf = 0u64;
        std::ptr::copy_nonoverlapping(
            (base as *const u8).add(offset as usize),
            &mut buf as *mut u64 as *mut u8,
            size as usize,
        );
        buf
    }
}

fn write_bits_at(base: u64, offset: i32, size: u32, bits: u64) {
    unsafe {
        std::ptr::copy_nonoverlapping(
            &bits as *const u64 as *const u8,
            (base as *mut u8).add(offset as usize),
            size as usize,
        );
    }
}

/// Alias -> module path for a function's own `require` statements. The
/// signature resolver consults these before the global aliases (PLAN 4.1).
fn require_aliases(reqs: &[LocalRequire]) -> HashMap<String, String> {
    reqs.iter()
        .filter_map(|r| r.alias.clone().map(|a| (a, r.path.clone())))
        .collect()
}

impl Interpreter {
    /// PLAN 4.1: the payload member of a fallible `with |payload error|`
    /// signature, classified by the shared predicate. `None` when the
    /// function cannot fail.
    fn fallible_payload(
        &self,
        returns: &[Type],
        local_aliases: &HashMap<String, String>,
    ) -> Option<MemberTy> {
        match classify_signature(returns, &|n| self.resolves_error_type(n, local_aliases)) {
            Signature::Fallible { payload } => Some(ast_member_ty(payload)),
            _ => None,
        }
    }

    /// PLAN 4.1: a named error side resolves to a registered error type.
    /// A bare name resolves through the global error-type registry; a
    /// dotted path `alias.Member` resolves through the module-alias table
    /// (the function's own aliases first, then the global ones), and the
    /// member must be an error type declared by that module.
    fn resolves_error_type(&self, name: &str, local_aliases: &HashMap<String, String>) -> bool {
        if self.error_type_ids.contains_key(name) {
            return true;
        }
        let Some((alias, member)) = name.rsplit_once('.') else {
            return false;
        };
        let Some(path) = local_aliases
            .get(alias)
            .or_else(|| self.aliases.get(alias))
            .cloned()
        else {
            return false;
        };
        self.error_type_ids.contains_key(member)
            && self
                .type_units
                .get(&(TypeCategory::Error, member.to_string()))
                == Some(&format!("module '{path}'"))
    }

    /// PLAN 4.2 / 4.4: every two-member union return must resolve its
    /// error side. Runs once after all units register and the alias
    /// bindings are built, so it sees the same tables the runtime
    /// resolver does. The checker covers main-unit declarations with
    /// unit-visible names; this pass also covers module units and the
    /// module-qualified references the checker cannot see through.
    fn validate_error_signatures(&self) -> IResult<()> {
        for entry in self.funcs.values() {
            let aliases = require_aliases(&entry.local_requires);
            let signature = classify_signature(&entry.function.returns, &|n| {
                self.resolves_error_type(n, &aliases)
            });
            if let Signature::UnknownError { name } = signature {
                let (span, owner) = match &entry.module {
                    None => (
                        entry
                            .function
                            .returns
                            .first()
                            .map(|t| Span::from_location(t.location))
                            .unwrap_or_default(),
                        format!("function '{}'", entry.function.name),
                    ),
                    Some(path) => (
                        Span::default(),
                        format!("function '{}' of module '{path}'", entry.function.name),
                    ),
                };
                return Err(InterpretError::new(
                    format!("unknown error type '{name}' in the signature of {owner}"),
                    span,
                    "E386",
                ));
            }
        }
        Ok(())
    }
}

fn ast_member_ty(t: &Type) -> MemberTy {
    match &t.kind {
        TypeKind::Primitive(Primitive::String) => MemberTy::String,
        TypeKind::Named(n) if n == "string" => MemberTy::String,
        TypeKind::Primitive(p) => MemberTy::Primitive(primitive_to_kind(*p).unwrap_or(4)),
        TypeKind::Named(n) => MemberTy::Named(n.clone()),
        TypeKind::Reference { inner } => ast_member_ty(inner),
        _ => MemberTy::Primitive(4),
    }
}

fn members_compatible(want: &MemberTy, got: &MemberTy) -> bool {
    match (want, got) {
        (a, b) if a == b => true,
        (MemberTy::Primitive(a), MemberTy::Primitive(b)) => {
            let intish = |k: u64| matches!(k, 1..=11);
            intish(*a) && intish(*b)
        }
        (MemberTy::String, MemberTy::Primitive(16))
        | (MemberTy::Primitive(16), MemberTy::String) => true,
        _ => false,
    }
}

fn int_literal_kind(n: i128) -> u64 {
    if n >= 0 {
        if n <= u8::MAX as i128 {
            6
        } else if n <= u16::MAX as i128 {
            7
        } else if n <= u32::MAX as i128 {
            8
        } else {
            9
        }
    } else if n >= i8::MIN as i128 {
        1
    } else if n >= i16::MIN as i128 {
        2
    } else if n >= i32::MIN as i128 {
        3
    } else {
        4
    }
}

/// Smallest float kind that preserves a float literal at the precision it is
/// written with: `f16` (12), then `f32` (13), else `f64` (14). A literal fits
/// a type when rounding the value through that type and back to the literal's
/// significant decimal digits reproduces the literal's value, so `3.14` is
/// `f16` while `3.14159265358979` is `f64`.
fn float_literal_kind(lexeme: &str, n: f64) -> u64 {
    if !n.is_finite() {
        return 14;
    }
    let digits = significant_decimal_digits(lexeme);
    let preserves = |narrowed: f64| -> bool {
        format!("{:.*e}", digits.saturating_sub(1), narrowed)
            .parse::<f64>()
            .is_ok_and(|rounded| rounded == n)
    };
    if preserves(half::f16::from_f64(n).to_f64()) {
        12
    } else if preserves(n as f32 as f64) {
        13
    } else {
        14
    }
}

/// Significant decimal digits written in a float literal lexeme: mantissa
/// digits with leading zeros skipped, exponent digits ignored.
fn significant_decimal_digits(lexeme: &str) -> usize {
    let mantissa = lexeme.split(['e', 'E']).next().unwrap_or(lexeme);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.trim_start_matches('0').len().max(1)
}

fn infer_value_kind(v: &Value) -> u64 {
    match v {
        Value::Bool(_) => 0,
        Value::Int(_) => 4,
        Value::Float(_) => 14,
        Value::Str { .. } => 16,
        Value::Heap { .. } => 0x50,
        Value::Array(elems) => {
            let count = elems.len() as u64;
            let elem = elems.first().map(infer_value_kind).unwrap_or(4);
            0x60 | (elem << 8) | (count << 40)
        }
    }
}
