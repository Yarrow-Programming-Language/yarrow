//! File-local `textDocument/rename` / `prepareRename` via scoped AST bindings.
//!
//! Edits only same-file identifier tokens bound to the same declaration. Require
//! / import bindings that need cross-file or path-string edits are refused with
//! a clear error (no partial silent skip).

use std::collections::HashMap;

use tower_lsp_server::ls_types::{Position, PrepareRenameResponse, TextEdit, Uri, WorkspaceEdit};
use yarrow_core::{Span, TokenKind, Tokenizer};

use crate::config::LspConfig;
use crate::definition::{Decl, DeclKind, collect_decls, identifier_span_at, resolve};
use crate::position::{PositionEncoding, PositionMap};

/// Prefer `prepareRename`: return the identifier range, or `None` when not renameable.
pub fn prepare_rename(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    config: &LspConfig,
) -> Option<PrepareRenameResponse> {
    let Target {
        map,
        name,
        name_span,
        decl,
        ..
    } = rename_target(path, text, encoding, position, config)?;
    if !renameable_decl(text, &decl) {
        return None;
    }
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: map.range(name_span),
        placeholder: name,
    })
}

/// Compute a workspace edit for renaming the binding at `position` to `new_name`.
///
/// Returns `Err` with a user-facing message when the rename is invalid or out of scope.
pub fn rename(
    uri: &Uri,
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    new_name: &str,
    config: &LspConfig,
) -> Result<WorkspaceEdit, String> {
    let Target {
        map,
        name: old_name,
        decls,
        decl,
        ..
    } = rename_target(path, text, encoding, position, config)
        .ok_or_else(|| "cannot rename: no identifier at position".to_string())?;

    if !renameable_decl(text, &decl) {
        return Err(
            "cannot rename require / import binding across modules; use an explicit alias for file-local rename, or rename at the definition"
                .into(),
        );
    }

    validate_new_name(new_name)?;

    if new_name == old_name {
        return Ok(WorkspaceEdit::new(HashMap::new()));
    }

    if name_collides(&decl, new_name, &decls) {
        return Err(format!(
            "cannot rename to `{new_name}`: name already bound in an overlapping scope"
        ));
    }

    let mut tokenizer = Tokenizer::new(text.to_string());
    let tokens = tokenizer
        .tokenize()
        .map_err(|_| "cannot rename: tokenize failed".to_string())?;

    let target = decl.name_span;
    let mut edits: Vec<TextEdit> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Identifier && t.lexeme == old_name)
        .filter(|t| {
            resolve(&decls, &old_name, t.location.offset)
                .is_some_and(|d| spans_equal(d.name_span, target))
        })
        .map(|t| {
            let span = Span::new(t.location.offset, t.end_offset);
            TextEdit {
                range: map.range(span),
                new_text: new_name.to_string(),
            }
        })
        .collect();

    if edits.is_empty() {
        return Err("cannot rename: no editable occurrences".into());
    }

    edits.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then(a.range.start.character.cmp(&b.range.start.character))
    });

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Ok(WorkspaceEdit::new(changes))
}

struct Target<'a> {
    map: PositionMap<'a>,
    name: String,
    name_span: Span,
    decls: Vec<Decl>,
    decl: Decl,
}

fn rename_target<'a>(
    path: &str,
    text: &'a str,
    encoding: PositionEncoding,
    position: Position,
    config: &LspConfig,
) -> Option<Target<'a>> {
    let session = config.session(path);
    let parsed = session.parse_source(text.to_string()).ok()?;
    let map = PositionMap::new(text, encoding);
    let offset = map.offset(position)?;
    let (name, name_span) = identifier_span_at(text, offset)?;
    let decls = collect_decls(&parsed.file, &parsed.program);
    let decl = resolve(&decls, &name, offset)?.clone();
    Some(Target {
        map,
        name,
        name_span,
        decls,
        decl,
    })
}

/// Local decls always; require only when the binding name is an Identifier (explicit alias).
fn renameable_decl(source: &str, decl: &Decl) -> bool {
    match decl.kind {
        DeclKind::Function | DeclKind::Variable | DeclKind::Type | DeclKind::Property => {
            decl.require_path.is_none()
        }
        DeclKind::Module => {
            decl.require_path.is_some() && span_is_identifier(source, decl.name_span)
        }
    }
}

fn span_is_identifier(source: &str, span: Span) -> bool {
    let mut tokenizer = Tokenizer::new(source.to_string());
    let Ok(tokens) = tokenizer.tokenize() else {
        return false;
    };
    tokens.iter().any(|t| {
        t.kind == TokenKind::Identifier && t.location.offset == span.lo && t.end_offset == span.hi
    })
}

fn validate_new_name(new_name: &str) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("cannot rename: new name is empty".into());
    }
    let mut chars = new_name.chars();
    let Some(first) = chars.next() else {
        return Err("cannot rename: new name is empty".into());
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "cannot rename: `{new_name}` is not a valid identifier"
        ));
    }

    let mut tokenizer = Tokenizer::new(new_name.to_string());
    let tokens = tokenizer
        .tokenize()
        .map_err(|_| format!("cannot rename: `{new_name}` is not a valid identifier"))?;
    let idents: Vec<_> = tokens.iter().filter(|t| t.kind != TokenKind::Eof).collect();
    if idents.len() != 1 || idents[0].kind != TokenKind::Identifier {
        return Err(format!(
            "cannot rename: `{new_name}` is a keyword or not a single identifier"
        ));
    }
    Ok(())
}

fn name_collides(target: &Decl, new_name: &str, decls: &[Decl]) -> bool {
    decls.iter().any(|d| {
        if spans_equal(d.name_span, target.name_span) {
            return false;
        }
        d.name == new_name && scopes_overlap(d.scope, target.scope)
    })
}

fn scopes_overlap(a: Span, b: Span) -> bool {
    a.lo < b.hi && b.lo < a.hi
}

fn spans_equal(a: Span, b: Span) -> bool {
    a.lo == b.lo && a.hi == b.hi
}
