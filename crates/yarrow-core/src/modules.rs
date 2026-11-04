//! Module loading for the interpreter.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::diagnostics::Span;
use crate::parser::ast::Program;

// Std modules embedded at build time by `build.rs`, as a built-in fallback
// search source: explicit `-L` paths take precedence over these.
include!(concat!(env!("OUT_DIR"), "/std_modules.rs"));

/// Look up a dotted module path in the embedded std table.
fn embedded_source(path: &str) -> Option<&'static str> {
    STD_MODULES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, source)| *source)
}

/// Error from module loading.
#[derive(Debug, Clone)]
pub struct ModuleError {
    pub message: String,
    pub code: String,
    pub span: Span,
}

impl ModuleError {
    pub fn new(message: impl Into<String>, span: Span, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: code.into(),
            span,
        }
    }
}

/// A required module (loaded from a `require` statement).
#[derive(Debug, Clone)]
pub struct RequiredModule {
    /// Module path (dotted, e.g. "std.io").
    pub path: String,
    /// Optional alias for the require statement.
    pub alias: Option<String>,
    /// Optional single item import.
    pub item: Option<String>,
    /// The parsed program.
    pub program: Program,
}

/// Module loader for resolving `require` paths.
#[derive(Debug, Clone, Default)]
pub struct ModuleLoader {
    search_paths: Vec<PathBuf>,
    /// Cache of resolved module paths to avoid re-reading.
    cache: HashMap<String, String>,
    /// Cache of loaded sources.
    source_cache: HashMap<String, String>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_search_path(&mut self, path: impl Into<PathBuf>) {
        self.search_paths.push(path.into());
    }

    /// Load a module by its dotted path.
    pub fn load_at(&mut self, path: &str, span: Span) -> Result<String, ModuleError> {
        if let Some(source) = self.source_cache.get(path) {
            return Ok(source.clone());
        }

        // Try to resolve the module path
        let fs_path = self.resolve_path(path);
        let source = match fs_path {
            Ok(fs_path) => std::fs::read_to_string(&fs_path).map_err(|e| {
                ModuleError::new(
                    format!("failed to read module '{}': {}", path, e),
                    span,
                    "E380",
                )
            })?,
            // Fall back to the embedded std modules when no search path
            // matches, so `std.*` resolves without `-L`.
            Err(_) => embedded_source(path)
                .ok_or_else(|| {
                    ModuleError::new(format!("module '{}' not found", path), span, "E380")
                })?
                .to_string(),
        };
        self.source_cache.insert(path.to_string(), source.clone());
        Ok(source)
    }

    /// Try to load a module without failing.
    pub fn try_load(&mut self, path: &str) -> Option<String> {
        if let Some(source) = self.source_cache.get(path) {
            return Some(source.clone());
        }
        for search_path in &self.search_paths {
            let fs_path = path.replace('.', "/") + ".yar";
            let candidate = search_path.join(&fs_path);
            if candidate.exists()
                && let Ok(source) = std::fs::read_to_string(&candidate)
            {
                return Some(source);
            }
        }
        embedded_source(path).map(str::to_string)
    }

    fn resolve_path(&mut self, path: &str) -> Result<PathBuf, ModuleError> {
        // If already in cache
        if let Some(resolved) = self.cache.get(path) {
            return Ok(PathBuf::from(resolved));
        }

        // Convert dotted path to filesystem path
        let fs_path = path.replace('.', "/") + ".yar";

        // Try each search path
        for search_path in &self.search_paths {
            let candidate = search_path.join(&fs_path);
            if candidate.exists() {
                self.cache
                    .insert(path.to_string(), candidate.to_string_lossy().into_owned());
                return Ok(candidate);
            }
        }

        Err(ModuleError::new(
            format!("module '{}' not found", path),
            Span::default(),
            "E380",
        ))
    }
}
