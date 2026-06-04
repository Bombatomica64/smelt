# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `2`
- Full suite executed: `true`

## Focused Runs

- `__smelt_module_test::test_parse_era_abbreviated`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s`

```text

running 1 test
__smelt_module_test::test_parse_era_abbreviated --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_era_abbreviated

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s

Error: Custom { kind: Other, error: "expect(...).toEqual(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```

## Regression Guards

- `__smelt_module_test::test_parse_with_options_strictvalidation_true_month_formatting_returns_invalid_date_for_invalid_month`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.02s`
- `__smelt_module_test::test_parse_month_formatting_abbreviated`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.05s`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 236 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.61s`
- Failing tests: `11`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 11 | `__smelt_module_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/date-fns-parse-20260603-month-focused-after-regex-array-consts.md`
- Resolved tests: `26`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_test::test_parse_common_formats_date_prototype_tostring`
- `__smelt_module_test::test_parse_context_allows_to_specify_the_context`
- `__smelt_module_test::test_parse_custom_locale_allows_to_pass_a_custom_locale`
- `__smelt_module_test::test_parse_era_abbreviated`
- `__smelt_module_test::test_parse_era_narrow`
- `__smelt_module_test::test_parse_era_with_week_numbering_year`
- `__smelt_module_test::test_parse_time_zones_properly_parses_dates_around_dst_transitions`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_d_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_dd_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yy_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yyyy_token_is_used`

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
     - `src/main.rs:976`
     - `src/main.rs:1087`
     - `src/main.rs:1140`
     - `src/main.rs:1154`
     - `src/main.rs:1392`
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
     - `src/main.rs:5243`
     - `src/main.rs:5276`
     - `src/main.rs:5283`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3765`
     - `src/main.rs:3773`
     - `src/main.rs:3783`
     - `src/main.rs:3793`
     - `src/main.rs:3865`
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 54s
```
