# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260527/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `0`
- Full suite executed: `false`

## Focused Runs

- `test_parse_failure_returns_referencedate_if_datestring_and_formatstring_are_empty_strings`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.00s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260527/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `311`

## Summary By Code

1. **warning** `unused_mut` - 220 diagnostics
2. **warning** `unused_parens` - 52 diagnostics
3. **warning** `unused_assignments` - 39 diagnostics

## Groups

1. **warning** `unused_mut` - 220 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/buildFormatLongFn_index.rs:10`
     - `src/buildFormatLongFn_index.rs:7`
     - `src/buildLocalizeFn_index.rs:7`
     - `src/buildMatchFn_index.rs:54`
     - `src/buildMatchFn_index.rs:100`
2. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:2349`
     - `src/buildMatchFn_index.rs:2282`
     - `src/buildMatchFn_index.rs:2208`
     - `src/buildMatchFn_index.rs:2141`
     - `src/buildMatchFn_index.rs:2064`
3. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:969`
     - `src/main.rs:1080`
     - `src/main.rs:1133`
     - `src/main.rs:1147`
     - `src/main.rs:1385`
4. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index.rs:290`
     - `src/parse_index.rs:3424`
     - `src/parse_index.rs:3426`
     - `src/parse_index.rs:3436`
     - `src/parse_index.rs:3438`
5. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/parse_index.rs:423`
     - `src/parse_index.rs:425`
     - `src/main.rs:5236`
     - `src/main.rs:5269`
     - `src/main.rs:5276`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3758`
     - `src/main.rs:3766`
     - `src/main.rs:3776`
     - `src/main.rs:3786`
     - `src/main.rs:3858`
7. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/parse_index.rs:3416`
     - `src/parse_index.rs:3427`
     - `src/parse_index.rs:3439`
8. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/utils.rs:623`
9. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:665`
     - `src/buildLocalizeFn_index.rs:51`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:3736`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:30`

## Cargo Stderr

```text
Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260527/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 31s
```
