# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `144`
- Warnings: `135`

## Summary By Code

1. **error** `E0425` - 143 diagnostics
2. **warning** `unused_mut` - 83 diagnostics
3. **warning** `unused_parens` - 38 diagnostics
4. **warning** `unused_assignments` - 12 diagnostics
5. **error** `E0282` - 1 diagnostic
6. **warning** `unreachable_code` - 1 diagnostic
7. **warning** `unused_unsafe` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 83 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:90`
2. **error** `E0425` - 43 occurrences
   - Message: cannot find value `smelt_capture_scope` in this scope
   - Examples:
     - `src/main.rs:1180`
     - `src/main.rs:1180`
     - `src/main.rs:1180`
     - `src/main.rs:1180`
     - `src/main.rs:1180`
3. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:48`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/find.rs:23`
     - `src/findIndex.rs:18`
4. **error** `E0425` - 12 occurrences
   - Message: cannot find value `interval_timeout_id` in this scope
   - Examples:
     - `src/funnel.rs:28`
     - `src/funnel.rs:28`
     - `src/funnel.rs:73`
     - `src/funnel.rs:73`
     - `src/funnel.rs:94`
5. **warning** `unused_parens` - 11 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:84`
     - `src/allPass_test.rs:85`
     - `src/anyPass_test.rs:84`
     - `src/anyPass_test.rs:85`
     - `src/purryOrderRules.rs:145`
6. **error** `E0425` - 10 occurrences
   - Message: cannot find value `burst_timeout_id` in this scope
   - Examples:
     - `src/funnel.rs:72`
     - `src/funnel.rs:72`
     - `src/funnel.rs:93`
     - `src/funnel.rs:93`
     - `src/funnel.rs:115`
7. **error** `E0425` - 10 occurrences
   - Message: cannot find value `cached_value` in this scope
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:87`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:102`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:68`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:83`
     - `src/funnel_remeda_debounce_test.rs:98`
8. **error** `E0425` - 10 occurrences
   - Message: cannot find value `remaining` in this scope
   - Examples:
     - `src/difference.rs:49`
     - `src/difference.rs:49`
     - `src/difference.rs:53`
     - `src/difference.rs:53`
     - `src/intersection.rs:49`
9. **error** `E0425` - 8 occurrences
   - Message: cannot find value `cool_down_timeout_id` in this scope
   - Examples:
     - `src/debounce.rs:103`
     - `src/debounce.rs:103`
     - `src/debounce.rs:156`
     - `src/debounce.rs:156`
     - `src/debounce.rs:224`
10. **error** `E0425` - 8 occurrences
   - Message: cannot find value `latest_call_args` in this scope
   - Examples:
     - `src/debounce.rs:58`
     - `src/debounce.rs:58`
     - `src/debounce.rs:105`
     - `src/debounce.rs:105`
     - `src/debounce.rs:133`
11. **error** `E0425` - 8 occurrences
   - Message: cannot find value `result` in this scope
   - Examples:
     - `src/debounce.rs:60`
     - `src/debounce.rs:60`
     - `src/debounce.rs:161`
     - `src/debounce.rs:161`
     - `src/debounce.rs:267`
12. **error** `E0425` - 6 occurrences
   - Message: cannot find value `burst_start_timestamp` in this scope
   - Examples:
     - `src/funnel.rs:92`
     - `src/funnel.rs:92`
     - `src/funnel.rs:114`
     - `src/funnel.rs:114`
     - `src/funnel.rs:292`
13. **error** `E0425` - 6 occurrences
   - Message: cannot find value `max_wait_timeout_id` in this scope
   - Examples:
     - `src/debounce.rs:59`
     - `src/debounce.rs:59`
     - `src/debounce.rs:135`
     - `src/debounce.rs:135`
     - `src/debounce.rs:226`
14. **error** `E0425` - 6 occurrences
   - Message: cannot find value `prepared_data` in this scope
   - Examples:
     - `src/funnel.rs:30`
     - `src/funnel.rs:30`
     - `src/funnel.rs:122`
     - `src/funnel.rs:122`
     - `src/funnel.rs:295`
15. **error** `E0425` - 4 occurrences
   - Message: cannot find value `set` in this scope
   - Examples:
     - `src/unique.rs:20`
     - `src/unique.rs:20`
     - `src/uniqueBy.rs:23`
     - `src/uniqueBy.rs:23`
16. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:96`
     - `src/funnel_lodash_debounce_test.rs:83`
     - `src/funnel_lodash_throttle_test.rs:77`
     - `src/funnel_lodash_throttle_test.rs:64`
17. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:48`
     - `src/toCamelCase.rs:50`
18. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
19. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:177`
     - `src/funnel.rs:240`
     - `src/splitAt.rs:38`
20. **error** `E0425` - 2 occurrences
   - Message: cannot find value `as_set` in this scope
   - Examples:
     - `src/isIncludedIn.rs:19`
     - `src/isIncludedIn.rs:19`
21. **error** `E0425` - 2 occurrences
   - Message: cannot find value `called` in this scope
   - Examples:
     - `src/once.rs:13`
     - `src/once.rs:13`
22. **error** `E0425` - 2 occurrences
   - Message: cannot find value `left` in this scope
   - Examples:
     - `src/drop.rs:34`
     - `src/drop.rs:34`
23. **error** `E0425` - 2 occurrences
   - Message: cannot find value `ret` in this scope
   - Examples:
     - `src/once.rs:11`
     - `src/once.rs:11`
24. **error** `E0425` - 2 occurrences
   - Message: cannot find value `trigger_at` in this scope
   - Examples:
     - `src/funnel.rs:124`
     - `src/funnel.rs:124`
25. **error** `E0425` - 2 occurrences
   - Message: cannot find value `wait_ms` in this scope
   - Examples:
     - `src/debounce.rs:163`
     - `src/debounce.rs:163`
26. **error** `E0282` - 1 occurrence
   - Message: type annotations needed
   - Examples:
     - `src/funnel.rs:56`
27. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:58`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:57`
30. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
31. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:98`
32. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:20`
33. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
34. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:32`
35. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:654`
36. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:88`
37. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`
38. **warning** `unused_unsafe` - 1 occurrence
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/words.rs:46`

## Cargo Stderr

```text
Blocking waiting for file lock on build directory
    Checking bit-vec v0.8.0
    Checking regex-automata v0.4.14
    Checking bit-set v0.8.0
    Checking regex v1.12.3
    Checking fancy-regex v0.14.0
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 144 previous errors; 135 warnings emitted
```
