//! Clap argument definitions for the Yarrow CLI.

use std::ffi::OsString;

use clap::{Parser, Subcommand};

/// Yarrow language interpreter and tools.
#[derive(Debug, Parser)]
#[command(
    name = "yarrow",
    about = "Yarrow language interpreter and tools",
    version,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Optional default positional. When present and no subcommand is
    /// provided, it is interpreted using the stack VM.
    #[arg(value_name = "FILE")]
    pub file: Option<std::path::PathBuf>,

    /// Explicit subcommands (use `yarrow run <FILE>`).
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

/// Flags that apply to every subcommand.
#[derive(Debug, Clone, clap::Args)]
pub struct GlobalArgs {
    /// When to color diagnostics: `always`, `never`, or `auto` (default).
    ///
    /// With `auto`, `NO_COLOR` forces never; `CLICOLOR_FORCE` / `FORCE_COLOR`
    /// (set and non-empty) force always. Explicit `always` / `never` win over
    /// env. Does not color `fmt` / `lsp` child output (those APIs have no
    /// color knob).
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    pub color: ColorArg,

    /// Maximum number of errors to report before stopping.
    #[arg(long, global = true, value_name = "N", default_value_t = yarrow_core::DEFAULT_ERROR_LIMIT)]
    pub error_limit: usize,

    /// Extra module search path (repeatable).
    #[arg(short = 'L', long = "search-path", global = true, value_name = "DIR")]
    pub search_paths: Vec<std::path::PathBuf>,

    /// Suppress driver chatter (`wrote …`, repl banners, `-v` progress).
    ///
    /// Never suppresses diagnostics or `explain` / `dump` payload on stdout.
    /// `fmt` has no quiet API; its check/error lines still print.
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Extra driver progress on stderr (`running …`, dump kind, etc.).
    ///
    /// Ignored when `-q` is set. Does not change exit codes or hide errors.
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
}

impl GlobalArgs {
    /// Whether to emit optional progress lines on stderr (`-v` and not `-q`).
    pub fn progress(&self) -> bool {
        self.verbose && !self.quiet
    }
}

/// Intermediate form printed by `dump --emit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum EmitKind {
    Tokens,
    Ast,
    Ir,
}

/// Artifact written by `compile --target object --emit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CompileEmitKind {
    /// Relocatable native object (`.o`). Default.
    Object,
    /// Linked host executable.
    Exe,
}

/// Compile / run backend selected with `--target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TargetKind {
    /// Cranelift in-process machine code (`--target jit`).
    Jit,
    /// Native relocatable object / linked executable (default).
    Object,
}

/// `--color` argument value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorArg {
    Always,
    Never,
    Auto,
}

impl ColorArg {
    pub fn to_core(self) -> yarrow_core::ColorChoice {
        match self {
            ColorArg::Always => yarrow_core::ColorChoice::Always,
            ColorArg::Never => yarrow_core::ColorChoice::Never,
            ColorArg::Auto => yarrow_core::ColorChoice::Auto,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    #[command(hide = true)]
    /// Compile and run a Yarrow source file (compiler - hidden).
    Run {
        /// Source file to compile and run.
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,

        /// Codegen backend (`jit` or `object`). Default: `object`.
        #[arg(long, value_enum, default_value = "object")]
        target: TargetKind,

        /// Top-level entry function name (default `main`).
        #[arg(long, value_name = "NAME", default_value = "main")]
        main: String,

        /// Arguments forwarded to the program (after `--`).
        #[arg(last = true, value_name = "ARGS")]
        program_args: Vec<OsString>,
    },

    #[command(hide = true)]
    /// Compile a Yarrow source file (compiler - hidden).
    Compile {
        /// Source file to compile.
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,

        /// Codegen backend (`jit` or `object`). Default: `object`.
        #[arg(long, value_enum, default_value = "object")]
        target: TargetKind,

        /// Artifact for `--target object`: relocatable `object` or linked `exe`.
        /// Ignored for `--target jit`. Default: `object`.
        #[arg(long, value_enum, default_value = "object")]
        emit: CompileEmitKind,

        /// Top-level entry function name (default `main`).
        #[arg(long, value_name = "NAME", default_value = "main")]
        main: String,

        /// Output path for `--target object` (default `object` → `./<stem>.o`,
        /// `exe` → `./<stem>`). Overrides the default; recorded for `clean`.
        #[arg(short = 'o', long = "output", value_name = "PATH")]
        output: Option<std::path::PathBuf>,
    },

    /// Remove artifacts recorded by this CLI's `compile`.
    ///
    /// Deletes only paths listed in `.yarrow-build/artifacts` (never a
    /// recursive `*.o` wipe). Missing files or a missing manifest exit `0`.
    /// Objects written outside this CLI (or before recording existed) are not
    /// removed unless they appear in the manifest.
    Clean,

    #[command(hide = true)]
    /// Check source without running main (compiler - hidden).
    Check {
        /// Root `.yar` file(s). One path → single-file check; two or more →
        /// project check. Required unless `--corpus` is set.
        #[arg(
            value_name = "FILE",
            num_args = 0..,
            required_unless_present = "corpus",
            conflicts_with = "corpus"
        )]
        files: Vec<std::path::PathBuf>,

        /// Directory of programs to check (immediate `*.yar` only).
        #[arg(long, value_name = "DIR", conflicts_with = "files")]
        corpus: Option<std::path::PathBuf>,

        /// Top-level entry function name (default `main`).
        #[arg(long, value_name = "NAME", default_value = "main")]
        main: String,
    },

    /// Execute a Yarrow source file on the stack VM interpreter.
    ///
    /// Executes the entry (`main` or `--main`) without compiling to machine code.
    /// This is the default action when passing a file directly to yarrow.
    Interpret {
        /// Source file to interpret.
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,

        /// Top-level entry function name (default `main`).
        #[arg(long, value_name = "NAME", default_value = "main")]
        main: String,

        /// Arguments forwarded to the program (after `--`).
        #[arg(last = true, value_name = "ARGS")]
        program_args: Vec<OsString>,
    },

    /// Interactive interpret loop (line-oriented).
    ///
    /// Snippets without a top-level `function` / `require` are wrapped as a
    /// `main` body. Expressions that leave a value print as the entry result.
    /// Exit with `exit`, `quit`, or EOF.
    Repl,

    /// Format Yarrow source to match the style guide.
    ///
    /// Respects paired `# yarrow-fmt-ignore-begin` / `# yarrow-fmt-ignore-end`
    /// regions (see `docs/STYLE_GUIDE.md`).
    ///
    /// Delegates in-process to `yarrow_fmt` (same as the `yarrow-fmt` binary).
    /// Exit: 0 ok / already formatted; 1 would change (`--check`) or format
    /// failure; 2 usage / I/O.
    Fmt {
        /// Exit 1 if any file would change; do not write.
        #[arg(long)]
        check: bool,

        /// Read source from stdin and write formatted text to stdout.
        #[arg(long)]
        stdin: bool,

        /// Soft wrap width in columns (default 100; minimum 20).
        #[arg(long, value_name = "N", default_value_t = 100)]
        max_width: usize,

        /// Force-on top-level require sorting (default is already on).
        #[arg(long = "sort-requires", overrides_with = "no_sort_requires")]
        sort_requires: bool,

        /// Keep top-level require source order.
        #[arg(long = "no-sort-requires", overrides_with = "sort_requires")]
        no_sort_requires: bool,

        /// Reorder top-level items to style-guide file layout (high churn; opt-in).
        #[arg(long)]
        reorder_layout: bool,

        /// On parse failure, apply LF / trailing-WS / final-newline hygiene only.
        #[arg(long)]
        best_effort: bool,

        /// Format a UTF-8 byte span `START:END` (exclusive end) via `format_range`.
        ///
        /// Requires `--stdin` or exactly one `.yar` file. Prints a
        /// `yarrow-fmt-range-v1` edit encoding to stdout (does not write the
        /// file). Incompatible with `--best-effort`. Editors should keep using
        /// LSP range formatting.
        #[arg(long, value_name = "START:END")]
        range: Option<String>,

        /// Files or directories (directories recurse for `*.yar`). Required unless `--stdin`.
        #[arg(value_name = "PATH")]
        paths: Vec<std::path::PathBuf>,
    },

    /// Start the Yarrow language server (LSP over stdio or TCP).
    ///
    /// Delegates in-process to `yarrow_lsp`. Point editors at `yarrow lsp`.
    Lsp {
        /// Speak LSP over stdin/stdout (default when `--listen` is omitted).
        #[arg(long, default_value_t = true)]
        stdio: bool,

        /// Accept one TCP client at HOST:PORT (port `0` = ephemeral).
        #[arg(long, value_name = "HOST:PORT")]
        listen: Option<String>,

        /// Extra module search path (in addition to global `-L`).
        #[arg(short = 'L', long = "search-path", value_name = "DIR")]
        search_paths: Vec<std::path::PathBuf>,

        /// Top-level entry function name (default `main`).
        #[arg(long, value_name = "NAME", default_value = "main")]
        main: String,

        /// Disable `textDocument/formatting`.
        #[arg(long)]
        no_format: bool,

        /// Disable `textDocument/inlayHint`.
        #[arg(long)]
        no_inlay: bool,

        /// Log level for process messages on stderr.
        #[arg(long, value_enum, default_value = "info")]
        log_level: crate::commands::LspLogLevel,
    },

    /// Print the long form of a diagnostic code.
    Explain {
        /// Diagnostic code, for example `E308`.
        #[arg(value_name = "CODE")]
        code: String,
    },

    /// Print the CLI version.
    Version,

    #[command(hide = true)]
    /// Print an intermediate representation and exit (compiler - hidden).
    Dump {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
        #[arg(long, value_enum, default_value = "ir")]
        emit: EmitKind,
    },
}
