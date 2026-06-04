# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `1`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_test::test_parse_month_formatting_abbreviated`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s`

```text
    --> src/main.rs:6185:39
     |
6185 |     fn set(&self, date: SmeltUnknown, mut flags: ParseFlags, value: f64) -> SmeltUnknown {
     |                                       ----^^^^^
     |                                       |
     |                                       help: remove this `mut`

warning: variable does not need to be mutable
    --> src/main.rs:6272:39
     |
6272 |     fn set(&self, date: SmeltUnknown, mut flags: ParseFlags, value: f64) -> SmeltUnknown {
     |                                       ----^^^^^
     |                                       |
     |                                       help: remove this `mut`

warning: unused return value of `clone` that must be used
    --> src/test.rs:5646:5
     |
5646 |     _smelt_tmp_1.clone();
     |     ^^^^^^^^^^^^^^^^^^^^
     |
     = note: cloning is often expensive and is not expected to have side effects
     = note: `#[warn(unused_must_use)]` (part of `#[warn(unused)]`) on by default
help: use `let _ = ...` to ignore the resulting value
     |
5646 |     let _ = _smelt_tmp_1.clone();
     |     +++++++

Error: Custom { kind: Other, error: "expect(...).toEqual(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```

## Regression Guards

- `__smelt_module_test::test_parse_two_digit_year`: `passed` - `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 247 filtered out; finished in 0.00s`
