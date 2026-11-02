mod explain;
mod fmt;
mod interpret;
mod lsp;
mod repl;

pub use explain::explain_code;
pub use fmt::run_fmt_command;
pub use interpret::interpret_file;
pub use lsp::{LspLogLevel, run_lsp};
pub use repl::run_repl;

use yarrow_core::RunResult;

/// Print a supported entry return value the same way for `run` and `interpret`.
pub(crate) fn print_run_result(result: RunResult) {
    match result {
        RunResult::Void => {}
        RunResult::Int(v) => println!("{v}"),
        RunResult::Bool(b) => println!("{b}"),
        RunResult::Float(f) => println!("{f}"),
        RunResult::Str(s) => println!("{s}"),
    }
}
