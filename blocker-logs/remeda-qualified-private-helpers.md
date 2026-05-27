# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `20`
- Warnings: `185`

## Summary By Code

1. **warning** `unused_mut` - 101 diagnostics
2. **warning** `unused_parens` - 45 diagnostics
3. **warning** `unused_unsafe` - 22 diagnostics
4. **error** `E0308` - 18 diagnostics
5. **warning** `unused_assignments` - 16 diagnostics
6. **error** `E0369` - 1 diagnostic
7. **error** `E0599` - 1 diagnostic
8. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 101 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:90`
2. **warning** `unused_unsafe` - 22 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/debounce.rs:62`
     - `src/debounce.rs:112`
     - `src/debounce.rs:144`
     - `src/debounce.rs:172`
     - `src/debounce.rs:238`
3. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/find.rs:23`
     - `src/findIndex.rs:18`
4. **error** `E0308` - 18 occurrences
   - Message: mismatched types
   - Examples:
     - `src/when.rs:16`
     - `src/when.rs:16`
     - `src/zip.rs:132`
     - `src/zip.rs:132`
     - `src/zip.rs:132`
5. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:84`
     - `src/allPass_test.rs:85`
     - `src/anyPass_test.rs:84`
     - `src/anyPass_test.rs:85`
     - `src/purryOrderRules.rs:148`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:97`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:82`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:78`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:63`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:96`
     - `src/funnel_lodash_debounce_test.rs:83`
     - `src/funnel_lodash_throttle_test.rs:77`
     - `src/funnel_lodash_throttle_test.rs:64`
8. **warning** `unused_parens` - 4 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/flat.rs:91`
     - `src/funnel.rs:186`
     - `src/funnel.rs:251`
     - `src/splitAt.rs:38`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:48`
     - `src/toCamelCase.rs:80`
10. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
11. **error** `E0369` - 1 occurrence
   - Message: cannot subtract `{float}` from `std::option::Option<f64>`
   - Examples:
     - `src/flat.rs:91`
12. **error** `E0599` - 1 occurrence
   - Message: no method named `flat` found for enum `SmeltUnknown` in the current scope
   - Examples:
     - `src/main.rs:1351`
13. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:122`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:20`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:654`
22. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:88`
23. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 20 previous errors; 185 warnings emitted
```
