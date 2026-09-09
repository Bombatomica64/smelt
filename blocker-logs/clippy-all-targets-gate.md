# `cargo clippy --all-targets` is green again

Round 13, item 0. CLAUDE.md names `cargo clippy --all-targets` as a pre-commit
gate, and it had been failing for everyone. Now it reports **0 errors** outside
`smelt-specialize`, whose five are known and owned elsewhere.

## Why it took several passes

Clippy stops at the FIRST failing target in a crate, so the count is not the
work remaining. Each fix uncovered the next target behind it: 24 → 4 → 6 → 1 →
2 → 1 → 5 → 6 → … Twelve rounds of "fix, re-run, new file appears". Worth
recording, because "clippy says N errors" on this workspace means "at least N".

## The lints, and what each fix actually was

**`needless_raw_string_hashes` — 139 sites, 13 files.** `r#"…"#` where the body
holds no `"` needs no hashes. Fixing these file-by-file was going to take as
many passes as there are test targets, so this one was applied by the lint's own
rule across `crates/`: de-hash a single-hash raw literal exactly when its body
contains neither `"` nor `#`. Most sites are Python-frontend test fixtures.

That blanket pass had one real failure mode and it did fire: **doc comments that
QUOTE `r#"…"#` as syntax**. In `emitter/rendered_text_rewrite.rs` it turned

```rust
/// Covers string (`"…"`), byte-string (`b"…"`), raw-string (`r"…"`, `r#"…"#`,
/// `br#"…"#`), char, and byte-char literals.
```

into a list naming `r"…"` twice — documentation made wrong by a lint fix. That
file is reverted whole; a grep for added comment lines containing `r"` confirms
it was the only one, and a grep for `r\"` / `\"#` confirms nothing landed inside
a string literal either. The lesson is the check, not the revert: a mechanical
rewrite over source has to be audited against comments and nested literals
before it is trusted.

**`too_many_lines` — 5 test functions, 61–86 lines.** Every one is a `#[test]`
whose body is a single TypeScript fixture program plus a one-line call, and each
carries `#[ignore = "slow: emits and runs a generated test crate"]`. Splitting
one into three or five Rust tests would multiply that emit-and-run cost with no
extra coverage, so instead the program is hoisted to a module-level `const` —
the convention `CALLABLE_OVERLOAD_PRELUDE` in `part_7_tests.rs` already follows.
The reasoning comment stays on the test, where it belongs. Files:
`form_data_runtime.rs`, `map_lookup_runtime.rs`,
`projected_receiver_place_runtime.rs`, `callback_destructuring_runtime.rs`,
`symbol_key_runtime.rs`.

**`similar_names` — 8 sites.** Two shapes, and neither wanted an `#[allow]`:

* `crates/smelt-transpiler/tests/common/mod.rs` held `expected_hir`/`actual_hir`
  and `expected_mir`/`actual_mir` in one scope — four bindings separated by a
  three-letter suffix — because the HIR and MIR halves of
  `verify_example_dumps` were the same three lines twice. They are now one
  `ensure_dump_matches(kind, name, golden, command, input)` helper, so the
  duplication goes with the lint.
* `generic_bindings.rs` tests paired `callee` with `caller`. The test locals are
  renamed `call_site`; the production parameter named `caller` (which is
  correct there) is untouched.

**`redundant_clone` — 4 sites**, in `manifest.rs` (×3) and
`python_specialization_parity.rs`. Each clones a value at its last use.

**`doc_markdown` — 5 sites.** `NodeNext`, `SameValueZero` (×2), `snake_case`d.

**`implicit_clone` — 1 site.** `node.to_path_buf()` on a `PathBuf` →
`node.clone()`; `node` is read again afterwards, so the clone is real, only its
spelling was indirect.

**`iter_on_single_items` — 2 sites.** `[x].into_iter().collect()` →
`HashSet::from([x])` in `type_substitution.rs` tests.

**`needless_borrow` — 3 sites.** `&format!(…)` where `format!(…)` already
satisfies the parameter, in `probe_diagnostics_tests.rs`.

**`char_lit_as_pattern` — 1 site.** `body.contains("7")` → `body.contains('7')`.

## Not touched

`smelt-specialize`: `prereq.rs:202`, `:215` and `sandbox.rs:1089`, `:1105`
(`panic_in_result_fn`) plus `python.rs:452` (`redundant_clone`). Known, owned
elsewhere, left alone on instruction.

## Gates

cargo test **105 result lines / 0 failures / 2658 passed** — frontend 1069 and
codegen 1024 both unchanged, which is the assertion that matters for a change
that rewrote 139 literals across the test corpus. clippy `--lib` clean;
`--all-targets` clean outside `smelt-specialize`; examples SmeltUnknown
avoidable 0 (+0).
