# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260527/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `310`

## Summary By Code

1. **warning** `unused_mut` - 220 diagnostics
2. **warning** `unused_parens` - 51 diagnostics
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
     - `src/main.rs:911`
     - `src/main.rs:1022`
     - `src/main.rs:1075`
     - `src/main.rs:1089`
     - `src/main.rs:1327`
4. **warning** `unused_parens` - 12 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index.rs:3222`
     - `src/parse_index.rs:3224`
     - `src/parse_index.rs:3234`
     - `src/parse_index.rs:3236`
     - `src/main.rs:3742`
5. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/parse_index.rs:221`
     - `src/parse_index.rs:223`
     - `src/main.rs:4762`
     - `src/main.rs:4795`
     - `src/main.rs:4802`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3284`
     - `src/main.rs:3292`
     - `src/main.rs:3302`
     - `src/main.rs:3312`
     - `src/main.rs:3384`
7. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/parse_index.rs:3214`
     - `src/parse_index.rs:3225`
     - `src/parse_index.rs:3237`
8. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/utils.rs:623`
9. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:682`
     - `src/buildLocalizeFn_index.rs:52`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:3262`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:30`

## Cargo Stderr

```text
Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260527/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 27s
```
