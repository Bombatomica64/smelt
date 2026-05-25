# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_resume/format_probe/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `176`

## Summary By Code

1. **warning** `unused_mut` - 137 diagnostics
2. **warning** `unused_parens` - 25 diagnostics
3. **warning** `unused_assignments` - 14 diagnostics

## Groups

1. **warning** `unused_mut` - 137 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/buildFormatLongFn_index.rs:10`
     - `src/buildFormatLongFn_index.rs:7`
     - `src/buildLocalizeFn_index.rs:7`
     - `src/buildMatchFn_index.rs:48`
     - `src/buildMatchFn_index.rs:87`
2. **warning** `unused_parens` - 14 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:908`
     - `src/main.rs:937`
     - `src/main.rs:1047`
     - `src/main.rs:1158`
     - `src/main.rs:1211`
3. **warning** `unused_parens` - 10 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/format_index.rs:260`
     - `src/format_index.rs:262`
     - `src/format_index.rs:266`
     - `src/format_index.rs:266`
     - `src/format_index.rs:266`
4. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:584`
     - `src/buildMatchFn_index.rs:517`
     - `src/buildMatchFn_index.rs:443`
     - `src/buildMatchFn_index.rs:376`
     - `src/buildMatchFn_index.rs:287`
5. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:2765`
     - `src/main.rs:2899`
6. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:368`
     - `src/buildLocalizeFn_index.rs:52`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:20`
9. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/format_index.rs:161`

## Cargo Stderr

```text
Checking date_fns_format_probe v0.1.0 (/tmp/smelt_date_fns_resume/format_probe/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.88s
```
