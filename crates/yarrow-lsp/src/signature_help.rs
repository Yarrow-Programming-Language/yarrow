//! `textDocument/signatureHelp` for postfix `name call` (and `a.b call`) sites.
//!
//! Uses AST-based signatures only (compiler-based type probes removed).

use tower_lsp_server::ls_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};
use yarrow_core::parser::ast::{
    Function, ParamModifier, Primitive, Stmt, StmtKind, Type, TypeKind,
};
use yarrow_core::{Program, SourceFile, Span, Token, TokenKind, Tokenizer};

use crate::config::LspConfig;
use crate::position::{PositionEncoding, PositionMap};

/// Signature help at a call site, or `None` outside a call / unresolved callee.
pub fn signature_help(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    config: &LspConfig,
) -> Option<SignatureHelp> {
    let session = config.session(path);
    let parsed = session.parse_source(text.to_string()).ok()?;
    let map = PositionMap::from_file(&parsed.file, encoding);
    let offset = map.offset(position)?;
    let site = call_site_at(&parsed.file.source, offset)?;

    let decls = collect_functions(&parsed.file, &parsed.program);
    let decl = resolve(&decls, &site.name, site.name_offset)?;

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: decl.signature.clone(),
            documentation: None,
            parameters: if decl.parameters.is_empty() {
                None
            } else {
                Some(decl.parameters.clone())
            },
            active_parameter: None,
        }],
        active_signature: Some(0),
        // Stack args precede the callee; do not guess activeParameter.
        active_parameter: None,
    })
}

#[derive(Debug, Clone)]
struct CallSite {
    /// Last identifier in the callee path (`demo`, `write_line`, …).
    name: String,
    /// Byte offset inside the simple name (for scope resolve).
    name_offset: usize,
}

#[derive(Debug, Clone)]
struct FnSig {
    name: String,
    name_span: Span,
    scope: Span,
    signature: String,
    parameters: Vec<ParameterInformation>,
}

fn call_site_at(source: &str, offset: usize) -> Option<CallSite> {
    let mut tokenizer = Tokenizer::new(source.to_string());
    let tokens = tokenizer.tokenize().ok()?;
    let mut best: Option<(usize, CallSite)> = None;

    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind != TokenKind::Call {
            continue;
        }
        let Some((path_lo, name_tok)) = callee_path_before(&tokens, i) else {
            continue;
        };
        let region_lo = path_lo;
        let region_hi = tok.end_offset;
        // Inclusive end so the cursor after `call` still counts.
        if offset < region_lo || offset > region_hi {
            continue;
        }
        let region_len = region_hi.saturating_sub(region_lo);
        let name_offset = name_tok
            .location
            .offset
            .min(name_tok.end_offset.saturating_sub(1));
        let site = CallSite {
            name: name_tok.lexeme.clone(),
            name_offset,
        };
        // Innermost: prefer the tightest callee…call region containing the cursor.
        if best.as_ref().is_none_or(|(len, _)| region_len < *len) {
            best = Some((region_len, site));
        }
    }
    best.map(|(_, site)| site)
}

/// Walk back from `call` over `ident` or `ident.ident…`.
fn callee_path_before(tokens: &[Token], call_index: usize) -> Option<(usize, &Token)> {
    if call_index == 0 {
        return None;
    }
    let mut i = call_index - 1;
    if tokens[i].kind != TokenKind::Identifier {
        return None;
    }
    let name_tok = &tokens[i];
    while i >= 2
        && tokens[i - 1].kind == TokenKind::Dot
        && tokens[i - 2].kind == TokenKind::Identifier
    {
        i -= 2;
    }
    let path_lo = tokens[i].location.offset;
    Some((path_lo, name_tok))
}

fn collect_functions(file: &SourceFile, program: &Program) -> Vec<FnSig> {
    let source = file.source.as_str();
    let file_scope = Span::new(0, source.len());
    let mut out = Vec::new();
    for item in &program.items {
        collect_stmt(source, item, file_scope, &mut out);
    }
    out
}

fn collect_stmt(source: &str, stmt: &Stmt, scope: Span, out: &mut Vec<FnSig>) {
    match &stmt.kind {
        StmtKind::Function(f) => {
            push_fn(source, stmt.span, scope, f, None, out);
            for s in &f.body {
                collect_stmt(source, s, stmt.span, out);
            }
        }
        StmtKind::Implement(imp) => {
            for f in &imp.functions {
                push_fn(source, stmt.span, scope, f, Some(imp.target.as_str()), out);
                for s in &f.body {
                    collect_stmt(source, s, stmt.span, out);
                }
            }
        }
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_stmt(source, s, scope, out);
            }
            for s in else_branch {
                collect_stmt(source, s, scope, out);
            }
        }
        StmtKind::For { body, .. } => {
            for s in body {
                collect_stmt(source, s, scope, out);
            }
        }
        StmtKind::Match {
            cases, else_branch, ..
        } => {
            for case in cases {
                for s in &case.body {
                    collect_stmt(source, s, scope, out);
                }
            }
            for s in else_branch {
                collect_stmt(source, s, scope, out);
            }
        }
        StmtKind::Defer { body } | StmtKind::Unsafe { body } => {
            for s in body {
                collect_stmt(source, s, scope, out);
            }
        }
        StmtKind::Handle { body, .. } => {
            for s in body {
                collect_stmt(source, s, scope, out);
            }
        }
        _ => {}
    }
}

fn push_fn(
    source: &str,
    item_span: Span,
    scope: Span,
    f: &Function,
    implement_target: Option<&str>,
    out: &mut Vec<FnSig>,
) {
    let Some(name_span) = name_span_in(source, item_span, &f.name) else {
        return;
    };
    let (signature, parameters) = format_signature(f, implement_target);
    out.push(FnSig {
        name: f.name.clone(),
        name_span,
        scope,
        signature,
        parameters,
    });
}

fn name_span_in(source: &str, span: Span, name: &str) -> Option<Span> {
    if name.is_empty() || span.lo > span.hi || span.hi > source.len() {
        return None;
    }
    let slice = &source[span.lo..span.hi];
    let rel = slice.find(name)?;
    let lo = span.lo + rel;
    let hi = lo + name.len();
    Some(Span::new(lo, hi))
}

fn resolve<'a>(decls: &'a [FnSig], name: &str, offset: usize) -> Option<&'a FnSig> {
    decls
        .iter()
        .filter(|d| d.name == name && d.scope.lo <= offset && offset < d.scope.hi)
        .min_by_key(|d| d.scope.len())
}

fn format_signature(
    f: &Function,
    implement_target: Option<&str>,
) -> (String, Vec<ParameterInformation>) {
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    let mut label = if let Some(target) = implement_target {
        format!("{} {unsafe_kw}function on {target}", f.name)
    } else {
        format!("{} {unsafe_kw}function", f.name)
    };

    let mut parameters = Vec::new();
    if !f.params.is_empty() {
        let param_strs: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                let ty = format_type(&p.ty);
                match p.modifier {
                    Some(ParamModifier::Copy) => format!("{ty} copy"),
                    Some(ParamModifier::Mutable) => format!("{ty} mutable"),
                    None => ty,
                }
            })
            .collect();
        label.push_str(" (");
        label.push_str(&param_strs.join(", "));
        label.push(')');
        for p in param_strs {
            parameters.push(ParameterInformation {
                label: ParameterLabel::Simple(p),
                documentation: None,
            });
        }
    }
    if !f.returns.is_empty() {
        let rets = f
            .returns
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" ");
        label.push_str(" with ");
        label.push_str(&rets);
    }
    (label, parameters)
}

fn format_type(ty: &Type) -> String {
    match &ty.kind {
        TypeKind::Named(name) => name.clone(),
        TypeKind::Primitive(p) => primitive_name(*p).to_string(),
        TypeKind::Array { element, size } => match size {
            Some(n) => format!("array<{} {n}>", format_type(element)),
            None => format!("array<{}>", format_type(element)),
        },
        TypeKind::List { element } => format!("list<{}>", format_type(element)),
        TypeKind::Hashmap { key, value } => {
            format!("hashmap<{} {}>", format_type(key), format_type(value))
        }
        TypeKind::Reference { inner } => format!("reference<{}>", format_type(inner)),
        TypeKind::Pointer { inner } => format!("pointer<{}>", format_type(inner)),
        TypeKind::Union(members) => {
            let inner = members
                .iter()
                .map(format_type)
                .collect::<Vec<_>>()
                .join(" ");
            format!("|{inner}|")
        }
    }
}

fn primitive_name(p: Primitive) -> &'static str {
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
}
