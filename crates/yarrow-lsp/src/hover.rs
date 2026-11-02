//! Same-file `textDocument/hover` from AST signatures only.
//!
//! Compiler-based type probes removed - only AST-based signatures are available.

use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use yarrow_core::parser::ast::{
    Function, Mutability, ParamModifier, Primitive, Stmt, StmtKind, Type, TypeKind,
};
use yarrow_core::{Program, SourceFile, Span, TokenKind, Tokenizer, explain_code};

use crate::config::LspConfig;
use crate::position::{PositionEncoding, PositionMap};

/// Hover at `position` in `text`, or `None` if unresolved / empty / parse failure.
pub fn hover(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    config: &LspConfig,
) -> Option<Hover> {
    let session = config.session(path);
    let parsed = session.parse_source(text.to_string()).ok()?;
    let map = PositionMap::from_file(&parsed.file, encoding);
    let offset = map.offset(position)?;
    let name = identifier_at(&parsed.file.source, offset)?;
    let decls = collect_hovers(&parsed.file, &parsed.program);
    let decl = resolve(&decls, &name, offset)?;
    let range = map.range(decl.name_span);

    let mut md = format!("```yarrow\n{}\n```", decl.signature);

    // With no type checker, we can only show AST-based info
    // Try to show diagnostics from parsing if available
    match session.parse_source(text.to_string()) {
        Ok(_) => {
            // Successful parse, nothing more to add
        }
        Err(err) => {
            if let Some(explain) = explain_in_batch(&err.batch, offset) {
                md.push_str("\n\n---\n\n");
                md.push_str(&explain);
            }
        }
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(range),
    })
}

#[derive(Debug, Clone)]
struct HoverDecl {
    name: String,
    name_span: Span,
    scope: Span,
    signature: String,
}

fn identifier_at(source: &str, offset: usize) -> Option<String> {
    let mut tokenizer = Tokenizer::new(source.to_string());
    let tokens = tokenizer.tokenize().ok()?;
    let token = tokens
        .iter()
        .find(|t| {
            t.kind == TokenKind::Identifier && t.location.offset <= offset && offset < t.end_offset
        })
        .or_else(|| {
            tokens.iter().find(|t| {
                t.kind == TokenKind::Identifier
                    && offset == t.end_offset
                    && t.location.offset < offset
            })
        })?;
    Some(token.lexeme.clone())
}

fn collect_hovers(file: &SourceFile, program: &Program) -> Vec<HoverDecl> {
    let source = file.source.as_str();
    let file_scope = Span::new(0, source.len());
    let mut out = Vec::new();
    for item in &program.items {
        collect_stmt(source, item, file_scope, &mut out);
    }
    out
}

fn collect_stmt(source: &str, stmt: &Stmt, scope: Span, out: &mut Vec<HoverDecl>) {
    match &stmt.kind {
        StmtKind::Function(f) => {
            push_hover(
                source,
                stmt.span,
                scope,
                &f.name,
                format_function(f, None),
                out,
            );
            collect_function_body(source, f, stmt.span, out);
        }
        StmtKind::Struct(s) => {
            push_hover(
                source,
                stmt.span,
                scope,
                &s.name,
                format!("{} struct", s.name),
                out,
            );
            for field in &s.fields {
                push_hover(
                    source,
                    stmt.span,
                    stmt.span,
                    &field.name,
                    format!("{} {}", format_type(&field.ty), field.name),
                    out,
                );
            }
        }
        StmtKind::Enum(e) => {
            let underlying = e
                .underlying
                .as_ref()
                .map(|t| format!(" {}", format_type(t)))
                .unwrap_or_default();
            push_hover(
                source,
                stmt.span,
                scope,
                &e.name,
                format!("{}{underlying} enum", e.name),
                out,
            );
            for member in &e.members {
                let value = member
                    .value
                    .as_ref()
                    .map(|v| format!(" = {v}"))
                    .unwrap_or_default();
                push_hover(
                    source,
                    stmt.span,
                    stmt.span,
                    &member.name,
                    format!("enum member {}.{}{value}", e.name, member.name),
                    out,
                );
            }
        }
        StmtKind::Union(u) => {
            let members = u
                .types
                .iter()
                .map(format_type)
                .collect::<Vec<_>>()
                .join(" ");
            push_hover(
                source,
                stmt.span,
                scope,
                &u.name,
                format!("{} |{members}| union", u.name),
                out,
            );
        }
        StmtKind::Error(err) => {
            push_hover(
                source,
                stmt.span,
                scope,
                &err.name,
                format!("{} error", err.name),
                out,
            );
            for member in &err.members {
                push_hover(
                    source,
                    stmt.span,
                    stmt.span,
                    member,
                    format!("error member {}.{}", err.name, member),
                    out,
                );
            }
        }
        StmtKind::Implement(imp) => {
            push_hover(
                source,
                stmt.span,
                scope,
                &imp.target,
                format!("{} implement", imp.target),
                out,
            );
            for f in &imp.functions {
                push_hover(
                    source,
                    stmt.span,
                    scope,
                    &f.name,
                    format_function(f, Some(&imp.target)),
                    out,
                );
                collect_function_body(source, f, stmt.span, out);
            }
        }
        StmtKind::Require { path, alias } => {
            let name = alias
                .as_deref()
                .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path));
            let sig = match alias {
                Some(a) => format!("\"{path}\" {a} require"),
                None => format!("\"{path}\" require"),
            };
            push_hover(source, stmt.span, scope, name, sig, out);
        }
        StmtKind::VarDecl {
            name,
            mutability,
            ty,
            ..
        } => {
            let kind = match mutability {
                Mutability::Mutable => "mutable",
                Mutability::Const => "const",
                Mutability::Static => "static",
            };
            push_hover(
                source,
                stmt.span,
                scope,
                name,
                format!("{name} {kind} {}", format_type(ty)),
                out,
            );
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

fn collect_function_body(
    source: &str,
    function: &Function,
    function_span: Span,
    out: &mut Vec<HoverDecl>,
) {
    for stmt in &function.body {
        collect_stmt(source, stmt, function_span, out);
    }
}

fn push_hover(
    source: &str,
    item_span: Span,
    scope: Span,
    name: &str,
    signature: String,
    out: &mut Vec<HoverDecl>,
) {
    if let Some(name_span) = name_span_in(source, item_span, name) {
        out.push(HoverDecl {
            name: name.to_string(),
            name_span,
            scope,
            signature,
        });
    }
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

fn resolve<'a>(decls: &'a [HoverDecl], name: &str, offset: usize) -> Option<&'a HoverDecl> {
    decls
        .iter()
        .filter(|d| d.name == name && d.scope.lo <= offset && offset < d.scope.hi)
        .min_by_key(|d| d.scope.len())
}

fn format_function(f: &Function, implement_target: Option<&str>) -> String {
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    let mut s = if let Some(target) = implement_target {
        format!("{} {unsafe_kw}function on {target}", f.name)
    } else {
        format!("{} {unsafe_kw}function", f.name)
    };
    if !f.params.is_empty() {
        let params = f
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
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(" (");
        s.push_str(&params);
        s.push(')');
    }
    if !f.returns.is_empty() {
        let rets = f
            .returns
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" ");
        s.push_str(" with ");
        s.push_str(&rets);
    }
    s
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

/// If a diagnostic covers `offset` and has a catalog explain, return a short blurb.
fn explain_in_batch(batch: &yarrow_core::DiagnosticBatch, offset: usize) -> Option<String> {
    for diag in batch.iter() {
        let covers = diag
            .labels
            .iter()
            .any(|l| l.span.lo <= offset && offset < l.span.hi);
        if !covers {
            continue;
        }
        let entry = explain_code(&diag.code)?;
        return Some(format!(
            "**Explain {}**: {}\n\n{}",
            entry.code,
            entry.title,
            entry.body.trim()
        ));
    }
    None
}
