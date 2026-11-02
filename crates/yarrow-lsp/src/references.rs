//! Same-file `textDocument/references` via scoped AST bindings.
//!
//! When an identifier resolves to a declaration, every same-name identifier in
//! this file that resolves to the same decl is returned. If no binding is found,
//! falls back to all identifier tokens with that lexeme (documented limitation:
//! not binding-accurate; still same-file only).

use tower_lsp_server::ls_types::{Location, Position, Uri};
use yarrow_core::{TokenKind, Tokenizer};

use crate::config::LspConfig;
use crate::definition::{collect_decls, identifier_at, resolve};
use crate::position::{PositionEncoding, PositionMap};

/// Find references at `position` in `text` (same document only).
pub fn find_references(
    uri: &Uri,
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    include_declaration: bool,
    config: &LspConfig,
) -> Option<Vec<Location>> {
    let session = config.session(path);
    let parsed = session.parse_source(text.to_string()).ok()?;
    let map = PositionMap::from_file(&parsed.file, encoding);
    let offset = map.offset(position)?;
    let name = identifier_at(&parsed.file.source, offset)?;

    let mut tokenizer = Tokenizer::new(parsed.file.source.clone());
    let tokens = tokenizer.tokenize().ok()?;
    let idents: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Identifier && t.lexeme == name)
        .collect();

    let decls = collect_decls(&parsed.file, &parsed.program);
    let locations = if let Some(decl) = resolve(&decls, &name, offset) {
        idents
            .iter()
            .filter(|t| {
                let at = t.location.offset;
                if !include_declaration
                    && spans_equal(t.location.offset, t.end_offset, decl.name_span)
                {
                    return false;
                }
                resolve(&decls, &name, at)
                    .is_some_and(|d| spans_equal(d.name_span.lo, d.name_span.hi, decl.name_span))
            })
            .map(|t| {
                let span = yarrow_core::Span::new(t.location.offset, t.end_offset);
                Location::new(uri.clone(), map.range(span))
            })
            .collect::<Vec<_>>()
    } else {
        // Textual fallback: no binding data for this name at the cursor.
        // `include_declaration` cannot be honored accurately without a decl.
        idents
            .iter()
            .map(|t| {
                let span = yarrow_core::Span::new(t.location.offset, t.end_offset);
                Location::new(uri.clone(), map.range(span))
            })
            .collect::<Vec<_>>()
    };

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

fn spans_equal(lo: usize, hi: usize, span: yarrow_core::Span) -> bool {
    lo == span.lo && hi == span.hi
}
