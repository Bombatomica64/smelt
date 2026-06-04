# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `2`
- Guard runs: `1`
- Full suite executed: `true`

## Focused Runs

- `context`: `failed` - `no test-result line`

```text
error[E0614]: type `f64` cannot be dereferenced
   --> src/main.rs:637:114
    |
637 | ...ateTime::<chrono::Utc>::from_timestamp_millis(*timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_t...
    |                                                  ^^^^^^^^^^^^^ can't be dereferenced

For more information about this error, try `rustc --explain E0614`.
error: could not compile `date_fns_parse_probe` (bin "date_fns_parse_probe" test) due to 1 previous error
```
- `DST`: `failed` - `no test-result line`

```text
error[E0614]: type `f64` cannot be dereferenced
   --> src/main.rs:637:114
    |
637 | ...ateTime::<chrono::Utc>::from_timestamp_millis(*timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_t...
    |                                                  ^^^^^^^^^^^^^ can't be dereferenced

For more information about this error, try `rustc --explain E0614`.
error: could not compile `date_fns_parse_probe` (bin "date_fns_parse_probe" test) due to 1 previous error
```

## Regression Guards

- `custom_locale`: `failed` - `no test-result line`

```text
error[E0614]: type `f64` cannot be dereferenced
   --> src/main.rs:637:114
    |
637 | ...ateTime::<chrono::Utc>::from_timestamp_millis(*timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_t...
    |                                                  ^^^^^^^^^^^^^ can't be dereferenced

For more information about this error, try `rustc --explain E0614`.
error: could not compile `date_fns_parse_probe` (bin "date_fns_parse_probe" test) due to 1 previous error
```

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/date-fns-parse-20260604-after-typed-virtual-origin-full.md`
- Resolved tests: `8`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `1`
- Warnings: `310`

## Summary By Code

1. **warning** `unused_mut` - 220 diagnostics
2. **warning** `unused_parens` - 52 diagnostics
3. **warning** `unused_assignments` - 38 diagnostics
4. **error** `E0614` - 1 diagnostic

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
10. **error** `E0614` - 1 occurrence
   - Message: type `f64` cannot be dereferenced
   - Examples:
     - `src/main.rs:637`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:32`

## Cargo Stderr

```text
Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260603/dist)
error: could not compile `date_fns_parse_probe` (bin "date_fns_parse_probe") due to 1 previous error; 310 warnings emitted
```
