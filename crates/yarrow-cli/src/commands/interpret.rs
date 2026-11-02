//! Implementation of the `interpret` subcommand.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use yarrow_core::{Session, SessionOptions};

use crate::args::GlobalArgs;
use crate::commands::print_run_result;
use crate::diagnostics::report_session_failure;

/// Check and interpret `file`, printing any return value from the entry.
///
/// Non-empty `program_args` (after `--`) are rejected until core exposes an
/// interpret argv API.
pub fn interpret_file(
    file: &Path,
    entry_name: &str,
    program_args: &[OsString],
    global: &GlobalArgs,
) -> ExitCode {
    if !program_args.is_empty() {
        eprintln!(
            "error: program arguments are not supported with interpret (no language argv API yet)"
        );
        return ExitCode::from(2);
    }

    let path = file.to_string_lossy().into_owned();
    let color = global.color.to_core();

    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let mut opts = SessionOptions::new(path.clone());
    for p in &global.search_paths {
        opts.module_search_paths.push(p.clone());
    }
    opts.error_limit = global.error_limit;
    opts.entry_name = entry_name.to_string();

    if global.progress() {
        eprintln!("interpreting {path} (entry {entry_name})");
    }

    let session = Session::new(opts);
    match session.interpret_source(source) {
        Ok(result) => {
            print_run_result(result);
            ExitCode::SUCCESS
        }
        Err(diags) => report_session_failure(&diags, color),
    }
}
