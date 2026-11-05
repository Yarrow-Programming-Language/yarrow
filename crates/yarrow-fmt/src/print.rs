//! Construct layout from `docs/STYLE_GUIDE.md` (Stages 6–14).
//!
//! Reprints the AST into preferred forms for requires, types, functions,
//! variables, containers, calls, control flow, defer, unsafe, and
//! handle/unwrap, with soft wrap at `max_width`. Own-line comments are
//! preserved by line gap with `# ` spacing; trailing comments are reattached
//! from trivia with one space before `#`. Top-level require sorting
//! ([`FormatOptions::sort_requires`], default on) and file-layout reorder
//! ([`FormatOptions::reorder_layout`], opt-in) apply when enabled.

use std::collections::HashSet;

use yarrow_core::parser::ast::{
    EnumDecl, ErrorDecl, Expr, Field, Function, Implement, MatchCase, MatchCaseKind, Mutability,
    ParamModifier, Parameter, Primitive, Stmt, StmtKind, StructDecl, Type, TypeKind, UnionDecl,
    Visibility,
};
use yarrow_core::{SourceFile, Span};

use crate::ByteRange;
use crate::FormatOptions;
use crate::comment::{normalize_comment, trailing_on_line, trailing_suffix};
use crate::ignore::{find_ignore_regions, has_ignore_markers};
use crate::ir::{FormatIr, TriviaMap};
use crate::layout::reorder_toplevel_indices;
use crate::phrase::{expr_tokens, layout_expr_stmts, wrap_tokens};
use crate::require::{require_group_blank, require_sort_key};

/// Reprint the program with Stage 6–14 construct layout.
///
/// Emits tab indentation and a single blank line between top-level items.
/// Idempotent when composed with hygiene / indent / blank on accepted inputs.
/// When the file has ignore markers, file-layout reorder is skipped so markers
/// stay paired; require runs that intersect an ignore region are not sorted.
pub fn apply_construct_layout(ir: &FormatIr, options: &FormatOptions) -> String {
    let mut p = Printer {
        file: &ir.file,
        trivia: &ir.trivia,
        max_width: options.effective_max_width(),
        out: String::with_capacity(ir.file.source.len().saturating_add(64)),
        depth: 0,
        last_emitted_line: 0,
        at_line_start: true,
        pending_blank: false,
        pending_trailing_line: None,
        emitted_trailing: HashSet::new(),
    };

    let items = &ir.program.items;
    let source = ir.file.source.as_str();
    let ignore_regions = find_ignore_regions(source);
    // Reorder would detach begin/end markers from their region; skip it.
    let reorder = options.reorder_layout && !has_ignore_markers(source);
    if reorder {
        p.print_reordered_toplevel(items, options.sort_requires);
    } else {
        let mut i = 0;
        while i < items.len() {
            if options.sort_requires && matches!(items[i].kind, StmtKind::Require { .. }) {
                let start = i;
                i += 1;
                while i < items.len() && matches!(items[i].kind, StmtKind::Require { .. }) {
                    i += 1;
                }
                let run = &items[start..i];
                let sort = !run_intersects_ignore(run, &ignore_regions);
                p.print_require_run(run, start > 0, sort);
                continue;
            }

            if i > 0 {
                // Requires stay one group; blank only before a non-require item
                // (or when leaving the require block).
                let prev_req = matches!(items[i - 1].kind, StmtKind::Require { .. });
                let cur_req = matches!(items[i].kind, StmtKind::Require { .. });
                p.pending_blank = !(prev_req && cur_req);
            }
            p.emit_gap_comments(items[i].span.line);
            p.print_stmt(&items[i]);
            p.finish_line();
            p.last_emitted_line = end_line(p.file, items[i].span).max(p.last_emitted_line);
            i += 1;
        }
    }

    p.emit_gap_comments(p.file.line_count().saturating_add(1));
    if !p.out.ends_with('\n') {
        p.out.push('\n');
    }
    p.out
}

fn run_intersects_ignore(run: &[Stmt], regions: &[ByteRange]) -> bool {
    run.iter().any(|stmt| {
        let span = ByteRange::new(stmt.span.lo, stmt.span.hi);
        regions.iter().any(|r| r.intersects(span))
    })
}

struct Printer<'a> {
    file: &'a SourceFile,
    trivia: &'a TriviaMap,
    max_width: usize,
    out: String,
    depth: usize,
    /// Last original source line whose content (or gap) was considered.
    last_emitted_line: usize,
    at_line_start: bool,
    pending_blank: bool,
    /// When set, [`finish_line`] appends that original line's trailing comment.
    pending_trailing_line: Option<usize>,
    emitted_trailing: HashSet<usize>,
}

impl<'a> Printer<'a> {
    fn emit_gap_comments(&mut self, until_line: usize) {
        let start = self.last_emitted_line.saturating_add(1);
        let last = self.file.line_count();
        if start == 0 || until_line <= start {
            return;
        }
        let end = until_line.min(last.saturating_add(1));
        for line in start..end {
            if line > last {
                break;
            }
            let raw = self.file.line_text(line);
            let trimmed = trim_indent(raw);
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                self.flush_pending_blank();
                self.write_indent();
                self.out.push_str(&normalize_comment(trimmed));
                self.out.push('\n');
                self.at_line_start = true;
                self.last_emitted_line = line;
            }
        }
    }

    fn flush_pending_blank(&mut self) {
        if self.pending_blank && !self.out.is_empty() && !self.out.ends_with("\n\n") {
            if !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            self.out.push('\n');
            self.at_line_start = true;
        }
        self.pending_blank = false;
    }

    /// Print a consecutive top-level require run. When `sort` is true, reorder
    /// std then local (alphabetical within each) and insert a blank between
    /// those groups. Own-line comments between requires move with the following
    /// require. Comments above the first require stay at block top unless a
    /// blank separates a trailing comment that documents that require.
    fn print_require_run(&mut self, group: &[Stmt], blank_before: bool, sort: bool) {
        if group.is_empty() {
            return;
        }

        let mut pre_group: Vec<String> = Vec::new();
        let mut entries: Vec<(Stmt, Vec<String>)> = Vec::with_capacity(group.len());
        for (idx, stmt) in group.iter().enumerate() {
            let lo = if idx == 0 {
                self.last_emitted_line.saturating_add(1)
            } else {
                end_line(self.file, group[idx - 1].span).saturating_add(1)
            };
            let hi = stmt.span.line;
            let mut gap: Vec<(usize, String)> = Vec::new();
            let mut last_blank: Option<usize> = None;
            if lo > 0 && lo < hi {
                for line in lo..hi {
                    if line > self.file.line_count() {
                        break;
                    }
                    let raw = self.file.line_text(line);
                    let trimmed = trim_indent(raw);
                    if trimmed.is_empty() {
                        last_blank = Some(line);
                    } else if trimmed.starts_with('#') {
                        gap.push((line, normalize_comment(trimmed)));
                    }
                }
            }

            let mut leads = Vec::new();
            if idx == 0 {
                if let Some(blank) = last_blank {
                    // Blank separates file/block header comments (stay put) from
                    // comments that document the first require (move with it).
                    for (line, comment) in gap {
                        if line < blank {
                            pre_group.push(comment);
                        } else {
                            leads.push(comment);
                        }
                    }
                } else {
                    // No blank: treat the whole gap as block header so sorting
                    // does not pull file comments under a reordered require.
                    pre_group.extend(gap.into_iter().map(|(_, c)| c));
                }
            } else {
                leads.extend(gap.into_iter().map(|(_, c)| c));
            }
            entries.push((stmt.clone(), leads));
        }

        if sort {
            entries.sort_by(|a, b| require_sort_key(&a.0).cmp(&require_sort_key(&b.0)));
        }

        if blank_before {
            self.pending_blank = true;
        }

        for comment in &pre_group {
            self.flush_pending_blank();
            self.write_indent();
            self.out.push_str(comment);
            self.out.push('\n');
            self.at_line_start = true;
        }
        // Keep a blank after block-header comments before the require group.
        if !pre_group.is_empty() {
            self.pending_blank = true;
        }

        for (j, (stmt, leads)) in entries.iter().enumerate() {
            if j > 0 {
                self.pending_blank = require_group_blank(&entries[j - 1].0, stmt);
            }
            for comment in leads {
                self.flush_pending_blank();
                self.write_indent();
                self.out.push_str(comment);
                self.out.push('\n');
                self.at_line_start = true;
            }
            self.print_stmt(stmt);
            self.finish_line();
        }

        let max_end = group
            .iter()
            .map(|s| end_line(self.file, s.span))
            .max()
            .unwrap_or(self.last_emitted_line);
        self.last_emitted_line = max_end.max(self.last_emitted_line);
    }

    /// Attach leading comments in source order, then emit items in style-guide
    /// file layout. Requires stay one group (optionally sorted).
    fn print_reordered_toplevel(&mut self, items: &[Stmt], sort_requires: bool) {
        if items.is_empty() {
            return;
        }

        let mut pre_group: Vec<String> = Vec::new();
        let mut entries: Vec<(Stmt, Vec<String>)> = Vec::with_capacity(items.len());
        for (idx, stmt) in items.iter().enumerate() {
            let lo = if idx == 0 {
                self.last_emitted_line.saturating_add(1)
            } else {
                end_line(self.file, items[idx - 1].span).saturating_add(1)
            };
            let hi = stmt.span.line;
            let mut gap: Vec<(usize, String)> = Vec::new();
            let mut last_blank: Option<usize> = None;
            if lo > 0 && lo < hi {
                for line in lo..hi {
                    if line > self.file.line_count() {
                        break;
                    }
                    let raw = self.file.line_text(line);
                    let trimmed = trim_indent(raw);
                    if trimmed.is_empty() {
                        last_blank = Some(line);
                    } else if trimmed.starts_with('#') {
                        gap.push((line, normalize_comment(trimmed)));
                    }
                }
            }

            let mut leads = Vec::new();
            if idx == 0 {
                if let Some(blank) = last_blank {
                    // Blank separates file header comments from comments that
                    // document the first top-level item.
                    for (line, comment) in gap {
                        if line < blank {
                            pre_group.push(comment);
                        } else {
                            leads.push(comment);
                        }
                    }
                } else {
                    // No blank: keep the gap as a file header so reorder/sort
                    // does not bury it under a moved item.
                    pre_group.extend(gap.into_iter().map(|(_, c)| c));
                }
            } else {
                leads.extend(gap.into_iter().map(|(_, c)| c));
            }
            entries.push((stmt.clone(), leads));
        }

        let order = reorder_toplevel_indices(items);
        let ordered: Vec<(Stmt, Vec<String>)> = order
            .into_iter()
            .map(|i| {
                let (stmt, leads) = entries[i].clone();
                (stmt, leads)
            })
            .collect();

        for comment in &pre_group {
            self.flush_pending_blank();
            self.write_indent();
            self.out.push_str(comment);
            self.out.push('\n');
            self.at_line_start = true;
        }
        if !pre_group.is_empty() {
            self.pending_blank = true;
        }

        let mut i = 0;
        while i < ordered.len() {
            if matches!(ordered[i].0.kind, StmtKind::Require { .. }) {
                let start = i;
                i += 1;
                while i < ordered.len() && matches!(ordered[i].0.kind, StmtKind::Require { .. }) {
                    i += 1;
                }
                let mut run = ordered[start..i].to_vec();
                if sort_requires {
                    run.sort_by(|a, b| require_sort_key(&a.0).cmp(&require_sort_key(&b.0)));
                }
                // Blank before the require block when something already printed.
                if start > 0 || !pre_group.is_empty() {
                    // start > 0 means prior non-require; pre_group already set pending.
                    if start > 0 {
                        self.pending_blank = true;
                    }
                }
                for (j, (stmt, leads)) in run.iter().enumerate() {
                    if j > 0 {
                        self.pending_blank = require_group_blank(&run[j - 1].0, stmt);
                    }
                    for comment in leads {
                        self.flush_pending_blank();
                        self.write_indent();
                        self.out.push_str(comment);
                        self.out.push('\n');
                        self.at_line_start = true;
                    }
                    self.print_stmt(stmt);
                    self.finish_line();
                }
                continue;
            }

            if i > 0 || !pre_group.is_empty() {
                self.pending_blank = true;
            }
            let (stmt, leads) = &ordered[i];
            for comment in leads {
                self.flush_pending_blank();
                self.write_indent();
                self.out.push_str(comment);
                self.out.push('\n');
                self.at_line_start = true;
            }
            self.print_stmt(stmt);
            self.finish_line();
            i += 1;
        }

        let max_end = items
            .iter()
            .map(|s| end_line(self.file, s.span))
            .max()
            .unwrap_or(self.last_emitted_line);
        self.last_emitted_line = max_end.max(self.last_emitted_line);
    }

    fn write_indent(&mut self) {
        if self.at_line_start {
            for _ in 0..self.depth {
                self.out.push('\t');
            }
            self.at_line_start = false;
        }
    }

    fn append_trailing_for_line(&mut self, line: usize) {
        if line == 0 || !self.emitted_trailing.insert(line) {
            return;
        }
        for comment in trailing_on_line(self.trivia, line) {
            self.out.push_str(&trailing_suffix(&comment.lexeme));
        }
    }

    fn finish_line(&mut self) {
        if !self.at_line_start {
            if let Some(line) = self.pending_trailing_line.take() {
                self.append_trailing_for_line(line);
            }
            self.out.push('\n');
            self.at_line_start = true;
        } else {
            self.pending_trailing_line = None;
        }
    }

    fn write_tokens_line(&mut self, tokens: &[String]) {
        self.write_tokens_line_at(self.depth, tokens, None);
    }

    fn write_tokens_line_trailing(&mut self, tokens: &[String], trailing_line: Option<usize>) {
        self.write_tokens_line_at(self.depth, tokens, trailing_line);
    }

    fn write_tokens_line_at(
        &mut self,
        depth: usize,
        tokens: &[String],
        trailing_line: Option<usize>,
    ) {
        if tokens.is_empty() {
            return;
        }
        self.flush_pending_blank();
        let saved = self.depth;
        self.depth = depth;
        self.write_indent();
        self.out.push_str(&tokens.join(" "));
        self.pending_trailing_line = trailing_line;
        self.finish_line();
        self.depth = saved;
    }

    fn write_phrase_lines(&mut self, lines: &[(usize, Vec<String>)]) {
        self.write_phrase_lines_trailing(lines, None);
    }

    fn write_phrase_lines_trailing(
        &mut self,
        lines: &[(usize, Vec<String>)],
        trailing_line: Option<usize>,
    ) {
        let last_idx = lines
            .iter()
            .rposition(|(_, tokens)| !tokens.is_empty())
            .unwrap_or(0);
        for (i, (depth, tokens)) in lines.iter().enumerate() {
            if tokens.is_empty() {
                continue;
            }
            let trail = if i == last_idx { trailing_line } else { None };
            self.write_tokens_line_at(*depth, tokens, trail);
        }
    }

    fn print_stmt(&mut self, stmt: &Stmt) {
        let trail = Some(end_line(self.file, stmt.span));
        match &stmt.kind {
            StmtKind::Require { path, alias } => self.print_require(path, alias.as_deref(), trail),
            StmtKind::Struct(decl) => self.print_struct(decl),
            StmtKind::Enum(decl) => self.print_enum(decl),
            StmtKind::Union(decl) => self.print_union(decl),
            StmtKind::Error(decl) => self.print_error(decl),
            StmtKind::Implement(impls) => self.print_implement(impls),
            StmtKind::Function(func) => self.print_function(func),
            StmtKind::VarDecl {
                name,
                mutability,
                ty,
                value,
            } => self.print_var_decl(name, *mutability, ty, value.as_ref(), trail),
            StmtKind::Set { target, value } => self.print_set(target, value.as_ref(), trail),
            StmtKind::Expr(expr) => {
                let tokens = expr_tokens(expr);
                self.write_tokens_line_trailing(&tokens, trail);
            }
            StmtKind::Return { value } => self.print_return(value.as_ref(), trail),
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.print_if(condition, then_branch, else_branch),
            StmtKind::For { source, body } => self.print_for(source, body),
            StmtKind::Match {
                value,
                cases,
                else_branch,
            } => self.print_match(value, cases, else_branch),
            StmtKind::Defer { body } => self.print_defer(body),
            StmtKind::Unsafe { body } => self.print_unsafe_block(body),
            StmtKind::Handle { body, fallback } => self.print_handle(body, fallback.as_ref()),
            StmtKind::Move { target, source } => {
                let mut tokens = expr_tokens(source);
                tokens.push(target.clone());
                tokens.push("move".into());
                self.write_tokens_line_trailing(&tokens, trail);
            }
            StmtKind::Fallback { value } => {
                let mut tokens = Vec::new();
                if let Some(v) = value {
                    tokens.extend(expr_tokens(v));
                }
                tokens.push("fallback".into());
                self.write_tokens_line_trailing(&tokens, trail);
            }
        }
    }

    fn print_require(&mut self, path: &str, alias: Option<&str>, trailing_line: Option<usize>) {
        let mut line = format!("\"{path}\"");
        if let Some(alias) = alias {
            line.push(' ');
            line.push_str(alias);
        }
        line.push_str(" require");
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&line);
        self.pending_trailing_line = trailing_line;
        self.finish_line();
    }

    fn print_struct(&mut self, decl: &StructDecl) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&decl.name);
        if let Some(vis) = decl.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        self.out.push_str(" struct");
        self.finish_line();

        self.depth += 1;
        for field in &decl.fields {
            self.print_field(field);
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_field(&mut self, field: &Field) {
        self.write_indent();
        self.out.push_str(&format_type(&field.ty));
        self.out.push(' ');
        self.out.push_str(&field.name);
        if let Some(vis) = field.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        // Fields have no stmt span; type location is the field line head.
        let line = field.ty.location.line.max(1);
        self.pending_trailing_line = Some(line);
        self.finish_line();
    }

    fn print_enum(&mut self, decl: &EnumDecl) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&decl.name);
        if let Some(ty) = &decl.underlying {
            self.out.push(' ');
            self.out.push_str(&format_type(ty));
        }
        if let Some(vis) = decl.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        self.out.push_str(" enum");
        self.finish_line();

        self.depth += 1;
        for member in &decl.members {
            self.write_indent();
            self.out.push_str(&member.name);
            if let Some(value) = &member.value {
                self.out.push(' ');
                self.out.push_str(value);
            }
            self.finish_line();
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_union(&mut self, decl: &UnionDecl) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&decl.name);
        if let Some(vis) = decl.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        self.out.push_str(" union");
        self.finish_line();

        self.depth += 1;
        for ty in &decl.types {
            self.write_indent();
            self.out.push_str(&format_type(ty));
            self.finish_line();
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_error(&mut self, decl: &ErrorDecl) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&decl.name);
        if let Some(inject) = &decl.inject {
            self.out.push(' ');
            self.out.push_str(inject);
        }
        if let Some(vis) = decl.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        self.out.push_str(" error");
        self.finish_line();

        self.depth += 1;
        for member in &decl.members {
            self.write_indent();
            self.out.push_str(member);
            self.finish_line();
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_implement(&mut self, impls: &Implement) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&impls.target);
        self.out.push_str(" implement");
        self.finish_line();

        self.depth += 1;
        for (i, func) in impls.functions.iter().enumerate() {
            if i > 0 {
                self.pending_blank = true;
            }
            self.print_function(func);
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_function(&mut self, func: &Function) {
        self.flush_pending_blank();
        self.write_indent();
        self.out.push_str(&func.name);
        if let Some(vis) = func.visibility {
            self.out.push(' ');
            self.out.push_str(visibility_word(vis));
        }
        if func.is_unsafe {
            self.out.push_str(" unsafe");
        }
        self.out.push_str(" function");

        if func.params.is_empty() {
            self.out.push_str(" do");
            self.finish_line();
        } else {
            self.finish_line();
            self.depth += 1;
            for param in &func.params {
                self.print_param(param);
            }
            self.depth -= 1;
            self.write_indent();
            self.out.push_str("do");
            self.finish_line();
        }

        self.depth += 1;
        self.print_body(&func.body);
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        if let Some(ret) = nonzero_return(&func.returns) {
            self.out.push_str(" with ");
            self.out.push_str(&format_type(ret));
        }
        self.finish_line();
    }

    fn print_param(&mut self, param: &Parameter) {
        self.write_indent();
        self.out.push_str(&format_type(&param.ty));
        match param.modifier {
            Some(ParamModifier::Copy) => self.out.push_str(" copy"),
            Some(ParamModifier::Mutable) => self.out.push_str(" mutable"),
            None => {}
        }
        self.finish_line();
    }

    fn print_body(&mut self, stmts: &[Stmt]) {
        let mut i = 0;
        // Do not inherit blanks from source gaps above the block opener
        // (`do` / `if` / `case` / `handle` / …).
        self.pending_blank = false;
        let mut prev_end_line = stmts
            .first()
            .map(|s| s.span.line.saturating_sub(1))
            .unwrap_or(self.last_emitted_line);
        self.last_emitted_line = prev_end_line;
        while i < stmts.len() {
            match &stmts[i].kind {
                StmtKind::Expr(_) => {
                    let start = i;
                    i += 1;
                    while i < stmts.len() && matches!(stmts[i].kind, StmtKind::Expr(_)) {
                        i += 1;
                    }
                    if had_blank_between(self.file, prev_end_line, stmts[start].span.line) {
                        self.pending_blank = true;
                    }
                    self.emit_gap_comments(stmts[start].span.line);

                    // Stage 7: attach `handle` / bare `return` to the preceding
                    // stack phrase (`call handle …`, `value return`).
                    match stmts.get(i).map(|s| &s.kind) {
                        Some(StmtKind::Handle { body, fallback }) => {
                            self.print_call_handle(&stmts[start..i], body, fallback.as_ref());
                            prev_end_line = end_line(self.file, stmts[i].span);
                            self.last_emitted_line = prev_end_line.max(self.last_emitted_line);
                            i += 1;
                        }
                        Some(StmtKind::Return { value: None }) => {
                            self.print_expr_phrase_with_return(&stmts[start..i]);
                            prev_end_line = end_line(self.file, stmts[i].span);
                            self.last_emitted_line = prev_end_line.max(self.last_emitted_line);
                            i += 1;
                        }
                        _ => {
                            self.print_expr_phrase(&stmts[start..i]);
                            if let Some(last) = stmts.get(i.saturating_sub(1)) {
                                prev_end_line = end_line(self.file, last.span);
                                self.last_emitted_line = prev_end_line.max(self.last_emitted_line);
                            }
                        }
                    }
                }
                _ => {
                    if had_blank_between(self.file, prev_end_line, stmts[i].span.line) {
                        self.pending_blank = true;
                    }
                    // Nested functions get a blank before the next sibling when
                    // the source had one; also prefer a blank after a nested
                    // function before non-function code (style-guide demo).
                    self.emit_gap_comments(stmts[i].span.line);
                    let is_nested_fn = matches!(stmts[i].kind, StmtKind::Function(_));
                    self.print_stmt(&stmts[i]);
                    prev_end_line = end_line(self.file, stmts[i].span);
                    self.last_emitted_line = prev_end_line.max(self.last_emitted_line);
                    if is_nested_fn && i + 1 < stmts.len() {
                        self.pending_blank = true;
                    }
                    i += 1;
                }
            }
        }
    }

    /// Stage 6–9: merge short calls; wrap over-width phrases; keep trailing.
    fn print_expr_phrase(&mut self, stmts: &[Stmt]) {
        let lines = layout_expr_stmts(stmts, self.depth, self.max_width);
        // Preserve blank/comment gaps from the first source line in the run.
        if let Some(first) = stmts.first() {
            let line_no = first.span.line.max(1);
            if had_blank_between(self.file, self.last_emitted_line, line_no) {
                self.pending_blank = true;
            }
            self.emit_gap_comments(line_no);
        }
        let trail = stmts.last().map(|s| end_line(self.file, s.span));
        self.write_phrase_lines_trailing(&lines, trail);
        if let Some(last) = stmts.last() {
            self.last_emitted_line = end_line(self.file, last.span).max(self.last_emitted_line);
        }
    }

    /// Same as [`print_expr_phrase`], then append bare `return` on the last line.
    fn print_expr_phrase_with_return(&mut self, stmts: &[Stmt]) {
        let mut lines = layout_expr_stmts(stmts, self.depth, self.max_width);
        if let Some((_, last)) = lines.last_mut() {
            last.push("return".into());
        } else {
            lines.push((self.depth, vec!["return".into()]));
        }
        // Re-wrap if appending `return` overflowed the soft width.
        if let Some((d, toks)) = lines.last()
            && *d + toks.join(" ").len() > self.max_width
        {
            let flat: Vec<String> = lines.iter().flat_map(|(_, t)| t.iter().cloned()).collect();
            lines = wrap_tokens(&flat, self.depth, self.max_width);
        }
        if let Some(first) = stmts.first() {
            let line_no = first.span.line.max(1);
            if had_blank_between(self.file, self.last_emitted_line, line_no) {
                self.pending_blank = true;
            }
            self.emit_gap_comments(line_no);
        }
        let trail = stmts.last().map(|s| end_line(self.file, s.span));
        self.write_phrase_lines_trailing(&lines, trail);
        if let Some(last) = stmts.last() {
            self.last_emitted_line = end_line(self.file, last.span).max(self.last_emitted_line);
        }
    }

    /// `call handle` / short `call handle <fb> fallback end` (Stage 7).
    fn print_call_handle(&mut self, call_stmts: &[Stmt], body: &[Stmt], fallback: Option<&Expr>) {
        let mut lines = layout_expr_stmts(call_stmts, self.depth, self.max_width);
        if lines.is_empty() {
            lines.push((self.depth, Vec::new()));
        }
        if let Some((_, last)) = lines.last_mut() {
            last.push("handle".into());
        }

        // Short form: empty handler body + fallback fits on one line.
        if body.is_empty()
            && let Some(fb) = fallback
        {
            let mut fb_tokens = phrase_tokens(fb);
            fb_tokens.retain(|t| !t.is_empty());
            fb_tokens.push("fallback".into());
            fb_tokens.push("end".into());
            if let Some((d, last)) = lines.last_mut() {
                let mut probe = last.clone();
                probe.extend(fb_tokens.iter().cloned());
                if *d + probe.join(" ").len() <= self.max_width {
                    last.extend(fb_tokens);
                    if let Some(first) = call_stmts.first() {
                        let line_no = first.span.line.max(1);
                        if had_blank_between(self.file, self.last_emitted_line, line_no) {
                            self.pending_blank = true;
                        }
                        self.emit_gap_comments(line_no);
                    }
                    let trail = call_stmts.last().map(|s| end_line(self.file, s.span));
                    self.write_phrase_lines_trailing(&lines, trail);
                    if let Some(last) = call_stmts.last() {
                        self.last_emitted_line =
                            end_line(self.file, last.span).max(self.last_emitted_line);
                    }
                    return;
                }
            }
        }

        if let Some(first) = call_stmts.first() {
            let line_no = first.span.line.max(1);
            if had_blank_between(self.file, self.last_emitted_line, line_no) {
                self.pending_blank = true;
            }
            self.emit_gap_comments(line_no);
        }
        let trail = call_stmts.last().map(|s| end_line(self.file, s.span));
        self.write_phrase_lines_trailing(&lines, trail);
        if let Some(last) = call_stmts.last() {
            self.last_emitted_line = end_line(self.file, last.span).max(self.last_emitted_line);
        }

        self.depth += 1;
        self.print_body(body);
        if let Some(fb) = fallback {
            let mut tokens = phrase_tokens(fb);
            tokens.retain(|t| !t.is_empty());
            tokens.push("fallback".into());
            self.write_tokens_line(&tokens);
        }
        self.depth -= 1;
        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_var_decl(
        &mut self,
        name: &str,
        mutability: Mutability,
        ty: &Type,
        value: Option<&Expr>,
        trailing_line: Option<usize>,
    ) {
        let value_tokens = match value {
            Some(v) => self.emit_absorbed_prefix(v),
            None => Vec::new(),
        };
        let mut tokens = value_tokens;
        tokens.push(name.into());
        tokens.push(mutability_word(mutability).into());
        tokens.push(format_type(ty));
        self.write_tokens_line_trailing(&tokens, trailing_line);
    }

    fn print_set(&mut self, target: &Expr, value: Option<&Expr>, trailing_line: Option<usize>) {
        let value_tokens = match value {
            Some(v) => self.emit_absorbed_prefix(v),
            None => Vec::new(),
        };
        let mut tokens = value_tokens;
        tokens.extend(expr_tokens(target));
        tokens.push("set".into());
        self.write_phrase_lines_trailing(
            &wrap_tokens(&tokens, self.depth, self.max_width),
            trailing_line,
        );
    }

    /// Emit stack phrases the parser folded into a consuming construct, and
    /// return the final line's tokens (the true initializer / condition tail).
    fn emit_absorbed_prefix(&mut self, expr: &Expr) -> Vec<String> {
        let lines = phrase_lines_spanned(expr);
        if lines.len() <= 1 {
            return phrase_tokens(expr);
        }
        for (spans, tokens) in &lines[..lines.len() - 1] {
            if let Some(first) = spans.first() {
                let line = first.line.max(1);
                if had_blank_between(self.file, self.last_emitted_line, line) {
                    self.pending_blank = true;
                }
                self.emit_gap_comments(line);
            }
            if !tokens.is_empty() {
                let trail = spans.last().map(|s| end_line(self.file, *s));
                self.write_tokens_line_trailing(tokens, trail);
            }
            if let Some(last) = spans.last() {
                let end = end_line(self.file, *last);
                self.last_emitted_line = end.max(self.last_emitted_line);
            }
        }
        let (spans, tokens) = &lines[lines.len() - 1];
        if let Some(first) = spans.first() {
            let line = first.line.max(1);
            if had_blank_between(self.file, self.last_emitted_line, line) {
                self.pending_blank = true;
            }
            self.emit_gap_comments(line);
        }
        tokens.clone()
    }

    fn print_return(&mut self, value: Option<&Expr>, trailing_line: Option<usize>) {
        let mut tokens = Vec::new();
        if let Some(v) = value {
            tokens.extend(phrase_tokens(v));
        }
        tokens.push("return".into());
        self.write_phrase_lines_trailing(
            &wrap_tokens(&tokens, self.depth, self.max_width),
            trailing_line,
        );
    }

    fn print_if(&mut self, condition: &Expr, then_branch: &[Stmt], else_branch: &[Stmt]) {
        // Condition may absorb prior stack lines via Seq; emit by original line,
        // then wrap if the `… if` head exceeds max_width.
        let cond_lines = phrase_lines(condition);
        if cond_lines.is_empty() {
            self.write_tokens_line(&["if".into()]);
        } else {
            let mut flat: Vec<String> = Vec::new();
            for (i, line) in cond_lines.iter().enumerate() {
                flat.extend(line.iter().cloned());
                if i + 1 == cond_lines.len() {
                    flat.push("if".into());
                }
            }
            // Prefer keeping original multi-line condition grouping when each
            // line fits; otherwise re-wrap the flat phrase before `if`.
            let joined_fits = self.depth + flat.join(" ").len() <= self.max_width;
            if joined_fits {
                self.write_tokens_line(&flat);
            } else if cond_lines.len() > 1 {
                for (i, line) in cond_lines.iter().enumerate() {
                    let mut tokens = line.clone();
                    if i + 1 == cond_lines.len() {
                        tokens.push("if".into());
                    }
                    let depth = if i == 0 { self.depth } else { self.depth + 1 };
                    self.write_phrase_lines(&wrap_tokens(&tokens, depth, self.max_width));
                }
            } else {
                self.write_phrase_lines(&wrap_tokens(&flat, self.depth, self.max_width));
            }
        }

        self.depth += 1;
        self.print_body(then_branch);
        self.depth -= 1;

        if !else_branch.is_empty() {
            self.flush_pending_blank();
            self.write_indent();
            self.out.push_str("else");
            self.finish_line();
            self.depth += 1;
            self.print_body(else_branch);
            self.depth -= 1;
        }

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_for(&mut self, source: &Expr, body: &[Stmt]) {
        let lines = phrase_lines(source);
        let mut flat = phrase_tokens(source);
        flat.push("for".into());
        if self.depth + flat.join(" ").len() <= self.max_width {
            self.write_tokens_line(&flat);
        } else if lines.len() > 1 {
            for (i, line) in lines.iter().enumerate() {
                let mut tokens = line.clone();
                if i + 1 == lines.len() {
                    tokens.push("for".into());
                }
                let depth = if i == 0 { self.depth } else { self.depth + 1 };
                self.write_phrase_lines(&wrap_tokens(&tokens, depth, self.max_width));
            }
        } else {
            self.write_phrase_lines(&wrap_tokens(&flat, self.depth, self.max_width));
        }

        self.depth += 1;
        self.print_body(body);
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_match(&mut self, value: &Expr, cases: &[MatchCase], else_branch: &[Stmt]) {
        let lines = phrase_lines(value);
        if lines.len() > 1 {
            for line in &lines[..lines.len() - 1] {
                let filtered: Vec<_> = line.iter().filter(|t| !t.is_empty()).cloned().collect();
                if !filtered.is_empty() {
                    self.write_tokens_line(&filtered);
                }
            }
            let mut last: Vec<_> = lines
                .last()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect();
            last.push("match".into());
            self.write_tokens_line(&last);
        } else {
            let mut tokens: Vec<_> = phrase_tokens(value)
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect();
            tokens.push("match".into());
            self.write_tokens_line(&tokens);
        }

        self.depth += 1;
        for (i, case) in cases.iter().enumerate() {
            if i > 0 {
                // Style guide: blank between multi-line cases; single-line stay tight.
                self.pending_blank = match_case_is_multiline(self.file, &cases[i - 1])
                    || match_case_is_multiline(self.file, case);
            }
            self.print_match_case(case);
        }
        if !else_branch.is_empty() {
            if !cases.is_empty() {
                let last_multi = cases
                    .last()
                    .is_some_and(|c| match_case_is_multiline(self.file, c));
                let else_multi = else_branch.len() > 1
                    || else_branch
                        .first()
                        .is_some_and(|s| end_line(self.file, s.span) > s.span.line);
                self.pending_blank = last_multi || else_multi;
            }
            self.flush_pending_blank();
            self.write_indent();
            self.out.push_str("else");
            self.finish_line();
            self.depth += 1;
            self.print_body(else_branch);
            self.depth -= 1;
            self.write_indent();
            self.out.push_str("end");
            self.finish_line();
        }
        self.depth -= 1;

        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_match_case(&mut self, case: &MatchCase) {
        match &case.kind {
            MatchCaseKind::Condition(expr) => {
                let lines = phrase_lines(expr);
                if lines.len() > 1 {
                    for line in &lines[..lines.len() - 1] {
                        self.write_tokens_line(line);
                    }
                    let mut last = lines.last().cloned().unwrap_or_default();
                    last.push("case".into());
                    self.write_tokens_line(&last);
                } else {
                    let mut tokens = phrase_tokens(expr);
                    tokens.push("case".into());
                    self.write_tokens_line(&tokens);
                }
            }
            MatchCaseKind::Type(ty) => {
                self.write_tokens_line(&[format_type(ty), "case".into()]);
            }
        }
        self.depth += 1;
        self.print_body(&case.body);
        self.depth -= 1;
        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_defer(&mut self, body: &[Stmt]) {
        // One-line when the body is only stack phrases and fits under width.
        if !body.is_empty() && body.iter().all(|s| matches!(s.kind, StmtKind::Expr(_))) {
            let mut tokens: Vec<String> = body
                .iter()
                .flat_map(|s| match &s.kind {
                    StmtKind::Expr(e) => expr_tokens(e),
                    _ => unreachable!(),
                })
                .filter(|t| !t.is_empty())
                .collect();
            let joined = tokens.join(" ");
            if self.depth + "defer ".len() + joined.len() + " end".len() <= self.max_width {
                let mut line = vec!["defer".into()];
                line.append(&mut tokens);
                line.push("end".into());
                self.write_tokens_line(&line);
                return;
            }
        }

        self.write_indent();
        self.out.push_str("defer");
        self.finish_line();
        self.depth += 1;
        self.print_body(body);
        self.depth -= 1;
        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_unsafe_block(&mut self, body: &[Stmt]) {
        self.write_indent();
        self.out.push_str("unsafe");
        self.finish_line();
        self.depth += 1;
        self.print_body(body);
        self.depth -= 1;
        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }

    fn print_handle(&mut self, body: &[Stmt], fallback: Option<&Expr>) {
        // Standalone `handle` (no preceding call phrase in this stmt list).
        self.write_indent();
        self.out.push_str("handle");
        self.finish_line();
        self.depth += 1;
        self.print_body(body);
        if let Some(fb) = fallback {
            let mut tokens = phrase_tokens(fb);
            tokens.retain(|t| !t.is_empty());
            tokens.push("fallback".into());
            self.write_tokens_line(&tokens);
        }
        self.depth -= 1;
        self.write_indent();
        self.out.push_str("end");
        self.finish_line();
    }
}

fn trim_indent(line: &str) -> &str {
    line.trim_start_matches([' ', '\t'])
}

fn is_blank_line(line: &str) -> bool {
    trim_indent(line).is_empty()
}

fn had_blank_between(file: &SourceFile, prev_end_line: usize, next_start_line: usize) -> bool {
    if prev_end_line == 0 || next_start_line <= prev_end_line + 1 {
        return false;
    }
    let last = file.line_count();
    ((prev_end_line + 1)..next_start_line)
        .filter(|l| *l <= last)
        .any(|l| is_blank_line(file.line_text(l)))
}

fn end_line(file: &SourceFile, span: Span) -> usize {
    if span.hi == 0 {
        return span.line;
    }
    file.location(span.hi.saturating_sub(1)).line
}

fn match_case_is_multiline(file: &SourceFile, case: &MatchCase) -> bool {
    if case.body.is_empty() {
        return false;
    }
    if case.body.len() > 1 {
        return true;
    }
    let body = &case.body[0];
    let body_end = end_line(file, body.span);
    if body_end > body.span.line {
        return true;
    }
    // Single-line body: multi-line case when the body sits below the case head.
    let head_line = match &case.kind {
        MatchCaseKind::Condition(expr) => condition_head_line(expr),
        MatchCaseKind::Type(ty) => ty.location.line.max(1),
    };
    body.span.line > head_line
}

fn condition_head_line(expr: &Expr) -> usize {
    match expr {
        Expr::Seq(elems) if !elems.is_empty() => {
            elems.iter().map(|(_, s)| s.line.max(1)).min().unwrap_or(1)
        }
        _ => 1,
    }
}

fn visibility_word(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Private => "private",
    }
}

fn mutability_word(m: Mutability) -> &'static str {
    match m {
        Mutability::Mutable => "mutable",
        Mutability::Const => "const",
        Mutability::Static => "static",
    }
}

fn nonzero_return(returns: &[Type]) -> Option<&Type> {
    match returns {
        [] => None,
        [ty] => match &ty.kind {
            TypeKind::Primitive(Primitive::Void) => None,
            _ => Some(ty),
        },
        [ty, ..] => Some(ty),
    }
}

pub(crate) fn format_type(ty: &Type) -> String {
    match &ty.kind {
        TypeKind::Named(name) => name.clone(),
        TypeKind::Primitive(p) => primitive_name(*p).into(),
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

/// Tokens for a stack phrase, flattening `Seq` in source order.
fn phrase_tokens(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Seq(elems) => elems.iter().flat_map(|(e, _)| expr_tokens(e)).collect(),
        other => expr_tokens(other),
    }
}

/// Group a phrase into lines using original `Seq` element spans when present.
fn phrase_lines(expr: &Expr) -> Vec<Vec<String>> {
    phrase_lines_spanned(expr)
        .into_iter()
        .map(|(_, tokens)| tokens)
        .collect()
}

fn phrase_lines_spanned(expr: &Expr) -> Vec<(Vec<Span>, Vec<String>)> {
    match expr {
        Expr::Seq(elems) if !elems.is_empty() => {
            let mut lines: Vec<(Vec<Span>, Vec<String>)> = Vec::new();
            let mut cur_line = 0usize;
            for (e, span) in elems {
                let line = span.line.max(1);
                if line != cur_line {
                    lines.push((Vec::new(), Vec::new()));
                    cur_line = line;
                }
                if let Some((spans, tokens)) = lines.last_mut() {
                    spans.push(*span);
                    tokens.extend(expr_tokens(e));
                }
            }
            lines.retain(|(_, t)| !t.is_empty());
            if lines.is_empty() {
                vec![(Vec::new(), phrase_tokens(expr))]
            } else {
                lines
            }
        }
        other => vec![(Vec::new(), expr_tokens(other))],
    }
}
