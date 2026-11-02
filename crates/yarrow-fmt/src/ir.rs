//! Format IR: parsed AST plus comment trivia attached by source span.
//!
//! Stage 2 preference: AST + trivia map (not a full CST). The printer
//! (later stages) walks [`FormatIr::program`] and consults
//! [`FormatIr::trivia`] for comment placement.

use std::collections::HashMap;

use yarrow_core::{
    DiagnosticBatch, Parser, Program, Session, SessionDiagnostics, SessionOptions, SourceFile,
    Span, Token, TokenKind,
};

/// A line comment from the tokenizer (`#` through EOL, exclusive of `\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub lexeme: String,
    pub span: Span,
}

/// How a comment relates to its anchor token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentAttach {
    /// Own-line / preceding comment; printer places it above the anchor.
    Leading,
    /// Same-line comment after the anchor token.
    Trailing,
}

/// One comment tied to a non-comment token span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedComment {
    pub comment: Comment,
    pub attach: CommentAttach,
}

/// Comments keyed by the span of the non-comment token they attach to.
///
/// Attachment rules (Stage 2; Stage 9 uses these for trailing reattach):
/// - Same line as the previous code token → trailing on that token.
/// - Otherwise → leading on the next code token.
/// - No following code token → [`TriviaMap::file_trailing`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriviaMap {
    by_anchor: HashMap<Span, Vec<AttachedComment>>,
    /// Comments after the last non-comment token (EOF-only files, etc.).
    file_trailing: Vec<Comment>,
}

impl TriviaMap {
    pub fn from_tokens(tokens: &[Token]) -> Self {
        let code_indices: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind != TokenKind::Comment && t.kind != TokenKind::Eof)
            .map(|(i, _)| i)
            .collect();

        let mut map = Self::default();
        let mut next_code = 0usize;

        for (i, tok) in tokens.iter().enumerate() {
            if tok.kind == TokenKind::Eof {
                break;
            }
            if tok.kind != TokenKind::Comment {
                if next_code < code_indices.len() && code_indices[next_code] == i {
                    next_code += 1;
                }
                continue;
            }

            let comment = Comment {
                lexeme: tok.lexeme.clone(),
                span: tok.span(),
            };

            let prev = next_code.checked_sub(1).map(|ci| &tokens[code_indices[ci]]);
            let next = code_indices.get(next_code).map(|&ci| &tokens[ci]);

            if let Some(prev) = prev
                && prev.location.line == tok.location.line
            {
                map.push_attached(
                    prev.span(),
                    AttachedComment {
                        comment,
                        attach: CommentAttach::Trailing,
                    },
                );
                continue;
            }

            if let Some(next) = next {
                map.push_attached(
                    next.span(),
                    AttachedComment {
                        comment,
                        attach: CommentAttach::Leading,
                    },
                );
            } else {
                map.file_trailing.push(comment);
            }
        }

        map
    }

    fn push_attached(&mut self, anchor: Span, attached: AttachedComment) {
        self.by_anchor.entry(anchor).or_default().push(attached);
    }

    /// Comments attached to `anchor` (leading and trailing), in source order.
    pub fn attached_to(&self, anchor: Span) -> &[AttachedComment] {
        self.by_anchor
            .get(&anchor)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// All attached comments (every anchor), for line-based trailing lookup.
    pub fn all_attached(&self) -> impl Iterator<Item = &AttachedComment> {
        self.by_anchor.values().flat_map(|v| v.iter())
    }

    /// Leading comments for `anchor`.
    pub fn leading(&self, anchor: Span) -> impl Iterator<Item = &Comment> {
        self.attached_to(anchor)
            .iter()
            .filter(|a| a.attach == CommentAttach::Leading)
            .map(|a| &a.comment)
    }

    /// Trailing comments for `anchor`.
    pub fn trailing(&self, anchor: Span) -> impl Iterator<Item = &Comment> {
        self.attached_to(anchor)
            .iter()
            .filter(|a| a.attach == CommentAttach::Trailing)
            .map(|a| &a.comment)
    }

    pub fn file_trailing(&self) -> &[Comment] {
        &self.file_trailing
    }

    pub fn is_empty(&self) -> bool {
        self.by_anchor.is_empty() && self.file_trailing.is_empty()
    }

    pub fn comment_count(&self) -> usize {
        self.by_anchor.values().map(|v| v.len()).sum::<usize>() + self.file_trailing.len()
    }
}

/// Parsed program plus comment trivia for formatting.
#[derive(Debug, Clone)]
pub struct FormatIr {
    pub file: SourceFile,
    pub program: Program,
    pub trivia: TriviaMap,
}

impl FormatIr {
    /// Tokenize, attach comment trivia, then parse into a format IR.
    ///
    /// Does not type-check or run the program. On tokenize/parse failure,
    /// returns the same diagnostic batch shape as [`Session::parse_source`].
    pub fn parse(
        source: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, SessionDiagnostics> {
        match Self::parse_recovering(source, path)? {
            FormatIrParse::Complete(ir) => Ok(ir),
            FormatIrParse::Partial { file, batch, .. } => Err(SessionDiagnostics { file, batch }),
        }
    }

    /// Like [`Self::parse`], but keeps a recovered AST when syntax errors were
    /// collected ([`Parser::parse_recovering`]). Tokenize failure is still `Err`.
    pub fn parse_recovering(
        source: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<FormatIrParse, SessionDiagnostics> {
        let opts = SessionOptions::new(path);
        let session = Session::new(opts);
        let (file, tokens) = session.tokenize_source(source.into())?;
        let trivia = TriviaMap::from_tokens(&tokens);
        let (program, batch) =
            Parser::with_error_limit(tokens, session.options.error_limit).parse_recovering();
        let ir = Self {
            file: file.clone(),
            program,
            trivia,
        };
        if batch.is_empty() {
            Ok(FormatIrParse::Complete(ir))
        } else {
            Ok(FormatIrParse::Partial { ir, file, batch })
        }
    }
}

/// Outcome of [`FormatIr::parse_recovering`].
#[derive(Debug, Clone)]
pub enum FormatIrParse {
    /// Clean parse; safe for full construct reprint.
    Complete(FormatIr),
    /// Recovered AST plus diagnostics. Stage 18 may selectively reprint
    /// top-level declarations whose spans clear error regions and re-format
    /// cleanly alone; other regions stay hygiene-only.
    Partial {
        ir: FormatIr,
        file: SourceFile,
        batch: DiagnosticBatch,
    },
}
