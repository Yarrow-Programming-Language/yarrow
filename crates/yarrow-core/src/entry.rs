//! Program entry for AOT object emit (Stages 17–18).
//!
//! Object emit exports linker [`PROCESS_MAIN_SYMBOL`] (`main`) with host process
//! ABI `() -> i32`. That Cranelift trampoline calls the Yarrow entry selected by
//! [`crate::CompileOptions::entry_name`] (default `"main"`) and maps its return
//! to a process exit status.
//!
//! The Yarrow entry body is kept under a private local symbol so it does not
//! clash with process `main` when `entry_name` is also `"main"`.
//!
//! JIT and interpret call the Yarrow entry by name; they never use process `main`.

/// Linker symbol object emit exports as the host process entry.
pub const PROCESS_MAIN_SYMBOL: &str = "main";

/// Default language entry name.
pub const DEFAULT_ENTRY_NAME: &str = "main";
