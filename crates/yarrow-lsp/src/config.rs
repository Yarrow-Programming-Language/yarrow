//! Process defaults and LSP `initializationOptions` for analysis.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yarrow_core::{DEFAULT_ENTRY_NAME, Session, SessionOptions};

/// Runtime configuration shared by diagnostics, navigation, and formatting.
#[derive(Debug, Clone)]
pub struct LspConfig {
    /// Extra module lookup roots (`-L` / init `searchPaths` / workspace folders).
    pub search_paths: Vec<PathBuf>,
    /// Explicit multi-root project entry files (`initializationOptions.projectRoots`).
    ///
    /// Empty means single-file `check_source` (default). Non-empty enables
    /// `Session::check_project` for those roots only (no folder crawl).
    pub project_roots: Vec<PathBuf>,
    /// Top-level entry name (default `main`).
    pub entry_name: String,
    /// When false, do not advertise or serve document / range / on-type formatting.
    pub format_enable: bool,
    /// When false, do not advertise or serve `textDocument/inlayHint`.
    pub inlay_hints_enable: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            search_paths: Vec::new(),
            project_roots: Vec::new(),
            entry_name: DEFAULT_ENTRY_NAME.to_string(),
            format_enable: true,
            inlay_hints_enable: true,
        }
    }
}

impl LspConfig {
    /// Whether multi-root project analysis is active.
    pub fn project_mode(&self) -> bool {
        !self.project_roots.is_empty()
    }

    /// Build [`SessionOptions`] for a document path.
    pub fn session_options(&self, source_path: impl Into<String>) -> SessionOptions {
        let mut opts = SessionOptions::new(source_path);
        opts.module_search_paths = self.search_paths.clone();
        opts.entry_name = self.entry_name.clone();
        opts
    }

    /// Session configured with this crate's search paths and entry name.
    pub fn session(&self, source_path: impl Into<String>) -> Session {
        Session::new(self.session_options(source_path))
    }

    /// Merge client init options and workspace folder roots into this config.
    pub fn apply_initialize(
        &mut self,
        init: Option<&InitializationOptions>,
        workspace_folders: &[PathBuf],
    ) {
        if let Some(init) = init {
            if let Some(paths) = &init.search_paths {
                for p in paths {
                    self.push_search_path(PathBuf::from(p));
                }
            }
            if let Some(roots) = &init.project_roots {
                self.project_roots.clear();
                for p in roots {
                    self.push_project_root(PathBuf::from(p));
                }
            }
            if let Some(name) = &init.entry_name
                && !name.is_empty()
            {
                self.entry_name = name.clone();
            }
            if let Some(fmt) = init.format {
                self.format_enable = fmt;
            }
            if let Some(inlays) = init.inlay_hints {
                self.inlay_hints_enable = inlays;
            }
        }
        for folder in workspace_folders {
            self.push_search_path(folder.clone());
        }
    }

    fn push_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.iter().any(|e| e == &path) {
            self.search_paths.push(path);
        }
    }

    fn push_project_root(&mut self, path: PathBuf) {
        let resolved = resolve_config_path(&path);
        if !self.project_roots.iter().any(|e| e == &resolved) {
            self.project_roots.push(resolved);
        }
    }
}

/// Resolve a config path: expand relative paths against the process cwd.
fn resolve_config_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// JSON shape under `InitializeParams.initialization_options` (camelCase).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializationOptions {
    /// Extra module search directories (`-L` equivalent).
    #[serde(default)]
    pub search_paths: Option<Vec<String>>,
    /// Explicit project root `.yar` paths (multi-root `check_project`).
    ///
    /// Omitted or empty keeps single-file analysis. Paths may be absolute or
    /// cwd-relative; no workspace folder crawl.
    #[serde(default)]
    pub project_roots: Option<Vec<String>>,
    /// Entry function name (default `main`).
    #[serde(default)]
    pub entry_name: Option<String>,
    /// Enable document, range, and on-type formatting (default true).
    #[serde(default)]
    pub format: Option<bool>,
    /// Enable inlay hints (default true).
    #[serde(default)]
    pub inlay_hints: Option<bool>,
}

impl InitializationOptions {
    /// Parse from the opaque `initializationOptions` value, or `None` if absent / invalid.
    pub fn from_value(value: Option<&serde_json::Value>) -> Option<Self> {
        let value = value?;
        serde_json::from_value(value.clone()).ok()
    }
}
