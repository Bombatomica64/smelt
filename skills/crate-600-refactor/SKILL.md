# Crate 600 LOC Refactor Skill

## Goal
Refactor Rust crates so every Rust source file stays at or under 600 LOC while preserving behavior and tests.

## Hard constraints
- Do not exceed 600 LOC in any `*.rs` file under the owned crate.
- Preserve behavior and public API unless explicitly required for modularization.
- Add module/function doc comments (`///` and `//!`) for newly introduced helpers.
- Avoid broad architectural rewrites unrelated to splitting large files.
- Do not revert unrelated in-progress edits in a dirty worktree.

## Cross-crate shape conventions
For conceptually similar logic, keep naming aligned across frontend/HIR/MIR:
- `map.rs` for map/dict/object mapping behavior
- `list.rs` for list/array behavior
- `call.rs` for call/invoke lowering
- `control_flow.rs` for branching/loops/matches
- `literals.rs` for literal lowering/formatting
- `types.rs` for type-shape helpers
- `validate.rs` for validation logic
- `format.rs` for pretty/diagnostic formatting
- `tests_*` files for split test suites

Only create modules that are actually needed in the crate.

## Procedure
1. Inventory files over 600 LOC in the crate.
2. Propose a module split that keeps cohesive logic together.
3. Extract helpers first, then move feature slices.
4. Ensure `mod` declarations and visibility are minimal and explicit.
5. Run crate-targeted tests/checks when possible.
6. Leave concise summary listing each new file and responsibility.

## Validation
At minimum, run the most relevant command set available for your scope:
- `cargo test -p <crate>`
- `cargo check -p <crate>`
- `cargo clippy -p <crate> -- -D warnings`

If workspace-level validation is requested, parent agent will run it globally.
