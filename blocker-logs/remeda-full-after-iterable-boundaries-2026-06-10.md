# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1709 passed; 80 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.05s`
- Failing tests: `80`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 7 | `__smelt_module_isStrictEqual_test` |
| 6 | `__smelt_module_evolve_test` |
| 6 | `__smelt_module_isShallowEqual_test` |
| 6 | `__smelt_module_when_test` |
| 5 | `__smelt_module_isDeepEqual_test` |
| 5 | `__smelt_module_mergeDeep_test` |
| 4 | `__smelt_module_omit_test` |
| 3 | `__smelt_module_intersection_test` |
| 3 | `__smelt_module_isIncludedIn_test` |
| 3 | `__smelt_module_reduce_test` |
| 3 | `__smelt_module_tap_test` |
| 3 | `__smelt_module_zipWith_test` |
| 2 | `__smelt_module_first_test` |
| 2 | `__smelt_module_isPlainObject_test` |
| 2 | `__smelt_module_shuffle_test` |
| 2 | `__smelt_module_sortBy_test` |
| 2 | `__smelt_module_sort_test` |
| 2 | `__smelt_module_splitAt_test` |
| 2 | `__smelt_module_splitWhen_test` |
| 1 | `__smelt_module_constant_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-full-after-erased-array-sort-2026-06-10.md`
- Resolved tests: `4`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_constant_test::test_returns_identity_doesn_t_clone`
- `__smelt_module_evolve_test::test_data_first_accept_function_whose_second_and_subsequent_arguments_are_optional`
- `__smelt_module_evolve_test::test_data_first_can_handle_data_that_is_complex_nested_objects`
- `__smelt_module_evolve_test::test_data_first_creates_a_new_object_by_evolving_the_data_according_to_the_transformation_functions`
- `__smelt_module_evolve_test::test_data_last_accept_function_whose_second_and_subsequent_arguments_are_optional`
- `__smelt_module_evolve_test::test_data_last_can_handle_data_that_is_complex_nested_objects`
- `__smelt_module_evolve_test::test_data_last_creates_a_new_object_by_evolving_the_data_according_to_the_transformation_functions`
- `__smelt_module_filter_test::test_data_last_filter_indexed`
- `__smelt_module_first_test::test_readonly_tuple_with_last`
- `__smelt_module_first_test::test_tuple_with_last_421`
- `__smelt_module_forEach_test::test_datalast_521`
- `__smelt_module_intersection_test::test_maintains_multi_set_semantics_returns_as_many_copies_as_available`
- `__smelt_module_intersection_test::test_maintains_multi_set_semantics_returns_only_one_copy`
- `__smelt_module_intersection_test::test_maintains_order_for_multiple_copies`
- `__smelt_module_isDeepEqual_test::test_functions_same_function_is_equal`
- `__smelt_module_isDeepEqual_test::test_null_prototype_objects_objects_with_different_non_null_prototypes_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_objects_empty_array_and_empty_object_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_objects_null_and_undefined_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_sample_objects_big_object`
- `__smelt_module_isDefined_test::test_should_work_as_type_guard_in_filter_1146`
- `__smelt_module_isIncludedIn_test::test_datafirst_only_tests_reference_equality_arrays`
- `__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_arrays`
- `__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_objects`
- `__smelt_module_isNonNull_test::test_should_work_as_type_guard_in_filter_1215`
- `__smelt_module_isNonNullish_test::test_should_work_as_type_guard_in_filter_1217`
- `__smelt_module_isNot_test::test_should_work_as_type_guard_in_filter_1219`
- `__smelt_module_isPlainObject_test::test_rejects_arrays`
- `__smelt_module_isPlainObject_test::test_rejects_classes`
- `__smelt_module_isPromise_test::test_should_work_as_type_guard_1240`
- `__smelt_module_isShallowEqual_test::test_built_ins_dates_1254`
- `__smelt_module_isShallowEqual_test::test_built_ins_regex_1253`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_objects`
- `__smelt_module_isStrictEqual_test::test_built_ins_dates_1272`
- `__smelt_module_isStrictEqual_test::test_built_ins_promises_1273`
- `__smelt_module_isStrictEqual_test::test_objects_arrays_1266`
- `__smelt_module_isStrictEqual_test::test_objects_maps_1269`
- `__smelt_module_isStrictEqual_test::test_objects_objects_1267`
- `__smelt_module_isStrictEqual_test::test_objects_sets_1270`
- `__smelt_module_isStrictEqual_test::test_objects_uint_arrays_1268`
- `__smelt_module_isSymbol_test::test_should_work_as_type_guard_1279`
- `__smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator`
- `__smelt_module_mergeAll_test::test_merge_objects`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_doesn_t_recurse_into_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_doesn_t_spread_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_not_merge_array_and_object`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_not_merge_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_work_with_weird_object_types_functions`
- `__smelt_module_omit_test::test_can_omit_symbol_keys`
- `__smelt_module_omit_test::test_datafirst_1384`
- `__smelt_module_omit_test::test_datalast_1386`
- `__smelt_module_omit_test::test_single_removed_prop_works`
- `__smelt_module_pullObject_test::test_datalast_undefined_values`
- `__smelt_module_reduce_test::test_data_first_indexed_1550`
- `__smelt_module_reduce_test::test_data_first_reduce`
- `__smelt_module_reduce_test::test_data_last_reduce`
- `__smelt_module_shuffle_test::test_data_first_1603`
- `__smelt_module_shuffle_test::test_data_last_1604`
- `__smelt_module_sortBy_test::test_data_last_sort_correctly_using_pipe_and_desc`
- `__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc`
- `__smelt_module_sort_test::test_data_first_sort`
- `__smelt_module_sort_test::test_data_last_sort`
- `__smelt_module_splitAt_test::test_data_first_split`
- `__smelt_module_splitAt_test::test_data_first_split_at_1`
- `__smelt_module_splitWhen_test::test_should_split_array`
- `__smelt_module_splitWhen_test::test_should_with_no_matches`
- `__smelt_module_tap_test::test_data_first_should_return_input_value`
- `__smelt_module_tap_test::test_data_last_should_infer_types_after_tapping_function_reference_with_parameter_type_any`
- `__smelt_module_tap_test::test_data_last_should_return_input_value`
- `__smelt_module_when_test::test_can_return_other_types`
- `__smelt_module_when_test::test_datafirst_with_else_returns_the_happy_path_when_true`
- `__smelt_module_when_test::test_datafirst_without_else_passes_extra_args_to_the_functions`
- `__smelt_module_when_test::test_datafirst_without_else_returns_the_happy_path_when_true`
- `__smelt_module_when_test::test_datalast_without_else_passes_extra_args_to_the_functions`
- `__smelt_module_when_test::test_recipes_can_replace_defaultto`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_truncate_to_shorter_first`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_truncate_to_shorter_second`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_zip_with_predicate`

</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `271`

## Summary By Code

1. **warning** `unused_mut` - 136 diagnostics
2. **warning** `unused_parens` - 93 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic
6. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 136 occurrences
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
3. **warning** `unused_unsafe` - 31 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
4. **warning** `unused_parens` - 24 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
6. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
7. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
13. **warning** `unused_must_use` - 1 occurrence
   - Message: unused `Result` that must be used
   - Examples:
     - `src/funnel_reference_batch_test.rs:159`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.59s
```
