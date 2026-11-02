//! Program entry point constants.
//!
//! These symbols define the entry point names for Yarrow programs. The interpreter
//! uses these to locate and execute the program's entry function.

/// Linker symbol object emit exports as the host process entry.
pub const PROCESS_MAIN_SYMBOL: &str = "main";

/// Default language entry name.
pub const DEFAULT_ENTRY_NAME: &str = "main";
