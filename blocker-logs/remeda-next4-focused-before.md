# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `4`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_isEmptyish_test`: `failed` - `test result: FAILED. 18 passed; 13 failed; 0 ignored; 0 measured; 1758 filtered out; finished in 0.00s`

```text
    __smelt_module_isEmptyish_test::test_arrays_array_like_e_g_arguments
    __smelt_module_isEmptyish_test::test_arrays_buffers
    __smelt_module_isEmptyish_test::test_arrays_sets
    __smelt_module_isEmptyish_test::test_keyed_collections_prototype_chains
    __smelt_module_isEmptyish_test::test_keyed_collections_symbol_props
    __smelt_module_isEmptyish_test::test_keyed_collections_url_search_params
    __smelt_module_isEmptyish_test::test_self_declared_sizes_length
    __smelt_module_isEmptyish_test::test_self_declared_sizes_length_has_precedence_over_size
    __smelt_module_isEmptyish_test::test_self_declared_sizes_size
    __smelt_module_isEmptyish_test::test_unsupported_types_always_true_dates
    __smelt_module_isEmptyish_test::test_unsupported_types_always_true_regexp
    __smelt_module_isEmptyish_test::test_unsupported_types_classes
    __smelt_module_isEmptyish_test::test_unsupported_types_errors

test result: FAILED. 18 passed; 13 failed; 0 ignored; 0 measured; 1758 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_dropFirstBy_test`: `failed` - `test result: FAILED. 0 passed; 12 failed; 0 ignored; 0 measured; 1777 filtered out; finished in 0.00s`

```text

failures:
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_clones_the_input_when_needed
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_handles_empty_arrays_gracefully_280
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_handles_negative_numbers_gracefully_281
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_handles_overflowing_numbers_gracefully_282
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_works_279
    __smelt_module_dropFirstBy_test::test_runtime_datafirst_works_with_complex_compare_rules_284
    __smelt_module_dropFirstBy_test::test_runtime_datalast_clones_the_data_when_needed
    __smelt_module_dropFirstBy_test::test_runtime_datalast_handles_empty_arrays_gracefully_286
    __smelt_module_dropFirstBy_test::test_runtime_datalast_handles_negative_numbers_gracefully_287
    __smelt_module_dropFirstBy_test::test_runtime_datalast_handles_overflowing_numbers_gracefully_288
    __smelt_module_dropFirstBy_test::test_runtime_datalast_works_285
    __smelt_module_dropFirstBy_test::test_runtime_datalast_works_with_complex_compare_rules_290

test result: FAILED. 0 passed; 12 failed; 0 ignored; 0 measured; 1777 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_fromKeys_test`: `failed` - `test result: FAILED. 2 passed; 12 failed; 0 ignored; 0 measured; 1775 filtered out; finished in 0.00s`

```text
thread '__smelt_module_fromKeys_test::test_datalast_works_on_regular_arrays' (97812) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_datalast_uses_the_last_value' (97811) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_datalast_works_with_symbols' (97817) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_datalast_works_with_number_keys' (97816) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_uses_the_last_value' (97818) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_works_on_regular_arrays' (97819) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_works_with_a_mix_of_key_types' (97821) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_works_with_duplicates' (97822) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_works_with_symbols' (97824) panicked at src/fromKeys.rs:39:90:
unknown is not null

thread '__smelt_module_fromKeys_test::test_works_with_number_keys' (97823) panicked at src/fromKeys.rs:39:90:
unknown is not null
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_randomBigInt_test`: `failed` - `test result: FAILED. 1 passed; 12 failed; 0 ignored; 0 measured; 1776 filtered out; finished in 6.10s`

```text
thread '__smelt_module_randomBigInt_test::test_crypto_module_polyfill_bigints_with_same_value' (97847) panicked at src/randomBigInt_test.rs:454:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_crypto_module_polyfill_results_are_varied' (97851) panicked at src/randomBigInt_test.rs:614:224:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_bigints_with_same_value' (97846) panicked at src/randomBigInt_test.rs:173:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_huge_bigints' (97854) panicked at src/randomBigInt_test.rs:213:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_crypto_module_polyfill_tiny_ranges_with_huge_numbers' (97852) panicked at src/randomBigInt_test.rs:563:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_crypto_module_polyfill_huge_bigints' (97848) panicked at src/randomBigInt_test.rs:506:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_negative_bigints' (98128) panicked at src/randomBigInt_test.rs:136:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_non_negative_bigints' (98131) panicked at src/randomBigInt_test.rs:89:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_results_are_varied' (98133) panicked at src/randomBigInt_test.rs:297:223:
unknown is not iterable

thread '__smelt_module_randomBigInt_test::test_tiny_ranges_with_huge_numbers' (98141) panicked at src/randomBigInt_test.rs:258:223:
unknown is not iterable
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_concat_test`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`
- `__smelt_module_addProp_test`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`
- `__smelt_module_binarySearchCutoffIndex_test`: `passed` - `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1778 filtered out; finished in 0.00s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `252`

## Summary By Code

1. **warning** `unused_parens` - 114 diagnostics
2. **warning** `unused_mut` - 100 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 100 occurrences
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.32s
```
