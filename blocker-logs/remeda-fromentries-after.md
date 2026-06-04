# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `1`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_fromEntries_test`: `failed` - `no test-result line`

```text
error[E0425]: cannot find function `smelt_number_to_string` in this scope
 --> src/fromEntries.rs:9:385
  |
9 | ... => value, SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), _ => retu...
  |                                              ^^^^^^^^^^^^^^^^^^^^^^ not found in this scope

error[E0277]: the trait bound `SmeltRecord<_, _>: From<Vec<_>>` is not satisfied
    --> src/fromEntries.rs:9:121
     |
   9 | ...g_0.clone() { SmeltUnknown::Array(entries) => SmeltRecord::from(entries.into_iter().filter_map(|entry| match entry { SmeltUnkno...
     |                                                  ^^^^^^^^^^^ unsatisfied trait bound
     |
help: the trait `From<Vec<_>>` is not implemented for `SmeltRecord<_, _>`
      but trait `From<[(_, _); _]>` is implemented for it
    --> src/main.rs:1271:1
     |
1271 | impl<K: Eq + ::std::hash::Hash + Clone, V, const N: usize> From<[(K, V); N]> for SmeltRecord<K, V> {
     | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
     = help: for that trait implementation, expected `[(_, _); _]`, found `Vec<_>`

Some errors have detailed explanations: E0277, E0425.
For more information about an error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```

## Regression Guards

- `__smelt_module_dropFirstBy_test`: `failed` - `no test-result line`

```text
error[E0425]: cannot find function `smelt_number_to_string` in this scope
 --> src/fromEntries.rs:9:385
  |
9 | ... => value, SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), _ => retu...
  |                                              ^^^^^^^^^^^^^^^^^^^^^^ not found in this scope

error[E0277]: the trait bound `SmeltRecord<_, _>: From<Vec<_>>` is not satisfied
    --> src/fromEntries.rs:9:121
     |
   9 | ...g_0.clone() { SmeltUnknown::Array(entries) => SmeltRecord::from(entries.into_iter().filter_map(|entry| match entry { SmeltUnkno...
     |                                                  ^^^^^^^^^^^ unsatisfied trait bound
     |
help: the trait `From<Vec<_>>` is not implemented for `SmeltRecord<_, _>`
      but trait `From<[(_, _); _]>` is implemented for it
    --> src/main.rs:1271:1
     |
1271 | impl<K: Eq + ::std::hash::Hash + Clone, V, const N: usize> From<[(K, V); N]> for SmeltRecord<K, V> {
     | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
     = help: for that trait implementation, expected `[(_, _); _]`, found `Vec<_>`

Some errors have detailed explanations: E0277, E0425.
For more information about an error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `2`
- Warnings: `253`

## Summary By Code

1. **warning** `unused_parens` - 114 diagnostics
2. **warning** `unused_mut` - 101 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **error** `E0277` - 1 diagnostic
6. **error** `E0425` - 1 diagnostic
7. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 101 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:105`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_parens` - 26 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:21`
     - `src/filter.rs:22`
     - `src/find.rs:22`
4. **warning** `unused_unsafe` - 23 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
     - `src/debounce.rs:266`
5. **warning** `unused_parens` - 15 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:86`
     - `src/allPass_test.rs:87`
     - `src/anyPass_test.rs:86`
     - `src/anyPass_test.rs:87`
     - `src/purryOrderRules.rs:148`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:111`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:96`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:92`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:77`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:109`
     - `src/funnel_lodash_debounce_test.rs:96`
     - `src/funnel_lodash_throttle_test.rs:90`
     - `src/funnel_lodash_throttle_test.rs:77`
8. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:47`
     - `src/toCamelCase.rs:79`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
10. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltRecord<_, _>: From<Vec<_>>` is not satisfied
   - Examples:
     - `src/fromEntries.rs:9`
11. **error** `E0425` - 1 occurrence
   - Message: cannot find function `smelt_number_to_string` in this scope
   - Examples:
     - `src/fromEntries.rs:9`
12. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
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
     - `src/evolve_test.rs:670`
19. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
20. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 2 previous errors; 253 warnings emitted
```
