//! Document outlines and workspace quick-open symbols.

use std::collections::HashSet;
use std::path::PathBuf;

use tower_lsp_server::ls_types::{
    DocumentSymbol, Location, Range, SymbolInformation, SymbolKind, Uri,
};
use yarrow_core::parser::ast::{Function, Stmt, StmtKind};
use yarrow_core::{Program, SourceFile, Span};

use crate::analysis::uri_to_source_path;
use crate::config::LspConfig;
use crate::modules;
use crate::position::{PositionEncoding, PositionMap};

/// Cap for `workspace/symbol` so huge buffers stay responsive.
pub const WORKSPACE_SYMBOL_LIMIT: usize = 100;

/// Parse `text` and return hierarchical document symbols, or `None` on parse failure.
pub fn document_symbols(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    config: &LspConfig,
) -> Option<Vec<DocumentSymbol>> {
    let session = config.session(path);
    let parsed = session.parse_source(text.to_string()).ok()?;
    Some(program_symbols(&parsed.file, &parsed.program, encoding))
}

/// Search open buffers (and on-disk `require` targets already resolvable from them).
///
/// Empty `query` returns a bounded list of collected symbols (capped at
/// [`WORKSPACE_SYMBOL_LIMIT`]). Closed or unchecked trees are not crawled.
pub fn workspace_symbols(
    open: &[(Uri, String)],
    query: &str,
    encoding: PositionEncoding,
    config: &LspConfig,
) -> Vec<SymbolInformation> {
    let mut out = Vec::new();
    let mut open_paths: HashSet<PathBuf> = HashSet::new();
    let mut require_queue: Vec<PathBuf> = Vec::new();

    for (uri, text) in open {
        let path = uri_to_source_path(uri);
        open_paths.insert(canonical_path(&path));
        collect_workspace_from_text(
            uri,
            &path,
            text,
            encoding,
            config,
            &mut out,
            Some(&mut require_queue),
        );
    }

    // Resolved requires only (no filesystem walk). Skip paths already open.
    let mut visited_requires: HashSet<PathBuf> = HashSet::new();
    for req_path in require_queue {
        let canon = canonical_path_buf(&req_path);
        if !visited_requires.insert(canon.clone()) || open_paths.contains(&canon) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&req_path) else {
            continue;
        };
        let Some(uri) = Uri::from_file_path(&req_path) else {
            continue;
        };
        let path_str = req_path.to_string_lossy().into_owned();
        collect_workspace_from_text(
            &uri, &path_str, &text, encoding, config, &mut out,
            None, // do not transitive-crawl further requires
        );
    }

    let q = query.to_ascii_lowercase();
    let mut filtered: Vec<SymbolInformation> = out
        .into_iter()
        .filter(|s| q.is_empty() || s.name.to_ascii_lowercase().contains(&q))
        .collect();

    // Prefer prefix matches, then shorter names, then name, then URI.
    filtered.sort_by(|a, b| {
        let a_pref = !q.is_empty() && a.name.to_ascii_lowercase().starts_with(&q);
        let b_pref = !q.is_empty() && b.name.to_ascii_lowercase().starts_with(&q);
        b_pref
            .cmp(&a_pref)
            .then(a.name.len().cmp(&b.name.len()))
            .then(a.name.cmp(&b.name))
            .then(a.location.uri.as_str().cmp(b.location.uri.as_str()))
    });
    filtered.truncate(WORKSPACE_SYMBOL_LIMIT);
    filtered
}

fn canonical_path(path: &str) -> PathBuf {
    PathBuf::from(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
}

fn canonical_path_buf(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn collect_workspace_from_text(
    uri: &Uri,
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    config: &LspConfig,
    out: &mut Vec<SymbolInformation>,
    mut require_queue: Option<&mut Vec<PathBuf>>,
) {
    let session = config.session(path);
    let Ok(parsed) = session.parse_source(text.to_string()) else {
        return;
    };
    let map = PositionMap::from_file(&parsed.file, encoding);
    let source = parsed.file.source.as_str();
    for item in &parsed.program.items {
        match &item.kind {
            StmtKind::Function(f) => {
                push_workspace_symbol(
                    source,
                    &map,
                    uri,
                    item.span,
                    &f.name,
                    SymbolKind::FUNCTION,
                    out,
                );
            }
            StmtKind::Struct(s) => {
                push_workspace_symbol(
                    source,
                    &map,
                    uri,
                    item.span,
                    &s.name,
                    SymbolKind::STRUCT,
                    out,
                );
            }
            StmtKind::Enum(e) => {
                push_workspace_symbol(source, &map, uri, item.span, &e.name, SymbolKind::ENUM, out);
            }
            StmtKind::Union(u) => {
                push_workspace_symbol(
                    source,
                    &map,
                    uri,
                    item.span,
                    &u.name,
                    SymbolKind::CLASS,
                    out,
                );
            }
            StmtKind::Error(err) => {
                push_workspace_symbol(
                    source,
                    &map,
                    uri,
                    item.span,
                    &err.name,
                    SymbolKind::CLASS,
                    out,
                );
            }
            StmtKind::Implement(imp) => {
                for f in &imp.functions {
                    push_workspace_symbol(
                        source,
                        &map,
                        uri,
                        item.span,
                        &f.name,
                        SymbolKind::METHOD,
                        out,
                    );
                }
            }
            StmtKind::Require { path: req, .. } => {
                if let Some(queue) = require_queue.as_mut()
                    && let Some(target) =
                        modules::resolve_require_file(path, req, &config.search_paths)
                    && let Some(disk) = target.path
                {
                    queue.push(disk);
                }
            }
            _ => {}
        }
    }
}

#[allow(deprecated)]
fn push_workspace_symbol(
    source: &str,
    map: &PositionMap<'_>,
    uri: &Uri,
    span: Span,
    name: &str,
    kind: SymbolKind,
    out: &mut Vec<SymbolInformation>,
) {
    if name.is_empty() {
        return;
    }
    let range = name_selection(source, map, span, name).unwrap_or_else(|| map.range(span));
    out.push(SymbolInformation {
        name: name.to_string(),
        kind,
        tags: None,
        deprecated: None,
        location: Location::new(uri.clone(), range),
        container_name: None,
    });
}

fn program_symbols(
    file: &SourceFile,
    program: &Program,
    encoding: PositionEncoding,
) -> Vec<DocumentSymbol> {
    let map = PositionMap::from_file(file, encoding);
    let source = file.source.as_str();
    program
        .items
        .iter()
        .filter_map(|item| stmt_symbol(source, &map, item))
        .collect()
}

fn stmt_symbol(source: &str, map: &PositionMap<'_>, stmt: &Stmt) -> Option<DocumentSymbol> {
    match &stmt.kind {
        StmtKind::Function(f) => Some(function_symbol(source, map, f, stmt.span)),
        StmtKind::Struct(s) => {
            let children = s
                .fields
                .iter()
                .filter_map(|field| {
                    name_only_symbol(source, map, stmt.span, &field.name, SymbolKind::FIELD)
                })
                .collect::<Vec<_>>();
            Some(named_symbol(
                source,
                map,
                stmt.span,
                s.name.clone(),
                SymbolKind::STRUCT,
                children,
            ))
        }
        StmtKind::Enum(e) => {
            let children = e
                .members
                .iter()
                .filter_map(|member| {
                    name_only_symbol(
                        source,
                        map,
                        stmt.span,
                        &member.name,
                        SymbolKind::ENUM_MEMBER,
                    )
                })
                .collect::<Vec<_>>();
            Some(named_symbol(
                source,
                map,
                stmt.span,
                e.name.clone(),
                SymbolKind::ENUM,
                children,
            ))
        }
        StmtKind::Union(u) => Some(named_symbol(
            source,
            map,
            stmt.span,
            u.name.clone(),
            SymbolKind::CLASS,
            Vec::new(),
        )),
        StmtKind::Error(err) => {
            let children = err
                .members
                .iter()
                .filter_map(|member| {
                    name_only_symbol(source, map, stmt.span, member, SymbolKind::ENUM_MEMBER)
                })
                .collect::<Vec<_>>();
            Some(named_symbol(
                source,
                map,
                stmt.span,
                err.name.clone(),
                SymbolKind::CLASS,
                children,
            ))
        }
        StmtKind::Implement(imp) => {
            let children = imp
                .functions
                .iter()
                .filter_map(|f| {
                    name_only_symbol(source, map, stmt.span, &f.name, SymbolKind::METHOD)
                })
                .collect::<Vec<_>>();
            Some(named_symbol(
                source,
                map,
                stmt.span,
                imp.target.clone(),
                SymbolKind::NAMESPACE,
                children,
            ))
        }
        StmtKind::Require { path, alias } => {
            let name = alias
                .clone()
                .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path).to_string());
            Some(named_symbol(
                source,
                map,
                stmt.span,
                name,
                SymbolKind::MODULE,
                Vec::new(),
            ))
        }
        _ => None,
    }
}

fn function_symbol(
    source: &str,
    map: &PositionMap<'_>,
    function: &Function,
    span: Span,
) -> DocumentSymbol {
    let children = function
        .body
        .iter()
        .filter_map(|inner| match &inner.kind {
            StmtKind::Function(nested) => Some(function_symbol(source, map, nested, inner.span)),
            _ => None,
        })
        .collect();
    named_symbol(
        source,
        map,
        span,
        function.name.clone(),
        SymbolKind::FUNCTION,
        children,
    )
}

fn named_symbol(
    source: &str,
    map: &PositionMap<'_>,
    span: Span,
    name: String,
    kind: SymbolKind,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    let range = map.range(span);
    let selection_range = name_selection(source, map, span, &name).unwrap_or(range);
    make_symbol(name, kind, range, selection_range, children)
}

fn name_only_symbol(
    source: &str,
    map: &PositionMap<'_>,
    parent: Span,
    name: &str,
    kind: SymbolKind,
) -> Option<DocumentSymbol> {
    let selection = name_selection(source, map, parent, name)?;
    Some(make_symbol(
        name.to_string(),
        kind,
        selection,
        selection,
        Vec::new(),
    ))
}

fn name_selection(source: &str, map: &PositionMap<'_>, span: Span, name: &str) -> Option<Range> {
    if name.is_empty() || span.lo > span.hi || span.hi > source.len() {
        return None;
    }
    let slice = &source[span.lo..span.hi];
    let rel = slice.find(name)?;
    let lo = span.lo + rel;
    let hi = lo + name.len();
    Some(map.range(Span::new(lo, hi)))
}

#[allow(deprecated)]
fn make_symbol(
    name: String,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}
