//! CLI-owned diagnostic rendering helpers.
//!
//! `yarrow-core` owns the low-level `render()` formatting, but the CLI owns
//! *policy*: filling missing paths, printing the batch cap message, and mapping
//! ICE-tagged batches to exit `101`.

use std::process::ExitCode;

use yarrow_core::{
    ColorChoice, Diagnostic, DiagnosticBatch, SessionDiagnostics, SessionFailureKind, SourceFile,
};

/// Exit code for an unexpected panic caught by the driver (`catch_unwind`).
pub const EXIT_ICE: u8 = 101;

/// Render a single diagnostic with rustc-like layout.
///
/// This is a thin wrapper around `yarrow_core::render` that guarantees
/// `diag.path` is set (so callers don't have to).
pub fn render_diag(diag: &Diagnostic, file: &SourceFile, color: ColorChoice) -> String {
    let mut diag = diag.clone();
    if diag.path.is_empty() {
        diag.path = file.path.clone();
    }
    yarrow_core::render(&diag, file, color)
}

/// Render a diagnostic batch (compile/check errors), including the
/// "aborting due to N previous errors (limit M)" message when the batch hit
/// its configured cap.
pub fn render_batch(batch: &DiagnosticBatch, file: &SourceFile, color: ColorChoice) -> String {
    let mut out = String::new();
    for diag in batch.iter() {
        out.push_str(&render_diag(diag, file, color));
    }

    if batch.is_at_limit() {
        out.push_str(&format!(
            "error: aborting due to {} previous errors (limit {})\n",
            batch.len(),
            batch.limit()
        ));
    }

    out
}

/// Map a failed session to exit `1` (user) or `101` (ICE). Never used for I/O.
pub fn exit_for_session_failure(diags: &SessionDiagnostics) -> ExitCode {
    match diags.failure_kind() {
        SessionFailureKind::Ice => ExitCode::from(EXIT_ICE),
        SessionFailureKind::User => ExitCode::from(1),
    }
}

/// Print a session failure and return the matching exit code.
pub fn report_session_failure(diags: &SessionDiagnostics, color: ColorChoice) -> ExitCode {
    eprint!("{}", render_batch(&diags.batch, &diags.file, color));
    exit_for_session_failure(diags)
}

/// Format a caught panic payload for stderr (Stage 16).
pub fn format_panic_payload(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "box <unknown>".to_string()
    }
}

/// Print the rustc-style ICE banner after `catch_unwind`.
pub fn report_caught_panic(payload: &(dyn std::any::Any + Send)) {
    let detail = format_panic_payload(payload);
    eprintln!("error: internal compiler error: {detail}");
    eprintln!();
    eprintln!(
        "note: the compiler panicked unexpectedly; this is a bug, not a problem in your program"
    );
    eprintln!(
        "note: please report this at https://github.com/Yarrow-Programming-Language/yarrow/issues"
    );
}
