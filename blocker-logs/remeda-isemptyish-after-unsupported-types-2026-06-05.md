# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `4`
- Full suite executed: `true`

## Focused Runs

- `__smelt_module_isEmptyish_test`: `failed` - `no test-result line`

```text
    --> src/funnel_test.rs:2726:333
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                          ^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`
     |
    ::: src/main.rs:1400:5
     |
1400 |     Null,
     |     ---- unit variant defined here

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:370
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | ...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                               ^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:395
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                        ^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:452
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 328 previous errors
```

## Regression Guards

- `__smelt_module_hasProp_test`: `failed` - `no test-result line`

```text
    --> src/funnel_test.rs:2726:333
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                          ^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`
     |
    ::: src/main.rs:1400:5
     |
1400 |     Null,
     |     ---- unit variant defined here

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:370
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | ...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                               ^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:395
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                        ^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:452
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 328 previous errors
```
- `__smelt_module_entries_test`: `failed` - `no test-result line`

```text
    --> src/funnel_test.rs:2726:333
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                          ^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`
     |
    ::: src/main.rs:1400:5
     |
1400 |     Null,
     |     ---- unit variant defined here

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:370
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | ...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                               ^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:395
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                        ^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:452
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 328 previous errors
```
- `__smelt_module_invert_test`: `failed` - `no test-result line`

```text
    --> src/funnel_test.rs:2726:333
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                          ^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`
     |
    ::: src/main.rs:1400:5
     |
1400 |     Null,
     |     ---- unit variant defined here

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:370
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | ...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                               ^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:395
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                        ^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:452
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 328 previous errors
```
- `__smelt_module_isEmpty_test`: `failed` - `no test-result line`

```text
    --> src/funnel_test.rs:2726:333
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                          ^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`
     |
    ::: src/main.rs:1400:5
     |
1400 |     Null,
     |     ---- unit variant defined here

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:370
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | ...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                               ^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:395
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                        ^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

error[E0308]: mismatched types
    --> src/funnel_test.rs:2726:452
     |
2726 | ...ch value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) =>...
     |       ------------- this expression has type `std::string::String`                                                                                                                                                                                                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `String`, found `SmeltUnknown`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 328 previous errors
```

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-isemptyish-after-object-create-proto-marker-2026-06-05.md`
- Resolved tests: `164`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `8`
- Warnings: `269`

## Summary By Code

1. **warning** `unused_mut` - 131 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **error** `E0308` - 8 diagnostics
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 131 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
     - `src/clamp.rs:8`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_unsafe` - 31 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
4. **warning** `unused_parens` - 23 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **error** `E0308` - 8 occurrences
   - Message: mismatched types
   - Examples:
     - `src/funnel_test.rs:2726`
     - `src/funnel_test.rs:2726`
     - `src/funnel_test.rs:2726`
     - `src/funnel_test.rs:2726`
     - `src/funnel_test.rs:2726`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:116`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:98`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:97`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:79`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
8. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
9. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:37`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
16. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 8 previous errors; 269 warnings emitted
```
