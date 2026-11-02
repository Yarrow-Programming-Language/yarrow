//! `yarrow lsp` - thin in-process wrapper around `yarrow_lsp` (stdio or TCP).
//!
//! Global `-q` suppresses the startup banner. Bound TCP listen address still
//! prints (harnesses need it). `--color` is a no-op: the LSP crate has no
//! color knob.

use std::path::PathBuf;
use std::process::ExitCode;

use yarrow_lsp::LspConfig;

use crate::args::GlobalArgs;

/// Start the language server on stdio or TCP (`--listen`).
pub fn run_lsp(
    global: &GlobalArgs,
    search_paths: &[PathBuf],
    entry_name: &str,
    format_enable: bool,
    inlay_hints_enable: bool,
    log_level: LspLogLevel,
    listen: Option<&str>,
) -> ExitCode {
    let mut search = global.search_paths.clone();
    for p in search_paths {
        if !search.iter().any(|e| e == p) {
            search.push(p.clone());
        }
    }
    let config = LspConfig {
        search_paths: search,
        project_roots: Vec::new(),
        entry_name: entry_name.to_string(),
        format_enable,
        inlay_hints_enable,
    };

    if let Some(addr) = listen {
        if log_level.allows(LspLogLevel::Info) && !global.quiet {
            eprintln!(
                "yarrow lsp: starting (tcp listen={addr}; entry={}; format={}; inlay={}; search_paths={})",
                config.entry_name,
                config.format_enable,
                config.inlay_hints_enable,
                config.search_paths.len()
            );
        }

        // Bound address always printed (even with --quiet) so harnesses can connect.
        let result = yarrow_lsp::run_tcp_blocking(addr, config, |local| {
            eprintln!("yarrow-lsp: listening on {local}");
        });
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                if log_level.allows(LspLogLevel::Error) {
                    eprintln!("yarrow lsp: {err}");
                }
                ExitCode::FAILURE
            }
        };
    }

    if log_level.allows(LspLogLevel::Info) && !global.quiet {
        eprintln!(
            "yarrow lsp: starting (stdio; entry={}; format={}; inlay={}; search_paths={})",
            config.entry_name,
            config.format_enable,
            config.inlay_hints_enable,
            config.search_paths.len()
        );
    }

    match yarrow_lsp::run_stdio_blocking(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if log_level.allows(LspLogLevel::Error) {
                eprintln!("yarrow lsp: {err}");
            }
            ExitCode::FAILURE
        }
    }
}

/// stderr log filter for `yarrow lsp --log-level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LspLogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
}

impl std::str::FromStr for LspLogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "off" => Ok(LspLogLevel::Off),
            "error" => Ok(LspLogLevel::Error),
            "warn" => Ok(LspLogLevel::Warn),
            "info" => Ok(LspLogLevel::Info),
            "debug" => Ok(LspLogLevel::Debug),
            _ => Err(format!("unknown log level: {}", s)),
        }
    }
}

impl LspLogLevel {
    pub fn allows(self, level: LspLogLevel) -> bool {
        match self {
            LspLogLevel::Off => false,
            LspLogLevel::Error => matches!(level, LspLogLevel::Error),
            LspLogLevel::Warn => matches!(level, LspLogLevel::Error | LspLogLevel::Warn),
            LspLogLevel::Info => {
                matches!(
                    level,
                    LspLogLevel::Error | LspLogLevel::Warn | LspLogLevel::Info
                )
            }
            LspLogLevel::Debug => true,
        }
    }
}
