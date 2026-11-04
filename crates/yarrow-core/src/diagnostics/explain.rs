//! Long-form explanations for diagnostic codes (`yarrow explain E308`).

/// One catalog entry: code, short title, and a paragraph for `--explain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainEntry {
    pub code: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

/// Teachable notes from Phase B diagnostics, keyed by code.
const CATALOG: &[ExplainEntry] = &[
    ExplainEntry {
        code: "E308",
        title: "unwrap and fallible returns",
        body: "\
A function declared `with |T Err|` is fallible: success carries `T` (or nothing \
if `T` is void), and failure carries an error member in an envelope.

`unwrap` applies to that envelope. On success it pushes the payload. On failure \
it propagates the error to the caller, so the caller must also be fallible \
(`with |T Err|`). Using `unwrap` in a function that cannot error is rejected.

On a value that is not an error envelope, `unwrap` is an identity (a no-op).

If the caller cannot propagate, recover with `handle ... fallback ... end` \
instead of `unwrap`. Anonymous `|T U|` unions are only supported as this \
fallible-return form, not as general types.",
    },
    ExplainEntry {
        code: "E370",
        title: "unsafe operation outside an unsafe context",
        body: "\
Raw pointers, `mem.allocate` / `free` / `load` / `store`, and calls to \
`unsafe function` need an unsafe context. Wrap the operation in \
`unsafe ... end`, or mark the enclosing function `unsafe function`.

`unsafe` does not turn off type, stack, ownership, or borrow checking.",
    },
    ExplainEntry {
        code: "E371",
        title: "dup on a non-copy type",
        body: "\
`dup` bitwise-duplicates copy types only: integers, floats, bool, rune, \
enum, `array<T N>`, and `pointer<T>`.

For `string`, `list<T>`, `hashmap<K V>`, unions, and structs, duplicate \
access goes through `borrow` (or `move` to transfer ownership) instead of \
`dup`.",
    },
    ExplainEntry {
        code: "E373",
        title: "use after move",
        body: "\
`move` transfers ownership: the source name is empty afterwards. Read from the \
destination variable, or avoid moving if you still need the source.",
    },
    ExplainEntry {
        code: "E374",
        title: "mutation or drop while a borrow is live",
        body: "\
A live `borrow` (or region put) pins the owner until the reference is released. \
Pop or otherwise consume the reference before mutating or dropping the owner.",
    },
    ExplainEntry {
        code: "E375",
        title: "second overlapping borrow",
        body: "\
Yarrow allows only one live borrow of a value at a time. Release the first \
reference (for example with `pop`) before borrowing again.",
    },
    ExplainEntry {
        code: "E376",
        title: "use after region free",
        body: "\
Values put into a region become invalid after `region.free`. Finish using \
borrows of region-attached values before freeing the region.",
    },
    ExplainEntry {
        code: "E334",
        title: "integer-only remainder and power",
        body: "\
`%` is integer remainder and `^` is integer exponentiation. They apply to \
integer operands only, not to floats.

For floats, use `/` for division. There is no float power operator yet; use \
integer `^` or an explicit conversion if you need exponentiation on integers \
first.",
    },
    ExplainEntry {
        code: "E382",
        title: "module dependency cycle",
        body: "\
A `require` closed a cycle in the module graph (A loads B which loads A again \
while A is still loading). Break the cycle by removing or restructuring one \
`require`. Shared helpers should be required from leaves toward shared modules, \
not mutually.",
    },
    ExplainEntry {
        code: "E383",
        title: "missing or empty project root",
        body: "\
`check_project` needs at least one root `.yar` file. \
`ProjectOptions::from_root_paths` fails with this code when a path is missing \
or unreadable. Supply existing paths or in-memory `ProjectRoot` sources.",
    },
    ExplainEntry {
        code: "E999",
        title: "internal compiler error (ICE)",
        body: "\
The compiler hit an invariant or API-boundary failure that is not a mistake in \
your program. Drivers should treat `E999` / `SessionDiagnostics::is_ice` as exit \
`101` (distinct from ordinary diagnostics at exit `1`). This is not recoverable \
by editing source; please report it with the command line and a minimal \
reproducer. Gate: `cargo run -p yarrow_core --example check_ice`.",
    },
    ExplainEntry {
        code: "W401",
        title: "unused binding",
        body: "\
A `const`, `mutable`, or `static` name was declared but never read, written, or \
moved from. Remove the binding, or use the value (for example with `pop` / \
`drop`, a call, or an assignment).",
    },
    ExplainEntry {
        code: "W402",
        title: "unused require",
        body: "\
A `require` brought a module or item into scope, but nothing in this file used \
it (no call through the alias, and no bare imported name). Remove the require, \
or call a function it exposes.",
    },
    ExplainEntry {
        code: "W403",
        title: "dead stack value",
        body: "\
A value was left on the operand stack and discarded at scope exit (return or \
falling off the end of a function). Consume it with an operator, `pop` / \
`drop`, a call, or a binding, or avoid pushing it.",
    },
    ExplainEntry {
        code: "W404",
        title: "never-written mutable",
        body: "\
A `mutable` binding of a scalar or enum type was read but never reassigned with \
`set` or `move`. Prefer `const` unless the binding will be written. Containers, \
structs, and pointers are not covered: they may mutate without rebinding the name.",
    },
    ExplainEntry {
        code: "W405",
        title: "redundant copy",
        body: "\
A function parameter is marked `copy`, but the parameter type is not heap-backed \
(`string`, `list`, `hashmap`, struct, union, or array). `copy` deep-copies those \
heap values into the callee; on scalars, enums, and pointers it has no effect. \
Omit `copy` unless the parameter needs a deep copy.",
    },
    ExplainEntry {
        code: "W406",
        title: "ambiguous require path",
        body: "\
A dotted `require` path names both a nested module file and a function in the \
parent module. The function wins (item import). Rename one of them, or require \
the parent module and call the function through that scope.",
    },
    ExplainEntry {
        code: "W407",
        title: "unreachable code",
        body: "\
A statement appears after divergent control flow (`return`, or both branches of \
an `if` that return, or `loop.break` / `loop.continue`). It can never run. Remove \
it or move it before the divergent statement.",
    },
    ExplainEntry {
        code: "W408",
        title: "empty match arm",
        body: "\
A `match` case arm has no statements between `case` and `end`. Selecting that \
arm does nothing. Add a body, fold the case into `else`, or remove the arm if it \
is not needed.",
    },
    ExplainEntry {
        code: "W409",
        title: "empty if then branch",
        body: "\
An `if` then-branch has no statements before `else` or `end`. Invert the \
condition and keep only the other branch, or add the intended body.",
    },
    ExplainEntry {
        code: "W410",
        title: "empty unsafe block",
        body: "\
An `unsafe ... end` block contains no statements. Unsafe marks where raw pointers \
and related ops are allowed; an empty block grants nothing. Remove it, or move \
the unsafe operations inside.",
    },
];

/// Normalize a user-supplied code (`e308`, `308`, `E308`, `w401`) to catalog form.
///
/// Bare digits default to an error code (`308` → `E308`). Warning codes must
/// use a `W` prefix so they do not collide with error numbers.
pub fn normalize_code(code: &str) -> String {
    let trimmed = code.trim().trim_start_matches('#');
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with('E') || upper.starts_with('W') {
        upper
    } else if !upper.is_empty() && upper.chars().all(|c| c.is_ascii_digit()) {
        format!("E{upper}")
    } else {
        upper
    }
}

/// Look up the long-form explanation for a diagnostic code.
pub fn explain_code(code: &str) -> Option<&'static ExplainEntry> {
    let key = normalize_code(code);
    CATALOG.iter().find(|e| e.code == key)
}

/// Render a catalog entry the way `rustc --explain` prints a code.
pub fn format_explain(entry: &ExplainEntry) -> String {
    let kind = if entry.code.starts_with('W') {
        "warning"
    } else {
        "error"
    };
    format!(
        "{kind}[{}]: {}\n\n{}\n",
        entry.code, entry.title, entry.body
    )
}
