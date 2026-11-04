//! Fallible-signature classification shared by the checker and the
//! interpreter (PLAN Stage 4).
//!
//! Fallibility is a property of the `with` return signature: a two-member
//! anonymous union `|payload error|` is fallible when one side is the
//! `error` primitive or a named type that resolves to a registered error
//! type. Both passes call [`classify_signature`] and supply the resolver
//! appropriate to what they can see (PLAN 4.1): the checker resolves bare
//! names against the declarations of the unit it is checking, while the
//! interpreter resolves against the registered error types and the
//! module-alias table.
//!
//! Anonymous unions are only supported as this fallible form, so a
//! two-member union return whose error side resolves to nothing is
//! rejected with `E386` (PLAN 4.2) instead of silently treating the
//! function as non-fallible.

use crate::parser::ast::{Primitive, Type, TypeKind};

/// Classification of one function's `with` return signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signature<'a> {
    /// Not a two-member anonymous union: the function cannot fail.
    NonFallible,
    /// The fallible form with a resolvable error side; `payload` is the
    /// other member.
    Fallible { payload: &'a Type },
    /// A two-member anonymous union whose error side resolves to no
    /// registered error type (`E386`, PLAN 4.2). `name` is the member to
    /// blame.
    UnknownError { name: String },
}

/// Classify a `with` return signature. `resolves` decides whether a named
/// type (bare or module-qualified) names a registered error type.
pub fn classify_signature<'a, F>(returns: &'a [Type], resolves: &F) -> Signature<'a>
where
    F: Fn(&str) -> bool,
{
    if returns.len() != 1 {
        return Signature::NonFallible;
    }
    let TypeKind::Union(members) = &returns[0].kind else {
        return Signature::NonFallible;
    };
    if members.len() != 2 {
        return Signature::NonFallible;
    }
    let is_err = |t: &Type| match &t.kind {
        TypeKind::Primitive(Primitive::Error) => true,
        TypeKind::Named(n) => resolves(n),
        _ => false,
    };
    match (is_err(&members[0]), is_err(&members[1])) {
        (false, true) => Signature::Fallible {
            payload: &members[0],
        },
        (true, false) => Signature::Fallible {
            payload: &members[1],
        },
        // Both sides are error types: no payload side, so this is not the
        // fallible form. Both sides resolve, so it is not E386 either.
        (true, true) => Signature::NonFallible,
        // Anonymous two-member unions exist only as the fallible form, so
        // a signature with no resolvable error side is rejected (PLAN 4.2).
        (false, false) => Signature::UnknownError {
            name: error_side_name(members),
        },
    }
}

/// The member to blame for an unresolvable signature. The grammar writes
/// the error side second (`|Success Err|`), so prefer the second member,
/// then the last named member.
fn error_side_name(members: &[Type]) -> String {
    for t in [&members[1], &members[0]] {
        if let TypeKind::Named(n) = &t.kind {
            return n.clone();
        }
    }
    type_name(&members[1])
}

/// Human-readable name of a type, for diagnostics.
fn type_name(t: &Type) -> String {
    match &t.kind {
        TypeKind::Named(n) => n.clone(),
        TypeKind::Primitive(p) => primitive_name(*p),
        _ => "the second union member".to_string(),
    }
}

fn primitive_name(p: Primitive) -> String {
    match p {
        Primitive::I8 => "i8",
        Primitive::I16 => "i16",
        Primitive::I32 => "i32",
        Primitive::I64 => "i64",
        Primitive::U8 => "u8",
        Primitive::U16 => "u16",
        Primitive::U32 => "u32",
        Primitive::U64 => "u64",
        Primitive::F16 => "f16",
        Primitive::F32 => "f32",
        Primitive::F64 => "f64",
        Primitive::String => "string",
        Primitive::Rune => "rune",
        Primitive::Bool => "bool",
        Primitive::Void => "void",
        Primitive::Error => "error",
        Primitive::Type => "type",
    }
    .to_string()
}
