# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `18`
- Warnings: `260`

## Summary By Code

1. **warning** `unused_mut` - 130 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **error** `E0308` - 14 diagnostics
5. **warning** `unused_assignments` - 14 diagnostics
6. **error** `E0277` - 3 diagnostics
7. **error** `no-code` - 1 diagnostic
8. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 130 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_parens` - 23 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
4. **warning** `unused_unsafe` - 23 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/difference.rs:55`
     - `src/drop.rs:36`
     - `src/funnel.rs:32`
     - `src/funnel.rs:83`
5. **error** `E0308` - 14 occurrences
   - Message: mismatched types
   - Examples:
     - `src/toCamelCase.rs:68`
     - `src/toCamelCase.rs:95`
     - `src/toCamelCase.rs:101`
     - `src/toCamelCase.rs:128`
     - `src/toCamelCase.rs:152`
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
8. **error** `E0277` - 2 occurrences
   - Message: can't compare `i64` with `f64`
   - Examples:
     - `src/uniqueWith.rs:28`
     - `src/uniqueWith.rs:39`
9. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
10. **error** `no-code` - 1 occurrence
   - Message: cast cannot be followed by a method call
   - Examples:
     - `src/firstBy_test.rs:515`
11. **error** `E0277` - 1 occurrence
   - Message: the `?` operator can only be used on `Option`s, not `Result`s, in a closure that returns `Option`
   - Examples:
     - `src/debounce.rs:314`
12. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:37`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
19. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
20. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 18 previous errors; 260 warnings emitted
```
