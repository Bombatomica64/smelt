# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_resume_20260526/format_probe/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `295`

## Summary By Code

1. **warning** `unused_mut` - 233 diagnostics
2. **warning** `unused_assignments` - 38 diagnostics
3. **warning** `unused_parens` - 24 diagnostics

## Groups

1. **warning** `unused_mut` - 233 occurrences
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
3. **warning** `unused_parens` - 14 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:934`
     - `src/main.rs:963`
     - `src/main.rs:1073`
     - `src/main.rs:1184`
     - `src/main.rs:1237`
4. **warning** `unused_parens` - 10 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/format_index.rs:108`
     - `src/format_index.rs:110`
     - `src/format_index.rs:114`
     - `src/format_index.rs:114`
     - `src/format_index.rs:114`
5. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:2796`
     - `src/main.rs:2930`
6. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:682`
     - `src/buildLocalizeFn_index.rs:52`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`

## Cargo Stderr

```text
Checking memchr v2.8.0
    Checking regex-syntax v0.8.10
    Checking siphasher v1.0.3
    Checking iana-time-zone v0.1.65
    Checking bit-vec v0.8.0
    Checking num-traits v0.2.19
    Checking pin-project-lite v0.2.17
    Checking tokio v1.52.3
    Checking phf_shared v0.12.1
    Checking bit-set v0.8.0
    Checking phf v0.12.1
    Checking chrono v0.4.44
    Checking aho-corasick v1.1.4
    Checking chrono-tz v0.10.4
    Checking regex-automata v0.4.14
    Checking regex v1.12.3
    Checking fancy-regex v0.14.0
    Checking date_fns_format_probe v0.1.0 (/tmp/smelt_date_fns_resume_20260526/format_probe/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 02s
```
