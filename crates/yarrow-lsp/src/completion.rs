//! Keyword + same-file name completions (and `std.*` require paths).

use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Position,
};
use yarrow_core::parser::ast::{Function, Stmt, StmtKind};
use yarrow_core::{Program, SourceFile, Span};

use crate::config::LspConfig;
use crate::position::{PositionEncoding, PositionMap};

/// Grammar / tokenizer keywords offered as completions.
const KEYWORDS: &[&str] = &[
    "and",
    "or",
    "xor",
    "not",
    "lshift",
    "rshift",
    "typeof",
    "if",
    "else",
    "for",
    "match",
    "case",
    "unwrap",
    "handle",
    "function",
    "return",
    "call",
    "do",
    "with",
    "end",
    "const",
    "static",
    "mutable",
    "set",
    "public",
    "private",
    "copy",
    "error",
    "struct",
    "implement",
    "enum",
    "union",
    "pop",
    "drop",
    "dup",
    "rot",
    "unrot",
    "swap",
    "require",
    "defer",
    "borrow",
    "move",
    "load",
    "store",
    "unsafe",
    "fallback",
    "true",
    "false",
];

/// Known `lib/std/*.yar` module paths (do not invent beyond this table).
const STD_MODULES: &[&str] = &[
    "std.error",
    "std.fs",
    "std.io",
    "std.list",
    "std.loop",
    "std.map",
    "std.math",
    "std.mem",
    "std.region",
    "std.string",
];

/// Completions at `position` in `text` (prefix-filtered; may be empty).
pub fn completions(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    position: Position,
    config: &LspConfig,
) -> Option<CompletionResponse> {
    let file = SourceFile::new(path.to_string(), text.to_string());
    let map = PositionMap::from_file(&file, encoding);
    let offset = map.offset(position)?;

    if let Some(path_prefix) = require_path_prefix(text, offset) {
        let items = STD_MODULES
            .iter()
            .filter(|m| m.starts_with(&path_prefix))
            .map(|m| item(m.to_string(), CompletionItemKind::MODULE))
            .collect::<Vec<_>>();
        return Some(CompletionResponse::Array(items));
    }

    let prefix = identifier_prefix(text, offset);
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for kw in KEYWORDS {
        if kw.starts_with(prefix.as_str()) && seen.insert((*kw).to_string()) {
            items.push(item((*kw).to_string(), CompletionItemKind::KEYWORD));
        }
    }

    let session = config.session(path);
    if let Ok(parsed) = session.parse_source(text.to_string()) {
        let decls = collect_names(&parsed.file, &parsed.program);
        for name in visible_names(&decls, offset) {
            if name.starts_with(prefix.as_str()) && seen.insert(name.clone()) {
                items.push(item(name, CompletionItemKind::FUNCTION));
            }
        }
    }

    Some(CompletionResponse::Array(items))
}

fn item(label: String, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        ..CompletionItem::default()
    }
}

/// Partial identifier ending at `offset` (exclusive), possibly empty.
fn identifier_prefix(source: &str, offset: usize) -> String {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let mut start = offset;
    for (idx, c) in before.char_indices().rev() {
        if is_ident_char(c) {
            start = idx;
        } else {
            break;
        }
    }
    before[start..].to_string()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// If the cursor sits inside an open `"…` at line start (require-path style),
/// return the path text typed so far (from after `"` to `offset`).
fn require_path_prefix(source: &str, offset: usize) -> Option<String> {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let open = before.rfind('"')?;
    if before[open + 1..].contains('"') {
        return None;
    }
    let line_start = before[..open].rfind('\n').map(|i| i + 1).unwrap_or(0);
    if !before[line_start..open].chars().all(|c| c.is_whitespace()) {
        return None;
    }
    let prefix = &before[open + 1..];
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        return None;
    }
    Some(prefix.to_string())
}

#[derive(Debug, Clone)]
struct Named {
    name: String,
    scope: Span,
}

fn collect_names(file: &SourceFile, program: &Program) -> Vec<Named> {
    let source = file.source.as_str();
    let file_scope = Span::new(0, source.len());
    let mut out = Vec::new();
    for item in &program.items {
        collect_stmt(item, file_scope, &mut out);
    }
    out
}

fn collect_stmt(stmt: &Stmt, scope: Span, out: &mut Vec<Named>) {
    match &stmt.kind {
        StmtKind::Function(f) => {
            push_name(&f.name, scope, out);
            collect_function_body(f, stmt.span, out);
        }
        StmtKind::Struct(s) => {
            push_name(&s.name, scope, out);
            for field in &s.fields {
                push_name(&field.name, stmt.span, out);
            }
        }
        StmtKind::Enum(e) => {
            push_name(&e.name, scope, out);
            for member in &e.members {
                push_name(&member.name, stmt.span, out);
            }
        }
        StmtKind::Union(u) => {
            push_name(&u.name, scope, out);
        }
        StmtKind::Error(err) => {
            push_name(&err.name, scope, out);
            for member in &err.members {
                push_name(member, stmt.span, out);
            }
        }
        StmtKind::Implement(imp) => {
            push_name(&imp.target, scope, out);
            for f in &imp.functions {
                push_name(&f.name, scope, out);
                collect_function_body(f, stmt.span, out);
            }
        }
        StmtKind::Require { path, alias } => {
            let name = alias
                .as_deref()
                .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path));
            push_name(name, scope, out);
        }
        StmtKind::VarDecl { name, .. } => {
            push_name(name, scope, out);
        }
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_stmt(s, scope, out);
            }
            for s in else_branch {
                collect_stmt(s, scope, out);
            }
        }
        StmtKind::For { body, .. } => {
            for s in body {
                collect_stmt(s, scope, out);
            }
        }
        StmtKind::Match {
            cases, else_branch, ..
        } => {
            for case in cases {
                for s in &case.body {
                    collect_stmt(s, scope, out);
                }
            }
            for s in else_branch {
                collect_stmt(s, scope, out);
            }
        }
        StmtKind::Defer { body } | StmtKind::Unsafe { body } => {
            for s in body {
                collect_stmt(s, scope, out);
            }
        }
        StmtKind::Handle { body, .. } => {
            for s in body {
                collect_stmt(s, scope, out);
            }
        }
        _ => {}
    }
}

fn collect_function_body(function: &Function, function_span: Span, out: &mut Vec<Named>) {
    for stmt in &function.body {
        collect_stmt(stmt, function_span, out);
    }
}

fn push_name(name: &str, scope: Span, out: &mut Vec<Named>) {
    if name.is_empty() {
        return;
    }
    out.push(Named {
        name: name.to_string(),
        scope,
    });
}

fn visible_names(decls: &[Named], offset: usize) -> Vec<String> {
    decls
        .iter()
        .filter(|d| d.scope.lo <= offset && offset < d.scope.hi)
        .map(|d| d.name.clone())
        .collect()
}
