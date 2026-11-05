use std::path::{Path, PathBuf};

use crate::diagnostics::{ColorChoice, Diagnostic, DiagnosticBatch, SourceFile, Span, render};
use crate::interpreter::{EvalContext, InterpretError};
use crate::parser::Parser;
use crate::parser::ast::{Program, StmtKind};
use crate::result::RunResult;
use crate::tokenizer::{Token, Tokenizer};

/// Options for one session.
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// Source path shown in diagnostics and used for module-relative imports.
    pub source_path: String,
    /// Extra module lookup roots (`"a.b"` => `a/b.yar`).
    pub module_search_paths: Vec<PathBuf>,
    /// Whether this session should require a top-level entry function.
    pub require_main: bool,
    /// Top-level entry function name (default [`crate::DEFAULT_ENTRY_NAME`]).
    pub entry_name: String,
    /// Maximum number of diagnostics to collect before aborting.
    pub error_limit: usize,
}

impl SessionOptions {
    pub fn new(source_path: impl Into<String>) -> Self {
        Self {
            source_path: source_path.into(),
            module_search_paths: Vec::new(),
            require_main: true,
            entry_name: crate::DEFAULT_ENTRY_NAME.to_string(),
            error_limit: crate::diagnostics::DEFAULT_ERROR_LIMIT,
        }
    }
}

/// Stateful frontend entry point: tokenize -> parse -> check -> interpret.
#[derive(Debug, Clone)]
pub struct Session {
    pub options: SessionOptions,
}

/// Program that passed parsing.
#[derive(Debug, Clone)]
pub struct ParsedProgram {
    pub file: SourceFile,
    pub program: Program,
}

impl ParsedProgram {
    pub fn new(file: SourceFile, program: Program) -> Self {
        Self { file, program }
    }
}

/// Diagnostics emitted while tokenizing/parsing one source file.
#[derive(Debug)]
pub struct SessionDiagnostics {
    pub file: SourceFile,
    pub batch: DiagnosticBatch,
}

/// How drivers should classify a failed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFailureKind {
    /// User / toolchain diagnostics.
    User,
    /// Internal error.
    Ice,
}

impl SessionDiagnostics {
    /// Whether this batch is an internal error.
    pub fn is_ice(&self) -> bool {
        self.batch.is_ice()
    }

    /// Classify for driver exit codes.
    pub fn failure_kind(&self) -> SessionFailureKind {
        if self.is_ice() {
            SessionFailureKind::Ice
        } else {
            SessionFailureKind::User
        }
    }

    /// Build a tagged ICE batch (does not panic).
    pub fn ice(
        path: impl Into<String>,
        source: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let path = path.into();
        let file = SourceFile::new(path.clone(), source.into());
        let mut batch = DiagnosticBatch::with_limit(1);
        batch.push(
            Diagnostic::ice(detail)
                .with_path(path)
                .with_primary(Span::default(), ""),
        );
        Self { file, batch }
    }
}

impl Session {
    pub fn new(options: SessionOptions) -> Self {
        Self { options }
    }

    /// Create a new session with default options for the given source path.
    pub fn with_source_path(source_path: impl Into<String>) -> Self {
        Self::new(SessionOptions::new(source_path))
    }

    /// Synthesize an ICE diagnostic without panicking.
    pub fn debug_trigger_ice(&self, detail: impl Into<String>) -> SessionDiagnostics {
        SessionDiagnostics::ice(&self.options.source_path, String::new(), detail)
    }

    /// Tokenize source text without parsing.
    pub fn tokenize_source(
        &self,
        source: String,
    ) -> Result<(SourceFile, Vec<Token>), SessionDiagnostics> {
        let path = self.options.source_path.clone();
        let file = SourceFile::new(path.clone(), source.clone());
        match Tokenizer::new(source).tokenize() {
            Ok(tokens) => Ok((file, tokens)),
            Err(e) => Err(SessionDiagnostics {
                file,
                batch: tokenize_batch(e, &path, self.options.error_limit),
            }),
        }
    }

    /// Parse source text.
    pub fn parse_source(&self, source: String) -> Result<ParsedProgram, SessionDiagnostics> {
        let (file, program, batch) = self.parse_source_recovering(source)?;
        if !batch.is_empty() {
            return Err(SessionDiagnostics { file, batch });
        }
        Ok(ParsedProgram::new(file, program))
    }

    /// Tokenize and parse with recovery: returns the recovered program plus any
    /// syntax diagnostics. Tokenize failure is still `Err`.
    pub fn parse_source_recovering(
        &self,
        source: String,
    ) -> Result<(SourceFile, Program, DiagnosticBatch), SessionDiagnostics> {
        let (file, tokens) = self.tokenize_source(source)?;
        let (program, batch) =
            Parser::with_error_limit(tokens, self.options.error_limit).parse_recovering();
        Ok((file, program, batch))
    }

    /// Parse and execute the configured entry on the AST interpreter.
    pub fn interpret_source(&self, source: String) -> Result<RunResult, SessionDiagnostics> {
        let parsed = self.parse_source(source)?;
        self.check_parsed_program(&parsed)?;
        self.require_main_if_needed(&parsed.file, &parsed.program)?;

        let mut ctx = EvalContext::new();
        // Audit-3 PLAN 6.2.1: a bare relative source path has an empty
        // parent directory; search the working directory instead, so
        // sibling modules resolve from inside the file's directory.
        let dir = Path::new(&self.options.source_path)
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ctx.add_module_search_path(dir);
        for p in &self.options.module_search_paths {
            ctx.add_module_search_path(p.clone());
        }

        if let Err(e) = ctx.load_program(&parsed.program) {
            return Err(SessionDiagnostics {
                file: parsed.file.clone(),
                batch: interpret_error_batch(e, self.options.error_limit),
            });
        }

        match ctx.run_entry(&self.options.entry_name) {
            Ok(result) => Ok(result),
            Err(e) => Err(SessionDiagnostics {
                file: parsed.file,
                batch: interpret_error_batch(e, self.options.error_limit),
            }),
        }
    }

    /// Run the static checking pass (PLAN Stage 3) on a parsed program.
    /// Errors abort the session before interpretation; warnings do not.
    fn check_parsed_program(&self, parsed: &ParsedProgram) -> Result<(), SessionDiagnostics> {
        let batch = crate::checker::check_program(
            &parsed.program,
            &self.options.source_path,
            self.options.error_limit,
        );
        if batch.has_errors() {
            return Err(SessionDiagnostics {
                file: parsed.file.clone(),
                batch,
            });
        }
        Ok(())
    }

    fn require_main_if_needed(
        &self,
        file: &SourceFile,
        program: &Program,
    ) -> Result<(), SessionDiagnostics> {
        let entry = self.options.entry_name.as_str();
        if !self.options.require_main || program.has_entry(entry) {
            return Ok(());
        }
        let path = self.options.source_path.clone();
        let mut batch = DiagnosticBatch::with_limit(self.options.error_limit);
        let span = program
            .entry_function(entry)
            .map(|(_, span)| span)
            .or_else(|| {
                program.items.iter().find_map(|item| match &item.kind {
                    StmtKind::Function(_) => Some(item.span),
                    _ => None,
                })
            })
            .or_else(|| program.items.first().map(|item| item.span))
            .unwrap_or_default();
        let diag = Diagnostic::error("E360", format!("program has no '{entry}' function"))
            .with_path(path)
            .with_primary(span, "")
            .with_note(format!(
                "running a `.yar` file requires a top-level `{entry}` entry point"
            ))
            .with_help(format!(
                "add `{entry} function do ... end`, optionally `with T` for a printable result"
            ));
        batch.push(diag);
        Err(SessionDiagnostics {
            file: file.clone(),
            batch,
        })
    }
}

fn tokenize_batch(
    err: crate::tokenizer::TokenizeError,
    path: &str,
    error_limit: usize,
) -> DiagnosticBatch {
    let mut batch = DiagnosticBatch::with_limit(error_limit);
    let diag = Diagnostic::error(err.code, err.message)
        .with_path(path)
        .with_primary(Span::from_location(err.location), "");
    batch.push(diag);
    batch
}

fn interpret_error_batch(err: InterpretError, error_limit: usize) -> DiagnosticBatch {
    let mut batch = DiagnosticBatch::with_limit(error_limit);
    batch.push(
        Diagnostic::error(err.code, err.message)
            .with_path(String::new())
            .with_primary(err.span, ""),
    );
    batch
}

/// Render all diagnostics from one batch for one source file.
pub fn render_batch(batch: &DiagnosticBatch, file: &SourceFile, color: ColorChoice) -> String {
    let mut out = String::new();
    for diag in batch.iter() {
        let mut diag = diag.clone();
        if diag.path.is_empty() {
            diag.path = file.path.clone();
        }
        out.push_str(&render(&diag, file, color));
    }
    if batch.is_at_limit() {
        out.push_str(&format!(
            "error: aborting due to {} previous errors (limit {})\n",
            batch.error_count(),
            batch.limit()
        ));
    }
    out
}
