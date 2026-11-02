//! `textDocument/semanticTokens/full` from core tokens + same-file decls.

use tower_lsp_server::ls_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};
use yarrow_core::{Span, Token, TokenKind, Tokenizer};

use crate::config::LspConfig;
use crate::definition::{Decl, DeclKind, collect_decls, resolve};
use crate::position::{PositionEncoding, PositionMap};

/// Legend advertised in `initialize` (index = `token_type` / modifier bit).
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::TYPE,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::OPERATOR,
        ],
        token_modifiers: vec![SemanticTokenModifier::DECLARATION],
    }
}

// Indices must match `legend()` token_types order (parameter reserved at 4).
const TY_KEYWORD: u32 = 0;
const TY_FUNCTION: u32 = 1;
const TY_VARIABLE: u32 = 2;
const TY_TYPE: u32 = 3;
const TY_PROPERTY: u32 = 5;
const TY_STRING: u32 = 6;
const TY_NUMBER: u32 = 7;
const TY_COMMENT: u32 = 8;
const TY_OPERATOR: u32 = 9;

const MOD_DECLARATION: u32 = 1 << 0;

/// Full-document semantic tokens, or empty when tokenize fails.
pub fn semantic_tokens_full(
    path: &str,
    text: &str,
    encoding: PositionEncoding,
    config: &LspConfig,
) -> SemanticTokens {
    let mut tokenizer = Tokenizer::new(text.to_string());
    let Ok(tokens) = tokenizer.tokenize() else {
        return SemanticTokens {
            result_id: None,
            data: Vec::new(),
        };
    };

    let session = config.session(path);
    let decls = session
        .parse_source(text.to_string())
        .map(|parsed| collect_decls(&parsed.file, &parsed.program))
        .unwrap_or_default();

    let map = PositionMap::new(text, encoding);
    let mut encoded = Vec::new();
    let mut cursor = EncodeCursor::default();

    for tok in &tokens {
        if tok.kind == TokenKind::Eof {
            continue;
        }
        let Some((token_type, modifiers)) = classify(tok, &decls) else {
            continue;
        };
        cursor.push(
            &map,
            text,
            Span::new(tok.location.offset, tok.end_offset),
            token_type,
            modifiers,
            &mut encoded,
        );
    }

    SemanticTokens {
        result_id: None,
        data: encoded,
    }
}

fn classify(tok: &Token, decls: &[Decl]) -> Option<(u32, u32)> {
    match tok.kind {
        TokenKind::Comment => Some((TY_COMMENT, 0)),
        TokenKind::String | TokenKind::Rune => Some((TY_STRING, 0)),
        TokenKind::Integer | TokenKind::Float => Some((TY_NUMBER, 0)),
        TokenKind::Identifier => classify_ident(tok, decls),
        k if is_keyword(k) => Some((TY_KEYWORD, 0)),
        k if is_operator(k) => Some((TY_OPERATOR, 0)),
        // Brackets and similar: leave to client grammar.
        _ => None,
    }
}

fn classify_ident(tok: &Token, decls: &[Decl]) -> Option<(u32, u32)> {
    let offset = tok.location.offset;
    let decl = resolve(decls, &tok.lexeme, offset)?;
    let token_type = match decl.kind {
        DeclKind::Function => TY_FUNCTION,
        DeclKind::Variable | DeclKind::Module => TY_VARIABLE,
        DeclKind::Type => TY_TYPE,
        DeclKind::Property => TY_PROPERTY,
    };
    let mut modifiers = 0;
    if decl.name_span.lo == tok.location.offset && decl.name_span.hi == tok.end_offset {
        modifiers |= MOD_DECLARATION;
    }
    Some((token_type, modifiers))
}

fn is_keyword(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::And
            | TokenKind::Or
            | TokenKind::Xor
            | TokenKind::Not
            | TokenKind::LeftShift
            | TokenKind::RightShift
            | TokenKind::Typeof
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::For
            | TokenKind::Match
            | TokenKind::Case
            | TokenKind::Unwrap
            | TokenKind::Handle
            | TokenKind::Function
            | TokenKind::Return
            | TokenKind::Call
            | TokenKind::Do
            | TokenKind::With
            | TokenKind::End
            | TokenKind::Const
            | TokenKind::Static
            | TokenKind::Mutable
            | TokenKind::Set
            | TokenKind::Unsafe
            | TokenKind::Public
            | TokenKind::Private
            | TokenKind::Copy
            | TokenKind::Error
            | TokenKind::Struct
            | TokenKind::Implement
            | TokenKind::Enum
            | TokenKind::Union
            | TokenKind::Require
            | TokenKind::Defer
            | TokenKind::Pop
            | TokenKind::Drop
            | TokenKind::Dup
            | TokenKind::Rot
            | TokenKind::Unrot
            | TokenKind::Swap
            | TokenKind::Borrow
            | TokenKind::Move
            | TokenKind::Load
            | TokenKind::Store
            | TokenKind::Fallback
            | TokenKind::True
            | TokenKind::False
    )
}

fn is_operator(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Asterisk
            | TokenKind::Slash
            | TokenKind::SlashSlash
            | TokenKind::Percent
            | TokenKind::Caret
            | TokenKind::Tilde
            | TokenKind::Pipe
            | TokenKind::Dot
            | TokenKind::EqualEqual
            | TokenKind::NotEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::At
    )
}

#[derive(Default)]
struct EncodeCursor {
    prev_line: u32,
    prev_start: u32,
}

impl EncodeCursor {
    /// Emit one or more single-line tokens for `span` (LSP forbids multi-line lengths).
    fn push(
        &mut self,
        map: &PositionMap<'_>,
        source: &str,
        span: Span,
        token_type: u32,
        modifiers: u32,
        out: &mut Vec<SemanticToken>,
    ) {
        if span.lo >= span.hi || span.hi > source.len() {
            return;
        }
        let mut lo = span.lo;
        while lo < span.hi {
            let line_end = match source[lo..].find('\n') {
                Some(rel) => lo + rel,
                None => source.len(),
            };
            let hi = span.hi.min(line_end);
            if hi > lo {
                let start = map.position(lo);
                let end = map.position(hi);
                // Same line by construction; length in negotiated character units.
                let length = end.character.saturating_sub(start.character);
                if length > 0 {
                    let delta_line = start.line.saturating_sub(self.prev_line);
                    let delta_start = if delta_line == 0 {
                        start.character.saturating_sub(self.prev_start)
                    } else {
                        start.character
                    };
                    out.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length,
                        token_type,
                        token_modifiers_bitset: modifiers,
                    });
                    self.prev_line = start.line;
                    self.prev_start = start.character;
                }
            }
            if hi >= span.hi {
                break;
            }
            // Skip the newline between segments.
            lo = hi + 1;
        }
    }
}
