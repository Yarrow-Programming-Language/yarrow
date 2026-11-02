//! Result types for program execution.

/// Result of running a Yarrow program entry.
#[derive(Debug, Clone, PartialEq)]
pub enum RunResult {
    /// Entry returned no value.
    Void,
    /// Entry returned an integer.
    Int(i64),
    /// Entry returned a float.
    Float(f64),
    /// Entry returned a boolean.
    Bool(bool),
    /// Entry returned a string.
    Str(String),
}
