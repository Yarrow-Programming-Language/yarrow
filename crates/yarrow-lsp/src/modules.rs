//! Resolve `require` dotted paths to on-disk `.yar` files or virtual std URIs.
//!
//! Std modules are embedded in `yarrow-core` at compile time. Navigation prefers
//! real `lib/std/**` paths from the repo / adjacent crate layout; when no file
//! exists (or `YARROW_LSP_FORCE_VIRTUAL_STD=1`), targets use the `yarrow-std:`
//! scheme and embedded source (Stage 26).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;
use yarrow_core::Session;
use yarrow_core::parser::ast::StmtKind;

/// URI scheme for embedded std modules when on-disk `lib/std` is missing.
pub const STD_URI_SCHEME: &str = "yarrow-std";

/// On-disk or virtual module target for go-to-definition.
#[derive(Debug, Clone)]
pub struct ModuleTarget {
    /// Dotted module path (`std.io`).
    pub module_path: String,
    /// Real file when resolvable; `None` means use [`Self::uri`] virtual scheme.
    pub path: Option<PathBuf>,
    /// Item import (`"std.math.sqrt" require`) binds only this top-level name.
    pub item: Option<String>,
}

impl ModuleTarget {
    /// LSP URI for this module (`file://` or `yarrow-std:/…`).
    pub fn uri(&self) -> Option<Uri> {
        if let Some(path) = &self.path {
            Uri::from_file_path(path)
        } else {
            virtual_std_uri(&self.module_path)
        }
    }

    /// Module source text (disk only, embedded std disabled without compiler).
    pub fn source(&self) -> Option<String> {
        if let Some(path) = &self.path {
            std::fs::read_to_string(path).ok()
        } else {
            None // embedded std disabled without compiler
        }
    }
}

/// Build `yarrow-std:/io.yar` for dotted `std.io`.
pub fn virtual_std_uri(module_path: &str) -> Option<Uri> {
    let rest = module_path.strip_prefix("std.")?;
    let rel = format!("{}.yar", rest.replace('.', "/"));
    Uri::from_str(&format!("{STD_URI_SCHEME}:/{rel}")).ok()
}

/// Parse `yarrow-std:/io.yar` (or `yarrow-std:io.yar`) back to `std.io`.
pub fn module_path_from_std_uri(uri: &Uri) -> Option<String> {
    if uri.scheme().as_str() != STD_URI_SCHEME {
        return None;
    }
    let mut path = uri.path().as_str().trim_start_matches('/');
    // Opaque form `yarrow-std:io.yar` may put the name in path() differently;
    // also accept the full URI after the scheme colon when path is empty.
    if path.is_empty() {
        let s = uri.as_str();
        let rest = s.strip_prefix(&format!("{STD_URI_SCHEME}:"))?;
        path = rest.trim_start_matches('/');
    }
    let stem = path.strip_suffix(".yar").unwrap_or(path);
    if stem.is_empty() {
        return None;
    }
    Some(format!("std.{}", stem.replace('/', ".")))
}

/// Whether `uri` uses the virtual std scheme.
pub fn is_virtual_std_uri(uri: &Uri) -> bool {
    uri.scheme().as_str() == STD_URI_SCHEME
}

/// Source for a virtual std URI (disabled without compiler).
pub fn source_for_std_uri(_uri: &Uri) -> Option<&'static str> {
    None // embedded std disabled without compiler
}

/// Resolve a `require` path string to a file under search roots / `lib/std`,
/// or to a virtual std target when only the embedded module exists.
pub fn resolve_require_file(
    source_path: &str,
    require_path: &str,
    search_paths: &[PathBuf],
) -> Option<ModuleTarget> {
    let (module_path, item) = classify_require(source_path, require_path, search_paths)?;
    let path = locate_module_file(source_path, &module_path, search_paths);
    if path.is_none() && !module_exists_embedded(&module_path) {
        return None;
    }
    Some(ModuleTarget {
        module_path,
        path,
        item,
    })
}

/// Parent-first item import vs whole-module (mirrors core `resolve_require`).
fn classify_require(
    source_path: &str,
    require_path: &str,
    search_paths: &[PathBuf],
) -> Option<(String, Option<String>)> {
    if let Some((parent, last)) = require_path.rsplit_once('.')
        && module_resolvable(source_path, parent, search_paths)
        && module_has_top_level_function(source_path, parent, last, search_paths)
    {
        return Some((parent.to_string(), Some(last.to_string())));
    }
    if module_resolvable(source_path, require_path, search_paths) {
        return Some((require_path.to_string(), None));
    }
    None
}

fn module_resolvable(source_path: &str, module_path: &str, search_paths: &[PathBuf]) -> bool {
    locate_module_file(source_path, module_path, search_paths).is_some()
}

fn module_exists_embedded(_module_path: &str) -> bool {
    false // embedded std disabled without compiler
}

fn module_has_top_level_function(
    source_path: &str,
    module_path: &str,
    name: &str,
    search_paths: &[PathBuf],
) -> bool {
    let (label, text) =
        if let Some(path) = locate_module_file(source_path, module_path, search_paths) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                return false;
            };
            (path.to_string_lossy().into_owned(), text)
        } else {
            return false;
        };
    let session = Session::with_source_path(label);
    let Ok(parsed) = session.parse_source(text) else {
        return false;
    };
    parsed.program.items.iter().any(|item| {
        matches!(&item.kind, StmtKind::Function(f) if f.name == name)
            || matches!(&item.kind, StmtKind::Implement(imp) if imp.functions.iter().any(|f| f.name == name))
    })
}

fn locate_module_file(
    source_path: &str,
    module_path: &str,
    search_paths: &[PathBuf],
) -> Option<PathBuf> {
    if force_virtual_std() && module_path.starts_with("std.") {
        return None;
    }

    if let Some(rest) = module_path.strip_prefix("std.") {
        let rel = PathBuf::from(rest.replace('.', "/")).with_extension("yar");
        for root in std_lib_roots(source_path) {
            let candidate = root.join(&rel);
            if candidate.is_file() {
                return canonical_or_self(candidate);
            }
        }
    }

    let rel = PathBuf::from(module_path.replace('.', "/")).with_extension("yar");
    if let Some(parent) = Path::new(source_path).parent() {
        let candidate = parent.join(&rel);
        if candidate.is_file() {
            return canonical_or_self(candidate);
        }
    }
    for root in search_paths {
        let candidate = root.join(&rel);
        if candidate.is_file() {
            return canonical_or_self(candidate);
        }
    }
    None
}

fn force_virtual_std() -> bool {
    std::env::var_os("YARROW_LSP_FORCE_VIRTUAL_STD").is_some_and(|v| v != "0")
}

fn std_lib_roots(source_path: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let lsp_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(lsp_manifest.join("../yarrow-core/lib/std"));

    let mut cur = Path::new(source_path).parent();
    while let Some(dir) = cur {
        let nested = dir.join("crates/yarrow-core/lib/std");
        if nested.is_dir() {
            roots.push(nested);
        }
        let flat = dir.join("lib/std");
        if flat.is_dir() {
            roots.push(flat);
        }
        cur = dir.parent();
    }
    roots
}

fn canonical_or_self(path: PathBuf) -> Option<PathBuf> {
    Some(path.canonicalize().unwrap_or(path))
}

/// Byte span of a top-level function / method named `item` in `text`, if any.
pub fn item_name_span_in(source: &str, path_label: &str, item: &str) -> Option<yarrow_core::Span> {
    let session = Session::with_source_path(path_label.to_string());
    let parsed = session.parse_source(source.to_string()).ok()?;
    let source = parsed.file.source.as_str();
    for stmt in &parsed.program.items {
        match &stmt.kind {
            StmtKind::Function(f) if f.name == item => {
                return name_span_in(source, stmt.span, item);
            }
            StmtKind::Implement(imp) => {
                for f in &imp.functions {
                    if f.name == item {
                        return name_span_in(source, stmt.span, item);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn name_span_in(source: &str, span: yarrow_core::Span, name: &str) -> Option<yarrow_core::Span> {
    if name.is_empty() || span.lo > span.hi || span.hi > source.len() {
        return None;
    }
    let slice = &source[span.lo..span.hi];
    let rel = slice.find(name)?;
    let lo = span.lo + rel;
    let hi = lo + name.len();
    Some(yarrow_core::Span::new(lo, hi))
}
