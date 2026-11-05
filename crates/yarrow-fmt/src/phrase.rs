//! Stack-phrase layout: space-separated tokens, soft wrap at `max_width`.
//!
//! Stage 8: keep a call (or short arithmetic) on one line when it fits;
//! otherwise break before consuming words and indent continuations one tab
//! deeper than the phrase start (`docs/STYLE_GUIDE.md`).

use yarrow_core::parser::ast::{BinOp, Expr, StackOp, Stmt, StmtKind, UnOp};

/// One emitted line: nesting depth (tabs) and space-separated tokens.
pub type PhraseLine = (usize, Vec<String>);

/// Layout consecutive expression statements as stack phrases.
///
/// A call phrase becomes one line when the full phrase fits under `max_width`;
/// otherwise it wraps into units (one arg per line, callee+`call` together)
/// with continuations at `depth + 1`. Partial merges are avoided so layout is
/// idempotent.
pub fn layout_expr_stmts(stmts: &[Stmt], depth: usize, max_width: usize) -> Vec<PhraseLine> {
    if stmts.is_empty() {
        return Vec::new();
    }

    let words: Vec<(usize, Vec<String>)> = stmts
        .iter()
        .map(|s| {
            let expr = match &s.kind {
                StmtKind::Expr(e) => e,
                _ => unreachable!("layout_expr_stmts expects Expr stmts"),
            };
            let mut tokens = expr_tokens(expr);
            tokens.retain(|t| !t.is_empty());
            (s.span.line.max(1), tokens)
        })
        .collect();

    let mut lines: Vec<(usize, Vec<String>)> = Vec::new();
    let mut cur_line = 0usize;
    for (line, tokens) in &words {
        if lines.is_empty() || *line != cur_line {
            lines.push((*line, Vec::new()));
            cur_line = *line;
        }
        if let Some((_, last)) = lines.last_mut() {
            last.extend(tokens.iter().cloned());
        }
    }

    // All-or-nothing merge into call / unwrap tails.
    let indent_cols = depth;
    let mut i = 0;
    while i < lines.len() {
        if is_call_tail(&lines[i].1) {
            let mut start = i;
            while start > 0 && !is_phrase_end(&lines[start - 1].1) {
                start -= 1;
            }
            if start < i {
                let mut merged = Vec::new();
                for (_, line) in &lines[start..=i] {
                    merged.extend(line.iter().cloned());
                }
                if indent_cols + merged.join(" ").len() <= max_width {
                    let line_no = lines[start].0;
                    lines[start] = (line_no, merged);
                    lines.drain(start + 1..=i);
                    i = start;
                }
            }
        }
        i += 1;
    }

    // Emit lines. Prefer whole call-phrase groups: either one merged line or
    // one wrap of the flattened run (never emit args before seeing the call).
    let n = lines.len();
    let mut group_of = vec![None; n];
    let mut g = 0usize;
    let mut i = 0;
    while i < n {
        if is_call_tail(&lines[i].1) {
            let mut start = i;
            while start > 0 && !is_phrase_end(&lines[start - 1].1) && group_of[start - 1].is_none()
            {
                start -= 1;
            }
            for slot in group_of.iter_mut().take(i + 1).skip(start) {
                *slot = Some(g);
            }
            g += 1;
            i += 1;
        } else {
            i += 1;
        }
    }

    let mut out: Vec<PhraseLine> = Vec::new();
    let mut emitted = vec![false; n];
    for i in 0..n {
        if emitted[i] {
            continue;
        }
        if let Some(gid) = group_of[i] {
            let mut flat = Vec::new();
            for (idx, og) in group_of.iter().enumerate() {
                if *og == Some(gid) {
                    flat.extend(lines[idx].1.iter().cloned());
                    emitted[idx] = true;
                }
            }
            out.extend(wrap_tokens(&flat, depth, max_width));
        } else {
            emitted[i] = true;
            out.extend(wrap_tokens(&lines[i].1, depth, max_width));
        }
    }
    out
}

/// Wrap a flat token list to `max_width`, breaking before consuming words.
pub fn wrap_tokens(tokens: &[String], depth: usize, max_width: usize) -> Vec<PhraseLine> {
    let tokens: Vec<String> = tokens.iter().filter(|t| !t.is_empty()).cloned().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    if phrase_fits(&tokens, depth, max_width) {
        return vec![(depth, tokens)];
    }

    let units = phrase_units(&tokens);
    let mut out: Vec<PhraseLine> = Vec::new();
    for (i, unit) in units.into_iter().enumerate() {
        let line_depth = if i == 0 { depth } else { depth + 1 };
        if phrase_fits(&unit, line_depth, max_width) {
            out.push((line_depth, unit));
        } else {
            out.extend(wrap_oversized_unit(&unit, line_depth, max_width));
        }
    }
    out
}

fn phrase_fits(tokens: &[String], depth: usize, max_width: usize) -> bool {
    depth + tokens.join(" ").len() <= max_width
}

fn phrase_units(tokens: &[String]) -> Vec<Vec<String>> {
    if let Some((args, tail)) = split_call_tail(tokens) {
        let mut units = unitize_prefix(args);
        units.push(tail);
        return units;
    }
    unitize_prefix(tokens)
}

fn split_call_tail(tokens: &[String]) -> Option<(&[String], Vec<String>)> {
    let n = tokens.len();
    if n >= 3 && tokens[n - 1] == "unwrap" && tokens[n - 2] == "call" {
        let tail = tokens[n - 3..].to_vec();
        return Some((&tokens[..n - 3], tail));
    }
    if n >= 2 && tokens[n - 1] == "call" {
        let tail = tokens[n - 2..].to_vec();
        return Some((&tokens[..n - 2], tail));
    }
    None
}

fn unitize_prefix(tokens: &[String]) -> Vec<Vec<String>> {
    let mut units: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for tok in tokens {
        cur.push(tok.clone());
        if is_consuming_word(tok) {
            units.push(std::mem::take(&mut cur));
        }
    }
    for tok in cur {
        units.push(vec![tok]);
    }
    units
}

fn wrap_oversized_unit(unit: &[String], depth: usize, max_width: usize) -> Vec<PhraseLine> {
    if unit.len() >= 2 {
        let last = unit.last().map(String::as_str).unwrap_or("");
        if last == "unwrap" && unit[unit.len() - 2] == "call" {
            let without_unwrap = &unit[..unit.len() - 1];
            let mut out = wrap_tokens(without_unwrap, depth, max_width);
            out.push((depth + 1, vec!["unwrap".into()]));
            return out;
        }
        if is_consuming_word(last) {
            let head = &unit[..unit.len() - 1];
            let mut out = wrap_tokens(head, depth, max_width);
            out.push((depth + 1, vec![last.into()]));
            return out;
        }
    }
    unit.iter()
        .enumerate()
        .map(|(i, tok)| {
            let d = if i == 0 { depth } else { depth + 1 };
            (d, vec![tok.clone()])
        })
        .collect()
}

fn is_call_tail(tokens: &[String]) -> bool {
    matches!(tokens.last().map(String::as_str), Some("call" | "unwrap"))
}

/// A line that already ends a stack phrase (do not merge into a following call).
pub fn is_phrase_end(tokens: &[String]) -> bool {
    match tokens.last().map(String::as_str) {
        Some(
            "call" | "unwrap" | "drop" | "pop" | "dup" | "swap" | "rot" | "unrot" | "borrow"
            | "typeof" | "load" | "store" | "return" | "set" | "move" | "fallback",
        ) => true,
        Some(w) if is_consuming_word(w) => true,
        _ => false,
    }
}

/// Words that consume stack values; preferred break points when wrapping.
pub fn is_consuming_word(w: &str) -> bool {
    matches!(
        w,
        "call"
            | "unwrap"
            | "set"
            | "return"
            | "drop"
            | "pop"
            | "dup"
            | "swap"
            | "rot"
            | "unrot"
            | "borrow"
            | "typeof"
            | "load"
            | "store"
            | "move"
            | "fallback"
            | "not"
            | "+"
            | "-"
            | "*"
            | "/"
            | "//"
            | "%"
            | "^"
            | "~"
            | "=="
            | "!="
            | ">"
            | ">="
            | "<"
            | "<="
            | "and"
            | "or"
            | "xor"
            | "lshift"
            | "rshift"
    )
}

/// Source line and paint depth for each distinct line in an expression run.
///
/// Prefers continuation depth already emitted by construct layout (leading
/// tabs deeper than `depth`). Falls back to width-based wrap detection when
/// the file was not yet wrapped.
pub fn paint_expr_run_depths(
    stmts: &[Stmt],
    depth: usize,
    max_width: usize,
    file: &yarrow_core::SourceFile,
) -> Vec<(usize, usize)> {
    if stmts.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<(usize, Vec<String>)> = Vec::new();
    let mut cur_line = 0usize;
    for s in stmts {
        let expr = match &s.kind {
            StmtKind::Expr(e) => e,
            _ => continue,
        };
        let line = s.span.line.max(1);
        let mut tokens = expr_tokens(expr);
        tokens.retain(|t| !t.is_empty());
        if lines.is_empty() || line != cur_line {
            lines.push((line, Vec::new()));
            cur_line = line;
        }
        if let Some((_, last)) = lines.last_mut() {
            last.extend(tokens);
        }
    }
    lines.retain(|(_, t)| !t.is_empty());
    if lines.is_empty() {
        return Vec::new();
    }

    // Construct layout already indented continuations one tab deeper.
    let any_continuation = lines
        .iter()
        .any(|(line, _)| leading_tabs(file.line_text(*line)) > depth);
    if any_continuation {
        return lines
            .into_iter()
            .map(|(line, _)| {
                let tabs = leading_tabs(file.line_text(line));
                let d = if tabs > depth { depth + 1 } else { depth };
                (line, d)
            })
            .collect();
    }

    // Fallback when source is still flat: detect over-width single phrases.
    let flat: Vec<String> = lines.iter().flat_map(|(_, t)| t.iter().cloned()).collect();
    let over_width = depth + flat.join(" ").len() > max_width;
    let single_phrase = lines.len() > 1
        && lines[..lines.len() - 1]
            .iter()
            .all(|(_, t)| !is_phrase_end(t));
    let wrapped = over_width && single_phrase;

    lines
        .into_iter()
        .enumerate()
        .map(|(i, (line, _))| {
            let d = if wrapped && i > 0 { depth + 1 } else { depth };
            (line, d)
        })
        .collect()
}

fn leading_tabs(line: &str) -> usize {
    line.chars().take_while(|c| *c == '\t').count()
}

/// Tokens for a single expression (no `Seq` line grouping).
pub fn expr_tokens(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Integer { value }
        | Expr::Float { value }
        | Expr::String { value }
        | Expr::Rune { value } => {
            vec![value.clone()]
        }
        Expr::Bool { value } => vec![if *value { "true" } else { "false" }.into()],
        Expr::Variable { name } => vec![name.clone()],
        // Stage 5: the type value carries the full type; it prints as
        // one word (`i32`, `array<i32 3>`, `|i32 string|`).
        Expr::TypeValue { ty } => vec![crate::print::format_type(ty)],
        Expr::Member { base, member } => {
            let mut tokens = expr_tokens(base);
            if let Some(last) = tokens.last_mut() {
                last.push('.');
                last.push_str(member);
            } else {
                tokens.push(format!(".{member}"));
            }
            tokens
        }
        Expr::Binary { op, left, right } => {
            let mut tokens = expr_tokens(left);
            tokens.extend(expr_tokens(right));
            tokens.push(binop_word(*op).into());
            tokens
        }
        Expr::Unary { op, operand } => {
            let mut tokens = expr_tokens(operand);
            tokens.push(unop_word(*op).into());
            tokens
        }
        Expr::Call { target } => {
            let mut tokens = expr_tokens(target);
            tokens.push("call".into());
            tokens
        }
        Expr::Unwrap { inner } => {
            let mut tokens = expr_tokens(inner);
            tokens.push("unwrap".into());
            tokens
        }
        Expr::Typeof { inner } => {
            let mut tokens = expr_tokens(inner);
            tokens.push("typeof".into());
            tokens
        }
        Expr::Borrow { inner } => {
            let mut tokens = expr_tokens(inner);
            tokens.push("borrow".into());
            tokens
        }
        Expr::Load { inner } => {
            let mut tokens = expr_tokens(inner);
            tokens.push("load".into());
            tokens
        }
        Expr::Store { addr, value } => {
            let mut tokens = expr_tokens(addr);
            tokens.extend(expr_tokens(value));
            tokens.push("store".into());
            tokens
        }
        Expr::ApplyBin(op) => vec![binop_word(*op).into()],
        Expr::ApplyUn(op) => vec![unop_word(*op).into()],
        Expr::ApplyTypeof => vec!["typeof".into()],
        Expr::ApplyBorrow => vec!["borrow".into()],
        Expr::ApplyLoad => vec!["load".into()],
        Expr::Builtin { name } => vec![format!("@{name}")],
        Expr::StackOp(op) => vec![stack_op_word(*op).into()],
        Expr::Array(elems) => vec![format_container('[', ']', elems)],
        Expr::List(elems) => vec![format_container('(', ')', elems)],
        Expr::Map(pairs) => {
            let mut inner = String::new();
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    inner.push(' ');
                }
                inner.push_str(&expr_tokens(k).join(" "));
                inner.push(' ');
                inner.push_str(&expr_tokens(v).join(" "));
            }
            vec![format!("{{{inner}}}")]
        }
        Expr::StructLit(fields) => {
            let mut inner = String::new();
            for (i, (name, value)) in fields.iter().enumerate() {
                if i > 0 {
                    inner.push(' ');
                }
                inner.push_str(name);
                inner.push(' ');
                inner.push_str(&expr_tokens(value).join(" "));
            }
            vec![format!("{{{inner}}}")]
        }
        Expr::EmptyMapOrStruct => vec!["{}".into()],
        Expr::Seq(elems) => elems.iter().flat_map(|(e, _)| expr_tokens(e)).collect(),
    }
}

fn format_container(open: char, close: char, elems: &[Expr]) -> String {
    let mut inner = String::new();
    for (i, e) in elems.iter().enumerate() {
        if i > 0 {
            inner.push(' ');
        }
        inner.push_str(&expr_tokens(e).join(" "));
    }
    format!("{open}{inner}{close}")
}

pub(crate) fn binop_word(op: BinOp) -> &'static str {
    match op {
        BinOp::Plus => "+",
        BinOp::Minus => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Fdiv => "//",
        BinOp::Mod => "%",
        BinOp::Pow => "^",
        BinOp::Concat => "~",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Xor => "xor",
        BinOp::Lshift => "lshift",
        BinOp::Rshift => "rshift",
    }
}

pub(crate) fn unop_word(op: UnOp) -> &'static str {
    match op {
        UnOp::Not => "not",
    }
}

pub(crate) fn stack_op_word(op: StackOp) -> &'static str {
    match op {
        StackOp::Dup => "dup",
        StackOp::Swap => "swap",
        StackOp::Rot => "rot",
        StackOp::Unrot => "unrot",
        StackOp::Pop => "pop",
        StackOp::Drop => "drop",
    }
}
