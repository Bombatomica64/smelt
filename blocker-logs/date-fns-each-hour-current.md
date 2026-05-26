# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_resume_20260526/each_hour_probe/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `20`

## Summary By Code

1. **warning** `unused_mut` - 12 diagnostics
2. **warning** `unused_parens` - 7 diagnostics
3. **warning** `unused_assignments` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 12 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/defaultOptions_index.rs:13`
     - `src/defaultOptions_index.rs:18`
     - `src/eachHourOfInterval_index.rs:42`
     - `src/eachHourOfInterval_index.rs:64`
     - `src/eachHourOfInterval_index.rs:8`
2. **warning** `unused_parens` - 7 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:821`
     - `src/main.rs:850`
     - `src/main.rs:960`
     - `src/main.rs:1071`
     - `src/main.rs:1124`
3. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:30`

## Cargo Stderr

```text
Checking date_fns_each_hour_probe v0.1.0 (/tmp/smelt_date_fns_resume_20260526/each_hour_probe/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
```
