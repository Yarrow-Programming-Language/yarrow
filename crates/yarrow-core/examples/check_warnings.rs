//! Stage 38 gate: warning catalog on `docs/examples/valid/16_*`–`21_*`.
//!
//! ```bash
//! cargo run -p yarrow_core --example check_warnings
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use yarrow_core::{ColorChoice, CompileOptions, ExecutionMode, Session, render_batch};

fn check_fixture(repo: &std::path::Path, rel: &str) -> yarrow_core::CheckedProgram {
    let path = repo.join(rel);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read {rel}: {e}");
    });
    let mut opts = CompileOptions::new(path.display().to_string());
    opts.mode = ExecutionMode::Check;
    Session::new(opts).check_source(source).unwrap_or_else(|d| {
        eprintln!("{}", render_batch(&d.batch, &d.file, ColorChoice::Never));
        panic!("{rel} must check successfully");
    })
}

fn codes(checked: &yarrow_core::CheckedProgram) -> HashSet<String> {
    checked.warnings.iter().map(|d| d.code.clone()).collect()
}

fn expect_codes(checked: &yarrow_core::CheckedProgram, rel: &str, want: &[&str]) {
    let got = codes(checked);
    for code in want {
        assert!(
            got.contains(*code),
            "{rel}: expected warning {code}, got {got:?}"
        );
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/yarrow-core → repo root");

    let unused = check_fixture(repo, "docs/examples/valid/16_unused_warnings.yar");
    expect_codes(&unused, "16_unused_warnings.yar", &["W401", "W402", "W403"]);

    let never = check_fixture(repo, "docs/examples/valid/17_never_written_warnings.yar");
    expect_codes(&never, "17_never_written_warnings.yar", &["W404", "W405"]);

    let dead = check_fixture(repo, "docs/examples/valid/18_unreachable_warnings.yar");
    expect_codes(&dead, "18_unreachable_warnings.yar", &["W407"]);

    let ambig = check_fixture(
        repo,
        "docs/examples/valid/19_require_ambiguous_warnings.yar",
    );
    expect_codes(&ambig, "19_require_ambiguous_warnings.yar", &["W406"]);

    let empty_match = check_fixture(repo, "docs/examples/valid/20_empty_match_warnings.yar");
    expect_codes(&empty_match, "20_empty_match_warnings.yar", &["W408"]);

    let empty_ctrl = check_fixture(repo, "docs/examples/valid/21_empty_if_unsafe_warnings.yar");
    expect_codes(
        &empty_ctrl,
        "21_empty_if_unsafe_warnings.yar",
        &["W409", "W410"],
    );

    println!("ok: W401–W410 warning fixtures");
}
