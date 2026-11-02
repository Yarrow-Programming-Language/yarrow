//! Yarrow command-line driver.
//!
//! This crate owns argument parsing, subcommands, stdout/stderr, and process
//! exit codes. All compile work is delegated to `yarrow_core`.
//!
//! # Entry point
//!
//! ```no_run
//! use yarrow_cli::run;
//! use std::process::ExitCode;
//!
//! fn main() -> ExitCode {
//!     run(std::env::args_os())
//! }
//! ```

mod args;
mod artifacts;
mod commands;
mod diagnostics;

pub use args::{Cli, Cmd, TargetKind};

use std::ffi::OsString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;

use yarrow_core::{CompileOptions, Session};

use crate::diagnostics::{EXIT_ICE, report_caught_panic, report_session_failure};

/// Main entry point. Accepts anything convertible to an iterator of OS
/// strings so it is easy to test with `&[&str]` slices.
///
/// Unexpected panics from command dispatch are caught and reported as an
/// internal compiler error (exit `101`). Ordinary session diagnostics stay
/// exit `1` unless tagged ICE (`E999` → `101`).
pub fn run<I, S>(args: I) -> ExitCode
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    use clap::Parser as _;
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // Clap writes help/version to stdout and errors to stderr.
            err.print().ok();
            let code = if err.use_stderr() { 2 } else { 0 };
            return ExitCode::from(code);
        }
    };

    // Documented Stage 16 gate hooks (not for normal use).
    // `YARROW_DEBUG_ICE=panic` → catch_unwind path; `session` → E999 → 101.
    match std::env::var("YARROW_DEBUG_ICE").ok().as_deref() {
        Some("panic") => {
            // Fall through into catch_unwind so the banner + 101 path runs.
        }
        Some("session") => {
            let session = Session::new(CompileOptions::new("<debug-ice>"));
            let diags = session.debug_trigger_ice("YARROW_DEBUG_ICE=session");
            return report_session_failure(&diags, cli.global.color.to_core());
        }
        _ => {}
    }

    // Suppress the default panic dump; we print a rustc-style ICE banner instead.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = catch_unwind(AssertUnwindSafe(|| dispatch(cli)));
    std::panic::set_hook(prev_hook);

    match caught {
        Ok(code) => code,
        Err(payload) => {
            report_caught_panic(&*payload);
            ExitCode::from(EXIT_ICE)
        }
    }
}

fn dispatch(cli: Cli) -> ExitCode {
    if std::env::var("YARROW_DEBUG_ICE").ok().as_deref() == Some("panic") {
        panic!("deliberate ICE panic (YARROW_DEBUG_ICE=panic)");
    }

    match (cli.cmd, cli.file) {
        (
            Some(Cmd::Run {
                file,
                target,
                main,
                program_args,
            }),
            None,
        ) => commands::run_file(&file, target, &main, &program_args, &cli.global),
        (None, Some(file)) => {
            // `yarrow <file>` now uses the interpreter by default.
            commands::interpret_file(&file, "main", &[], &cli.global)
        }
        (
            Some(Cmd::Compile {
                file,
                target,
                emit,
                main,
                output,
            }),
            None,
        ) => commands::compile_file(&file, target, emit, &main, output.as_deref(), &cli.global),
        (
            Some(Cmd::Check {
                files,
                corpus,
                main,
            }),
            None,
        ) => {
            if let Some(dir) = corpus {
                commands::check_corpus(&dir, &main, &cli.global)
            } else {
                commands::check_files(&files, &main, &cli.global)
            }
        }
        (
            Some(Cmd::Interpret {
                file,
                main,
                program_args,
            }),
            None,
        ) => commands::interpret_file(&file, &main, &program_args, &cli.global),
        (Some(Cmd::Repl), None) => commands::run_repl(&cli.global),
        (
            Some(Cmd::Fmt {
                check,
                stdin,
                max_width,
                sort_requires,
                no_sort_requires,
                reorder_layout,
                best_effort,
                range,
                paths,
            }),
            None,
        ) => {
            let range = match range {
                Some(s) => match yarrow_fmt::parse_range_arg(&s) {
                    Ok(r) => Some(r),
                    Err(msg) => {
                        eprintln!("yarrow fmt: {msg}");
                        return ExitCode::from(2);
                    }
                },
                None => None,
            };
            commands::run_fmt_command(yarrow_fmt::FmtInput {
                options: yarrow_fmt::FormatOptions {
                    max_width,
                    sort_requires: yarrow_fmt::resolve_sort_requires_flags(
                        sort_requires,
                        no_sort_requires,
                    ),
                    reorder_layout,
                },
                check,
                stdin,
                best_effort,
                range,
                paths,
            })
        }
        (
            Some(Cmd::Lsp {
                stdio,
                listen,
                search_paths,
                main,
                no_format,
                no_inlay,
                log_level,
            }),
            None,
        ) => {
            if listen.is_none() && !stdio {
                eprintln!("error: pass --stdio (default) or --listen HOST:PORT");
                ExitCode::from(2)
            } else {
                commands::run_lsp(
                    &cli.global,
                    &search_paths,
                    &main,
                    !no_format,
                    !no_inlay,
                    log_level,
                    listen.as_deref(),
                )
            }
        }
        (Some(Cmd::Dump { file, emit }), None) => commands::dump_file(&file, emit, &cli.global),
        (Some(Cmd::Explain { code }), None) => commands::explain_code(&code, &cli.global),
        (Some(Cmd::Clean), None) => commands::clean_artifacts(&cli.global),
        (Some(Cmd::Version), None) => {
            // Clap's built-in `--version` prints only the top-level crate version;
            // this subcommand matches `rustc -V` style UX.
            println!("yarrow {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        (Some(_), Some(_)) => {
            // Shouldn't happen in practice (clap would accept it), but keep it
            // deterministic.
            eprintln!("error: provide either a subcommand or a single FILE argument");
            ExitCode::from(2)
        }
        (None, None) => {
            // `arg_required_else_help` isn't enough once everything is optional.
            // Print a concise usage and keep exit code consistent.
            eprintln!(
                "usage: yarrow <file.yar>\n       yarrow interpret <file.yar>\n       yarrow repl\n       yarrow fmt [--check] [PATH...]\n       yarrow lsp\n       yarrow clean\n\n       (Hidden compiler commands: run, compile, check, dump)"
            );
            ExitCode::from(2)
        }
    }
}
