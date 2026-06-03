# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_addProp_test`: `failed` - `test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`

```text

running 2 tests
__smelt_module_addProp_test::test_data_first_simple_14 --- FAILED
__smelt_module_addProp_test::test_data_last_simple_15 --- FAILED

failures:

failures:
    __smelt_module_addProp_test::test_data_first_simple_14
    __smelt_module_addProp_test::test_data_last_simple_15

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_binarySearchCutoffIndex_test`: `failed` - `test result: FAILED. 6 passed; 5 failed; 0 ignored; 0 measured; 1778 filtered out; finished in 0.00s`

```text

running 11 tests
... 3/11
__smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_0_items --- FAILED
__smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_12_items --- FAILED
__smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_20_items --- FAILED
__smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_4_items --- FAILED
. 8/11
__smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_8_items --- FAILED
..
failures:

failures:
    __smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_0_items
    __smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_12_items
    __smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_20_items
    __smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_4_items
    __smelt_module_binarySearchCutoffIndex_test::test_binary_search_correctness_via_the_index_after_8_items

test result: FAILED. 6 passed; 5 failed; 0 ignored; 0 measured; 1778 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_ceil_test`: `failed` - `test result: FAILED. 15 passed; 6 failed; 0 ignored; 0 measured; 1768 filtered out; finished in 0.00s`

```text
thread '__smelt_module_ceil_test::test_data_first_case_1_should_throw_for_d_precision_49' (4005091) panicked at src/ceil.rs:13:1206:
precision must be an integer: inf
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '__smelt_module_ceil_test::test_data_first_should_throw_for_precision_higher_than_15_and_lower_than_15_51' (4005095) panicked at src/ceil.rs:13:1206:
precision must be between -15 and 15

thread '__smelt_module_ceil_test::test_data_first_case_0_should_throw_for_d_precision_48' (4005089) panicked at src/ceil.rs:13:1206:
precision must be an integer: NaN
Error: 
thread '__smelt_module_ceil_test::test_data_first_should_throw_for_non_integer_precision_50' (4005094) panicked at src/ceil.rs:13:1206:
precision must be an integer: 21.37
Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_ceil_test::test_data_first_should_throw_for_precision_higher_than_15_and_lower_than_15_51' (4005095) panicked at src/ceil.rs:13:1206:
precision must be between -15 and 15

thread '__smelt_module_ceil_test::test_data_last_should_throw_for_non_integer_precision_60' (4005104) panicked at src/ceil.rs:13:1206:
precision must be an integer: 21.37

thread '__smelt_module_ceil_test::test_data_last_case_1_should_throw_for_d_precision_59' (4005102) panicked at src/ceil.rs:13:1206:
precision must be an integer: inf
Error: 
thread '__smelt_module_ceil_test::test_data_last_should_throw_for_precision_higher_than_15_and_lower_than_15_61' (4005105) panicked at src/ceil.rs:13:1206:
precision must be between -15 and 15
Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_ceil_test::test_data_last_case_0_should_throw_for_d_precision_58' (4005100) panicked at src/ceil.rs:13:1206:
precision must be an integer: NaN
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_entries_test`: `passed` - `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 1784 filtered out; finished in 0.00s`
- `__smelt_module_invert_test`: `passed` - `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1778 filtered out; finished in 0.00s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `240`

## Summary By Code

1. **warning** `unused_parens` - 106 diagnostics
2. **warning** `unused_mut` - 96 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 96 occurrences
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
3. **warning** `unused_unsafe` - 23 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
     - `src/debounce.rs:266`
4. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/find.rs:23`
     - `src/findIndex.rs:18`
5. **warning** `unused_parens` - 13 occurrences
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
10. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:670`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
18. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.94s
```
