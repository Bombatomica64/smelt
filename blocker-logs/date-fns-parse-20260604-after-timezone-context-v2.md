# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `2`
- Guard runs: `1`
- Full suite executed: `false`

## Focused Runs

- `context`: `failed` - `test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 245 filtered out; finished in 0.04s`

```text

running 2 tests
. 1/2
__smelt_module_test::test_parse_context_allows_to_specify_the_context --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_context_allows_to_specify_the_context

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 245 filtered out; finished in 0.04s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```
- `DST`: `passed` - `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 247 filtered out; finished in 0.00s`

## Regression Guards

- `custom_locale`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.02s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `310`

## Summary By Code

1. **warning** `unused_mut` - 220 diagnostics
2. **warning** `unused_parens` - 52 diagnostics
3. **warning** `unused_assignments` - 38 diagnostics

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
     - `src/main.rs:989`
     - `src/main.rs:1100`
     - `src/main.rs:1153`
     - `src/main.rs:1167`
     - `src/main.rs:1405`
4. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index.rs:335`
     - `src/parse_index.rs:3519`
     - `src/parse_index.rs:3521`
     - `src/parse_index.rs:3531`
     - `src/parse_index.rs:3533`
5. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/parse_index.rs:518`
     - `src/parse_index.rs:520`
     - `src/main.rs:5256`
     - `src/main.rs:5289`
     - `src/main.rs:5296`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3778`
     - `src/main.rs:3786`
     - `src/main.rs:3796`
     - `src/main.rs:3806`
     - `src/main.rs:3878`
7. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/parse_index.rs:3511`
     - `src/parse_index.rs:3522`
     - `src/parse_index.rs:3534`
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
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:32`

## Cargo Stderr

```text
Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260603/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 49s
```
