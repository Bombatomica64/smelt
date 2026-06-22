# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1720 passed; 69 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.23s`
- Failing tests: `69`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 5 | `__smelt_module_isDeepEqual_test` |
| 5 | `__smelt_module_mergeDeep_test` |
| 4 | `__smelt_module_evolve_test` |
| 4 | `__smelt_module_isShallowEqual_test` |
| 4 | `__smelt_module_isStrictEqual_test` |
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
| 2 | `__smelt_module_when_test` |
| 1 | `__smelt_module_constant_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-full-after-when-evolve-2026-06-22.md`
- Resolved tests: `2`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_constant_test::test_returns_identity_doesn_t_clone`
- `__smelt_module_evolve_test::test_data_first_can_handle_data_that_is_complex_nested_objects`
- `__smelt_module_evolve_test::test_data_first_creates_a_new_object_by_evolving_the_data_according_to_the_transformation_functions`
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
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays`
- `__smelt_module_isStrictEqual_test::test_built_ins_promises_1273`
- `__smelt_module_isStrictEqual_test::test_objects_arrays_1266`
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
- `__smelt_module_when_test::test_datafirst_without_else_passes_extra_args_to_the_functions`
- `__smelt_module_when_test::test_recipes_can_replace_defaultto`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_truncate_to_shorter_first`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_truncate_to_shorter_second`
- `__smelt_module_zipWith_test::test_data_second_with_initial_arg_should_zip_with_predicate`

</details>
