# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `true`

## Focused Runs

- `throws_an_error`: `passed` - `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 241 filtered out; finished in 0.09s`

## Regression Guards

- `date_prototype_tostring`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.10s`
- `context`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 245 filtered out; finished in 0.12s`
- `transitions`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.32s`

## Full Suite

- Status: `passed`
- Result: `test result: ok. 247 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.25s`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/date-fns-parse-20260604-after-timezone-green-full.md`
- Resolved tests: `5`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `309`

## Summary By Code

1. **warning** `unused_mut` - 219 diagnostics
2. **warning** `unused_parens` - 52 diagnostics
3. **warning** `unused_assignments` - 38 diagnostics

## Groups

1. **warning** `unused_mut` - 219 occurrences
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
     - `src/main.rs:5227`
     - `src/main.rs:5260`
     - `src/main.rs:5267`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3776`
     - `src/main.rs:3784`
     - `src/main.rs:3794`
     - `src/main.rs:3804`
     - `src/main.rs:3872`
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
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.30s
```
