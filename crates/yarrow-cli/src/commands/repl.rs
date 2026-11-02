//! Interactive interpreter (`yarrow repl`).
//!
//! Line-oriented loop over [`yarrow_core::EvalContext`].

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use yarrow_core::{ColorChoice, EvalContext, Session, SessionOptions};

use crate::args::GlobalArgs;
use crate::commands::print_run_result;
use crate::diagnostics::render_batch;

/// Start an interactive interpret loop until EOF / `exit` / `quit`.
pub fn run_repl(global: &GlobalArgs) -> ExitCode {
    let color = global.color.to_core();

    let mut opts = SessionOptions::new("<repl>");
    for p in &global.search_paths {
        opts.module_search_paths.push(p.clone());
    }
    opts.error_limit = global.error_limit;

    let session = Session::new(opts);
    let mut ctx = EvalContext::new();
    for p in &global.search_paths {
        ctx.add_module_search_path(p.clone());
    }

    if !global.quiet {
        eprintln!("yarrow repl (interpret). Type `exit` or Ctrl-D to quit.");
    }

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if !global.quiet {
            eprint!(">>> ");
            let _ = io::stderr().flush();
        }

        let Some(read) = lines.next() else {
            if !global.quiet {
                eprintln!();
            }
            break;
        };

        let line = match read {
            Ok(line) => line,
            Err(e) => {
                eprintln!("error: failed to read stdin: {e}");
                return ExitCode::from(2);
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        eval_line(&session, &mut ctx, trimmed, color, global.progress());
        let _ = io::stdout().flush();
    }

    ExitCode::SUCCESS
}

fn eval_line(
    session: &Session,
    _ctx: &mut EvalContext,
    line: &str,
    color: ColorChoice,
    progress: bool,
) {
    // Wrap the line as a main function if it doesn't look like a program
    let source = if looks_like_program(line) {
        format!("{line}\n")
    } else {
        wrap_as_main(line)
    };

    if progress {
        eprintln!("repl source:\n{source}");
    }

    match session.interpret_source(source) {
        Ok(result) => print_run_result(result),
        Err(diags) => {
            eprint!("{}", render_batch(&diags.batch, &diags.file, color));
        }
    }
}

fn wrap_as_main(body: &str) -> String {
    format!("main function do\n\t{body}\nend\n")
}

fn looks_like_program(src: &str) -> bool {
    src.split_whitespace()
        .any(|w| w == "function" || w == "require")
}
