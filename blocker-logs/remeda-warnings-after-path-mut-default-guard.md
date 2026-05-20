# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `11`
- Warnings: `221`

## Summary By Code

1. **warning** `unused_mut` - 119 diagnostics
2. **warning** `unused_assignments` - 64 diagnostics
3. **warning** `unused_parens` - 36 diagnostics
4. **error** `E0384` - 10 diagnostics
5. **warning** `unreachable_code` - 2 diagnostics
6. **error** `E0596` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 119 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/conditional.rs:20`
     - `src/countBy.rs:7`
     - `src/countBy.rs:34`
     - `src/dropFirstBy.rs:7`
     - `src/dropLastWhile.rs:7`
2. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:40`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
3. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:85`
     - `src/allPass_test.rs:86`
     - `src/anyPass_test.rs:85`
     - `src/anyPass_test.rs:86`
     - `src/purryOrderRules.rs:206`
4. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/pipe.rs:10`
     - `src/pipe.rs:182`
     - `src/pipe.rs:254`
     - `src/randomBigInt.rs:91`
     - `src/truncate.rs:31`
5. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:218`
     - `src/debounce.rs:195`
     - `src/debounce.rs:190`
     - `src/debounce.rs:116`
     - `src/debounce.rs:68`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:7`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_test.rs:7`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
8. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:160`
     - `src/truncate.rs:145`
     - `src/truncate.rs:119`
     - `src/truncate.rs:104`
9. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
10. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
11. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
12. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
13. **error** `E0384` - 2 occurrences
   - Message: cannot assign twice to immutable variable `_smelt_tmp_28`
   - Examples:
     - `src/truncate.rs:88`
     - `src/truncate.rs:88`
14. **error** `E0384` - 2 occurrences
   - Message: cannot assign twice to immutable variable `_smelt_tmp_32`
   - Examples:
     - `src/truncate.rs:91`
     - `src/truncate.rs:91`
15. **error** `E0384` - 2 occurrences
   - Message: cannot assign twice to immutable variable `global_separator`
   - Examples:
     - `src/truncate.rs:89`
     - `src/truncate.rs:89`
16. **warning** `unreachable_code` - 2 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/isDeepEqual.rs:305`
     - `src/sample.rs:85`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
18. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:8`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:8`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:130`
     - `src/truncate.rs:89`
20. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/fromKeys.rs:36`
     - `src/omit.rs:129`
21. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:151`
     - `src/truncate.rs:110`
22. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
23. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
24. **error** `E0384` - 1 occurrence
   - Message: cannot assign twice to immutable variable `_smelt_tmp_26`
   - Examples:
     - `src/truncate.rs:85`
25. **error** `E0384` - 1 occurrence
   - Message: cannot assign twice to immutable variable `_smelt_tmp_27`
   - Examples:
     - `src/truncate.rs:86`
26. **error** `E0384` - 1 occurrence
   - Message: cannot assign twice to immutable variable `_smelt_tmp_29`
   - Examples:
     - `src/truncate.rs:126`
27. **error** `E0384` - 1 occurrence
   - Message: cannot assign twice to immutable variable `_smelt_tmp_30`
   - Examples:
     - `src/truncate.rs:127`
28. **error** `E0596` - 1 occurrence
   - Message: cannot borrow `other_copy` as mutable, as it is not declared as mutable
   - Examples:
     - `src/isDeepEqual.rs:290`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
30. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
31. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
32. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
33. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
34. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
35. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
36. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
37. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/pipe.rs:8`
38. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:302`
39. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_cool_down_end` is never read
   - Examples:
     - `src/debounce.rs:8`
40. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_debounced_call` is never read
   - Examples:
     - `src/debounce.rs:9`
41. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_invoke` is never read
   - Examples:
     - `src/debounce.rs:7`
42. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
43. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
44. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_fn` is never read
   - Examples:
     - `src/pipe.rs:253`
45. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_sequence` is never read
   - Examples:
     - `src/pipe.rs:9`
46. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
47. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:47`
48. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
49. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
50. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
51. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
52. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
53. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:87`
54. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Updating crates.io index
     Locking 62 packages to latest Rust 1.93.0 compatible versions
      Adding rand v0.9.4 (available: v0.10.1)
   Compiling proc-macro2 v1.0.106
   Compiling unicode-ident v1.0.24
   Compiling quote v1.0.45
   Compiling libc v0.2.186
   Compiling getrandom v0.3.4
   Compiling zerocopy v0.8.48
   Compiling autocfg v1.5.0
   Compiling serde_core v1.0.228
    Checking memchr v2.8.0
    Checking cfg-if v1.0.4
   Compiling zmij v1.0.21
   Compiling serde v1.0.228
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
   Compiling serde_json v1.0.149
    Checking regex-syntax v0.8.10
    Checking tinyvec_macros v0.1.1
    Checking tinyvec v1.11.0
   Compiling syn v2.0.117
    Checking rand_core v0.9.5
    Checking iana-time-zone v0.1.65
    Checking pin-project-lite v0.2.17
    Checking itoa v1.0.18
    Checking chrono v0.4.44
    Checking unicode-normalization v0.1.25
    Checking regex-automata v0.4.14
    Checking regex v1.12.3
   Compiling tokio-macros v2.7.0
   Compiling serde_derive v1.0.228
    Checking ppv-lite86 v0.2.21
    Checking rand_chacha v0.9.0
    Checking tokio v1.52.3
    Checking rand v0.9.4
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 11 previous errors; 221 warnings emitted
```
