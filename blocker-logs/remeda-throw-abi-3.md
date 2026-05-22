# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `5`
- Warnings: `224`

## Summary By Code

1. **warning** `unused_mut` - 157 diagnostics
2. **warning** `unused_parens` - 37 diagnostics
3. **warning** `unused_assignments` - 29 diagnostics
4. **error** `E0599` - 2 diagnostics
5. **error** `E0271` - 1 diagnostic
6. **error** `E0282` - 1 diagnostic
7. **error** `E0308` - 1 diagnostic
8. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 157 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/clone.rs:16`
     - `src/clone.rs:17`
     - `src/conditional.rs:94`
     - `src/conditional.rs:95`
     - `src/conditional.rs:96`
2. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:38`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
3. **warning** `unused_parens` - 11 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:83`
     - `src/allPass_test.rs:84`
     - `src/anyPass_test.rs:83`
     - `src/anyPass_test.rs:84`
     - `src/purryOrderRules.rs:148`
4. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:92`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:77`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:78`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:63`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:94`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:79`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:80`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:65`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:91`
     - `src/funnel_lodash_debounce_test.rs:78`
     - `src/funnel_lodash_throttle_test.rs:77`
     - `src/funnel_lodash_throttle_test.rs:64`
8. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
10. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
11. **error** `E0271` - 1 occurrence
   - Message: expected `{closure@debounce.rs:20:289}` to return `Result<(), Box<dyn Error>>`, but it returns `()`
   - Examples:
     - `src/debounce.rs:20`
12. **error** `E0282` - 1 occurrence
   - Message: type annotations needed
   - Examples:
     - `src/firstBy.rs:11`
13. **error** `E0308` - 1 occurrence
   - Message: mismatched types
   - Examples:
     - `src/debounce.rs:99`
14. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for enum `Result<T, E>` in the current scope
   - Examples:
     - `src/conditional.rs:15`
15. **error** `E0599` - 1 occurrence
   - Message: the method `clone` exists for enum `Result<std::option::Option<SmeltUnknown>, Box<dyn StdError>>`, but its trait bounds were not satisfied
   - Examples:
     - `src/firstBy.rs:11`
16. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:85`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:57`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:56`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:102`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/pipe.rs:182`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:129`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:29`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:663`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:118`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:116`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
30. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:87`
31. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 5 previous errors; 224 warnings emitted
```
