//! `textDocument/inlayHint` - disabled without type checker.
//!
//! Type probes (TypeProbe) and multi-file type checking removed with compiler.
//! This module now returns empty hints.

use tower_lsp_server::ls_types::{InlayHint, Range};

use crate::config::LspConfig;
use crate::position::PositionEncoding;

/// Inlay hints for sites that intersect `range`.
/// Always returns empty since type information is no longer available.
pub fn inlay_hints(
    _path: &str,
    _text: &str,
    _encoding: PositionEncoding,
    _range: Range,
    config: &LspConfig,
) -> Vec<InlayHint> {
    if !config.inlay_hints_enable {
        return Vec::new();
    }

    // Without a type checker, we cannot provide type hints
    Vec::new()
}
