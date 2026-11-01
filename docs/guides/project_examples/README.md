# Multi-root Project Structure

This directory demonstrates how to structure a Yarrow project with multiple root files that share module search paths. There is no manifest or package-manager syntax — each root is a normal compilation unit and `require` works as described in [RUNTIME.md](../../docs/RUNTIME.md).

## Product Shape

- An **explicit set of root `.yar` files** sharing module search paths
- No manifest file required
- Each root is a normal compilation unit
- Modules resolve relative to each root's directory

## Files

| File | Role |
| --- | --- |
| [`root_a.yar`](root_a.yar) | Root A — a compilation entry point |
| [`root_b.yar`](root_b.yar) | Root B — another entry point (does not require A) |
| [`shared/util.yar`](shared/util.yar) | Shared helper module, required as `"shared.util"` |

## CLI Usage

Check both roots together:
```bash
yarrow check docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar
```

Run with a specific entry:
```bash
yarrow run docs/guides/project_examples/root_a.yar docs/guides/project_examples/root_b.yar --main root_a
```

## Library API

```rust
use yarrow_core::{ProjectOptions, check_project};

let mut opts = ProjectOptions::from_root_paths([
    "docs/guides/project_examples/root_a.yar",
    "docs/guides/project_examples/root_b.yar",
])?;

// Parent directories of each root are added as search paths
// "shared.util" resolves to shared/util.yar
let checked = check_project(&opts)?;

assert_eq!(checked.roots.len(), 2);
assert!(checked.graph.modules.iter().any(|m| m == "shared.util"));
```

## LSP Usage

Pass the root paths via `initializationOptions.projectRoots`. See test scenarios in `crates/yarrow-lsp/scripts/harness.mjs`: `project-roots` and `project-missing-root`.

## Related

- [RUNTIME.md — Projects section](../../docs/RUNTIME.md#projects)
- [yarrow-core Stage 28](../../crates/yarrow-core/PLAN.md)
- [yarrow-cli Stage 13](../../crates/yarrow-cli/PLAN.md)
- [yarrow-lsp Stage 21](../../crates/yarrow-lsp/PLAN.md)
