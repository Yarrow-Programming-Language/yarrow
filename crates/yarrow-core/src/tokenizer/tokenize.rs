use crate::tokenizer::token::Location;
use crate::tokenizer::token::Token;
use crate::tokenizer::token::TokenizeError;
use crate::tokenizer::token_kind::TokenKind;

use std::collections::HashMap;

pub struct Tokenizer {
    source: String,
    start: usize,
    current: usize,
    line: usize,
    line_start: usize,
    tokens: Vec<Token>,
    keywords: HashMap<String, TokenKind>,
}

impl Tokenizer {
    pub fn new(source: String) -> Self {
        let mut keywords = HashMap::new();
        keywords.insert("and".to_string(), TokenKind::And);
        keywords.insert("or".to_string(), TokenKind::Or);
        keywords.insert("xor".to_string(), TokenKind::Xor);
        keywords.insert("not".to_string(), TokenKind::Not);
        keywords.insert("lshift".to_string(), TokenKind::LeftShift);
        keywords.insert("rshift".to_string(), TokenKind::RightShift);
        keywords.insert("typeof".to_string(), TokenKind::Typeof);
        keywords.insert("if".to_string(), TokenKind::If);
        keywords.insert("else".to_string(), TokenKind::Else);
        keywords.insert("for".to_string(), TokenKind::For);
        keywords.insert("match".to_string(), TokenKind::Match);
        keywords.insert("case".to_string(), TokenKind::Case);
        keywords.insert("unwrap".to_string(), TokenKind::Unwrap);
        keywords.insert("handle".to_string(), TokenKind::Handle);
        keywords.insert("function".to_string(), TokenKind::Function);
        keywords.insert("return".to_string(), TokenKind::Return);
        keywords.insert("call".to_string(), TokenKind::Call);
        keywords.insert("do".to_string(), TokenKind::Do);
        keywords.insert("with".to_string(), TokenKind::With);
        keywords.insert("end".to_string(), TokenKind::End);
        keywords.insert("const".to_string(), TokenKind::Const);
        keywords.insert("static".to_string(), TokenKind::Static);
        keywords.insert("mutable".to_string(), TokenKind::Mutable);
        keywords.insert("set".to_string(), TokenKind::Set);
        keywords.insert("public".to_string(), TokenKind::Public);
        keywords.insert("private".to_string(), TokenKind::Private);
        keywords.insert("copy".to_string(), TokenKind::Copy);
        keywords.insert("error".to_string(), TokenKind::Error);
        keywords.insert("struct".to_string(), TokenKind::Struct);
        keywords.insert("implement".to_string(), TokenKind::Implement);
        keywords.insert("enum".to_string(), TokenKind::Enum);
        keywords.insert("union".to_string(), TokenKind::Union);
        keywords.insert("pop".to_string(), TokenKind::Pop);
        keywords.insert("drop".to_string(), TokenKind::Drop);
        keywords.insert("dup".to_string(), TokenKind::Dup);
        keywords.insert("rot".to_string(), TokenKind::Rot);
        keywords.insert("unrot".to_string(), TokenKind::Unrot);
        keywords.insert("swap".to_string(), TokenKind::Swap);
        keywords.insert("require".to_string(), TokenKind::Require);
        keywords.insert("defer".to_string(), TokenKind::Defer);
        keywords.insert("borrow".to_string(), TokenKind::Borrow);
        keywords.insert("move".to_string(), TokenKind::Move);
        keywords.insert("load".to_string(), TokenKind::Load);
        keywords.insert("store".to_string(), TokenKind::Store);
        keywords.insert("unsafe".to_string(), TokenKind::Unsafe);
        keywords.insert("fallback".to_string(), TokenKind::Fallback);
        keywords.insert("true".to_string(), TokenKind::True);
        keywords.insert("false".to_string(), TokenKind::False);

        Self {
            source,
            start: 0,
            current: 0,
            line: 1,
            line_start: 0,
            tokens: Vec::new(),
            keywords,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, TokenizeError> {
        while !self.is_at_end() {
            self.start = self.current;
            self.scan_token()?;
        }

        let location = self.get_location_at(self.current);
        self.tokens.push(Token::eof(location));

        Ok(std::mem::take(&mut self.tokens))
    }

    fn scan_token(&mut self) -> Result<(), TokenizeError> {
        let c = self.advance();

        match c {
            ' ' | '\t' | '\r' => {}
            '\n' => {
                self.line += 1;
                self.line_start = self.current;
            }
            '#' => self.scan_comment(),
            '(' => self.add_token(TokenKind::LeftParen),
            ')' => self.add_token(TokenKind::RightParen),
            '{' => self.add_token(TokenKind::LeftCurly),
            '}' => self.add_token(TokenKind::RightCurly),
            '[' => self.add_token(TokenKind::LeftSquare),
            ']' => self.add_token(TokenKind::RightSquare),
            '%' => self.add_token(TokenKind::Percent),
            '*' => self.add_token(TokenKind::Asterisk),
            '^' => self.add_token(TokenKind::Caret),
            '/' => {
                if self.match_char('/') {
                    self.add_token(TokenKind::SlashSlash);
                } else {
                    self.add_token(TokenKind::Slash);
                }
            }
            '.' => self.add_token(TokenKind::Dot),
            '=' => {
                if self.match_char('=') {
                    self.add_token(TokenKind::EqualEqual);
                } else {
                    return Err(TokenizeError::new(
                        "unexpected character '='".to_string(),
                        self.get_location(),
                        "E100".to_string(),
                    ));
                }
            }
            '<' => {
                if self.match_char('=') {
                    self.add_token(TokenKind::LessEqual);
                } else {
                    self.add_token(TokenKind::Less);
                }
            }
            '>' => {
                if self.match_char('=') {
                    self.add_token(TokenKind::GreaterEqual);
                } else {
                    self.add_token(TokenKind::Greater);
                }
            }
            '!' => {
                if self.match_char('=') {
                    self.add_token(TokenKind::NotEqual);
                } else {
                    return Err(TokenizeError::new(
                        "unexpected character '!'".to_string(),
                        self.get_location(),
                        "E100".to_string(),
                    ));
                }
            }
            '-' => {
                if self.peek().is_ascii_digit() {
                    self.handle_number()?;
                } else {
                    self.add_token(TokenKind::Minus);
                }
            }
            '+' => self.add_token(TokenKind::Plus),
            '~' => self.add_token(TokenKind::Tilde),
            '|' => self.add_token(TokenKind::Pipe),
            c if c.is_ascii_digit() => self.handle_number()?,
            '"' => self.handle_string()?,
            '\'' => self.handle_rune()?,
            '@' => {
                // `@name` is a builtin word: scan the identifier inline so
                // keywords (like `load`/`store`) are not substituted. The
                // lexeme keeps the '@'; the parser strips it.
                while self.peek().is_ascii_alphanumeric() || self.peek() == '_' {
                    self.advance();
                }
                self.add_token(TokenKind::At);
            }
            c if c.is_ascii_alphabetic() => self.handle_identifier(),
            // EBNF: identifier = letter, { letter | digit | "_" }; '_' only
            // appears inside a name (and as a digit separator in numbers),
            // so it cannot start one.
            '_' => {
                return Err(TokenizeError::new(
                    "identifiers must start with a letter; '_' is only allowed inside a name"
                        .to_string(),
                    self.get_location_at(self.start),
                    "E134".to_string(),
                ));
            }
            _ => {
                return Err(TokenizeError::new(
                    format!("unexpected character '{c}'"),
                    self.get_location(),
                    "E100".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn handle_number(&mut self) -> Result<(), TokenizeError> {
        if self.source.as_bytes()[self.start] == b'-' {
            self.advance();
        }

        let first_digit_is_zero = self.source.as_bytes()[self.current - 1] == b'0';

        let mut radix = 10;
        if first_digit_is_zero {
            match self.peek() {
                'b' | 'B' => {
                    self.advance();
                    radix = 2;
                }
                'x' | 'X' => {
                    self.advance();
                    radix = 16;
                }
                _ => {}
            }
        }

        if radix != 10 {
            if !self.is_digit(self.peek(), radix) {
                return Err(TokenizeError::new(
                    format!(
                        "invalid digit in '0{}' literal",
                        if radix == 2 { 'b' } else { 'x' }
                    ),
                    self.get_location(),
                    "E110".to_string(),
                ));
            }
            while self.is_digit(self.peek(), radix) || self.peek() == '_' {
                self.advance();
            }
        } else {
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                self.advance();
            }

            if self.peek() == '.' && self.peek_next().is_ascii_digit() {
                self.advance();
                while self.peek().is_ascii_digit() || self.peek() == '_' {
                    self.advance();
                }
            }
        }

        let is_float = radix == 10 && self.source[self.start..self.current].contains('.');
        self.add_token(if is_float {
            TokenKind::Float
        } else {
            TokenKind::Integer
        });
        Ok(())
    }

    fn handle_string(&mut self) -> Result<(), TokenizeError> {
        while !self.is_at_end() && self.peek() != '"' {
            if self.peek() == '\n' {
                return Err(TokenizeError::new(
                    "unterminated string literal: newlines are not allowed inside strings"
                        .to_string(),
                    self.get_location(),
                    "E120".to_string(),
                ));
            }

            if self.peek() == '\\' {
                self.advance();
                if self.is_at_end() {
                    return Err(TokenizeError::new(
                        "invalid escape sequence".to_string(),
                        self.get_location(),
                        "E121".to_string(),
                    ));
                }

                let escape = self.peek();
                if !['\\', '"', '\'', 'n', 'r', 't', 'v', 'b', 'a', 'f'].contains(&escape) {
                    return Err(TokenizeError::new(
                        format!("invalid escape sequence '\\{escape}'"),
                        self.get_location(),
                        "E121".to_string(),
                    ));
                }
                self.advance();
            } else {
                self.advance();
            }
        }

        if self.is_at_end() {
            return Err(TokenizeError::new(
                "unterminated string literal".to_string(),
                self.get_location(),
                "E122".to_string(),
            ));
        }

        self.advance();
        self.add_token(TokenKind::String);
        Ok(())
    }

    fn handle_rune(&mut self) -> Result<(), TokenizeError> {
        while !self.is_at_end() && self.peek() != '\'' {
            if self.peek() == '\n' {
                return Err(TokenizeError::new(
                    "unterminated rune literal: newlines are not allowed inside runes".to_string(),
                    self.get_location(),
                    "E130".to_string(),
                ));
            }

            if self.peek() == '\\' {
                self.advance();
                if self.is_at_end() {
                    return Err(TokenizeError::new(
                        "invalid escape sequence".to_string(),
                        self.get_location(),
                        "E131".to_string(),
                    ));
                }

                let escape = self.peek();
                if !['\\', '\'', '"', 'n', 'r', 't', 'v', 'b', 'a', 'f'].contains(&escape) {
                    return Err(TokenizeError::new(
                        format!("invalid escape sequence '\\{escape}'"),
                        self.get_location(),
                        "E131".to_string(),
                    ));
                }
                self.advance();
            } else {
                self.advance();
            }
        }

        if self.is_at_end() {
            return Err(TokenizeError::new(
                "unterminated rune literal".to_string(),
                self.get_location(),
                "E132".to_string(),
            ));
        }

        self.advance();

        let content = &self.source[self.start + 1..self.current - 1];
        if content.replace('\\', "").chars().count() > 1 {
            return Err(TokenizeError::new(
                "rune literals must contain exactly one character or escape sequence".to_string(),
                self.get_location(),
                "E133".to_string(),
            ));
        }

        self.add_token(TokenKind::Rune);
        Ok(())
    }

    fn handle_identifier(&mut self) {
        while self.peek().is_ascii_alphanumeric() || self.peek() == '_' {
            self.advance();
        }

        let text = self.lexeme();
        let kind = self
            .keywords
            .get(text.as_str())
            .copied()
            .unwrap_or(TokenKind::Identifier);
        self.add_token(kind);
    }

    /// Emit a `Comment` token for `#` … end of line (newline left for the
    /// next scan; whitespace / newlines are not emitted as trivia).
    fn scan_comment(&mut self) {
        while !self.is_at_end() && self.peek() != '\n' {
            self.advance();
        }
        self.add_token(TokenKind::Comment);
    }

    fn add_token(&mut self, kind: TokenKind) {
        let lexeme = self.lexeme();
        let location = self.get_location_at(self.start);
        self.tokens
            .push(Token::new(kind, lexeme, location, self.current));
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.peek() != expected {
            return false;
        }
        self.advance();
        true
    }

    fn advance(&mut self) -> char {
        let c = self.peek();
        if !self.is_at_end() {
            self.current += c.len_utf8();
        }
        c
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.source.len()
    }

    fn peek(&self) -> char {
        self.source[self.current..].chars().next().unwrap_or('\0')
    }

    fn peek_next(&self) -> char {
        let mut chars = self.source[self.current..].chars();
        chars.next();
        chars.next().unwrap_or('\0')
    }

    fn is_digit(&self, c: char, radix: u32) -> bool {
        c.is_digit(radix)
    }

    fn lexeme(&self) -> String {
        self.source[self.start..self.current].to_string()
    }

    fn get_location_at(&self, offset: usize) -> Location {
        Location::new(
            self.line,
            offset.saturating_sub(self.line_start) + 1,
            offset,
        )
    }

    fn get_location(&self) -> Location {
        self.get_location_at(self.current)
    }
}
