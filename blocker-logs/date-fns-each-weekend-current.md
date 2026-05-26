# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_resume_20260526/each_weekend_probe/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `27`

## Summary By Code

1. **warning** `unused_mut` - 15 diagnostics
2. **warning** `unused_parens` - 11 diagnostics
3. **warning** `unused_assignments` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 15 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/defaultOptions_index.rs:13`
     - `src/defaultOptions_index.rs:18`
     - `src/eachDayOfInterval_index.rs:44`
     - `src/eachDayOfInterval_index.rs:66`
     - `src/eachDayOfInterval_index.rs:8`
2. **warning** `unused_parens` - 11 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:839`
     - `src/main.rs:868`
     - `src/main.rs:978`
     - `src/main.rs:1089`
     - `src/main.rs:1142`
3. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:30`

## Cargo Stderr

```text
Checking date_fns_each_weekend_probe v0.1.0 (/tmp/smelt_date_fns_resume_20260526/each_weekend_probe/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
```
