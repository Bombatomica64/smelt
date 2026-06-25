# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `5`
- Guard runs: `1`
- Full suite executed: `true`

## Focused Runs

- `isDeepEqual`: `passed` - `test result: ok. 73 passed; 0 failed; 0 ignored; 0 measured; 1716 filtered out; finished in 0.01s`
- `isDefined`: `failed` - `test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`

```text

running 2 tests
. 1/2
__smelt_module_isDefined_test::test_should_work_as_type_guard_in_filter_1146 --- FAILED

failures:

failures:
    __smelt_module_isDefined_test::test_should_work_as_type_guard_in_filter_1146

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `isNonNull`: `failed` - `test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 1785 filtered out; finished in 0.00s`

```text

running 4 tests
... 3/4
__smelt_module_isNonNull_test::test_should_work_as_type_guard_in_filter_1215 --- FAILED

failures:

failures:
    __smelt_module_isNonNull_test::test_should_work_as_type_guard_in_filter_1215

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 1785 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toHaveLength(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `isNonNullish`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`
- `pullObject`: `failed` - `test result: FAILED. 7 passed; 9 failed; 0 ignored; 0 measured; 1773 filtered out; finished in 0.00s`

```text
    __smelt_module_pullObject_test::test_datalast_string_items
    __smelt_module_pullObject_test::test_datalast_undefined_values

test result: FAILED. 7 passed; 9 failed; 0 ignored; 0 measured; 1773 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }

thread '__smelt_module_pullObject_test::test_datalast_guaranteed_to_run_on_each_item' (228240) panicked at src/pipe.rs:296:758:
unknown is not iterable
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '__smelt_module_pullObject_test::test_datalast_number_items' (228242) panicked at src/pipe.rs:296:758:
unknown is not iterable

thread '__smelt_module_pullObject_test::test_datalast_last_value_wins' (228241) panicked at src/pipe.rs:296:758:
unknown is not iterable

thread '__smelt_module_pullObject_test::test_datalast_string_items' (228245) panicked at src/pipe.rs:296:758:
unknown is not iterable

thread '__smelt_module_pullObject_test::test_datalast_object_items' (228244) panicked at src/pipe.rs:296:758:
unknown is not iterable

thread '__smelt_module_pullObject_test::test_datalast_number_keys' (228243) panicked at src/pipe.rs:296:758:
unknown is not iterable

thread '__smelt_module_pullObject_test::test_datalast_undefined_values' (228246) panicked at src/pipe.rs:296:758:
unknown is not iterable
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `isNot`: `failed` - `test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`

```text

running 2 tests
. 1/2
__smelt_module_isNot_test::test_should_work_as_type_guard_in_filter_1219 --- FAILED

failures:

failures:
    __smelt_module_isNot_test::test_should_work_as_type_guard_in_filter_1219

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1576 passed; 213 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.26s`
- Failing tests: `213`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 11 | `__smelt_module_firstBy_test` |
| 9 | `__smelt_module_pullObject_test` |
| 7 | `__smelt_module_countBy_test` |
| 7 | `__smelt_module_fromKeys_test` |
| 6 | `__smelt_module_toKebabCase_test` |
| 6 | `__smelt_module_toSnakeCase_test` |
| 5 | `__smelt_module_dropFirstBy_test` |
| 5 | `__smelt_module_dropWhile_test` |
| 5 | `__smelt_module_split_test` |
| 5 | `__smelt_module_takeFirstBy_test` |
| 5 | `__smelt_module_tap_test` |
| 4 | `__smelt_module_capitalize_test` |
| 4 | `__smelt_module_dropLastWhile_test` |
| 4 | `__smelt_module_first_test` |
| 4 | `__smelt_module_mapWithFeedback_test` |
| 4 | `__smelt_module_nthBy_test` |
| 4 | `__smelt_module_prop_test` |
| 4 | `__smelt_module_sortBy_test` |
| 4 | `__smelt_module_sumBy_test` |
| 4 | `__smelt_module_takeLastWhile_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-after-undefined-phase1-2026-06-23.md`
- Resolved tests: `2`
- Newly failing tests: `194`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_capitalize_test::test_data_last_empty_string_30`
- `__smelt_module_capitalize_test::test_data_last_on_lower_case_31`
- `__smelt_module_capitalize_test::test_data_last_on_mixed_case_33`
- `__smelt_module_capitalize_test::test_data_last_on_upper_case_32`
- `__smelt_module_concat_test::test_data_last_concat`
- `__smelt_module_conditional_test::test_runtime_datafirst_accepts_and_runs_a_default_fallback_case`
- `__smelt_module_conditional_test::test_runtime_datalast_should_return_value_of_first_pair`
- `__smelt_module_constant_test::test_can_be_put_in_a_pipe_144`
- `__smelt_module_constant_test::test_can_completely_change_the_type_of_the_pipe`
- `__smelt_module_constant_test::test_returns_identity_doesn_t_clone`
- `__smelt_module_countBy_test::test_datalast_array_of_objects`
- `__smelt_module_countBy_test::test_datalast_array_of_strings`
- `__smelt_module_countBy_test::test_datalast_countby`
- `__smelt_module_countBy_test::test_datalast_indexed`
- `__smelt_module_countBy_test::test_datalast_mixed_data_types`
- `__smelt_module_countBy_test::test_datalast_symbols`
- `__smelt_module_countBy_test::test_skip_items`
- `__smelt_module_defaultTo_test::test_undefined_fallback`
- `__smelt_module_differenceWith_test::test_data_last_lazy_244`
- `__smelt_module_differenceWith_test::test_data_last_should_allow_differencing_different_data_types`
- `__smelt_module_difference_test::test_lazy`
- `__smelt_module_dropFirstBy_test::test_runtime_datalast_clones_the_data_when_needed`
- `__smelt_module_dropFirstBy_test::test_runtime_datalast_handles_negative_numbers_gracefully_287`
- `__smelt_module_dropFirstBy_test::test_runtime_datalast_handles_overflowing_numbers_gracefully_288`
- `__smelt_module_dropFirstBy_test::test_runtime_datalast_works_285`
- `__smelt_module_dropFirstBy_test::test_runtime_datalast_works_with_complex_compare_rules_290`
- `__smelt_module_dropLastWhile_test::test_data_last_should_return_a_copy_of_the_array_when_the_last_item_fails_the_predicate`
- `__smelt_module_dropLastWhile_test::test_data_last_should_return_an_empty_array_when_all_items_pass_the_predicate_308`
- `__smelt_module_dropLastWhile_test::test_data_last_should_return_first_item_when_first_item_fails_the_predicate`
- `__smelt_module_dropLastWhile_test::test_data_last_should_return_items_until_the_last_predicate_failure`
- `__smelt_module_dropWhile_test::test_data_last_should_return_a_copy_of_the_array_when_the_first_item_fails_the_predicate`
- `__smelt_module_dropWhile_test::test_data_last_should_return_an_empty_array_when_all_items_pass_the_predicate_320`
- `__smelt_module_dropWhile_test::test_data_last_should_return_an_empty_array_when_an_empty_array_is_passed_321`
- `__smelt_module_dropWhile_test::test_data_last_should_return_items_starting_from_the_first_predicate_failure`
- `__smelt_module_dropWhile_test::test_data_last_should_return_last_item_when_last_item_fails_the_predicate`
- `__smelt_module_endsWith_test::test_data_last_330`
- `__smelt_module_filter_test::test_data_last_filter_indexed`
- `__smelt_module_findIndex_test::test_data_last_found_385`
- `__smelt_module_findIndex_test::test_data_last_not_found_386`
- `__smelt_module_findLastIndex_test::test_data_last_found_400`
- `__smelt_module_findLastIndex_test::test_data_last_not_found_401`
- `__smelt_module_findLast_test::test_data_last_findlast`
- `__smelt_module_findLast_test::test_data_last_indexed_394`
- `__smelt_module_firstBy_test::test_runtime_datalast_breaks_ties_with_multiple_order_rules`
- `__smelt_module_firstBy_test::test_runtime_datalast_can_compare_booleans`
- `__smelt_module_firstBy_test::test_runtime_datalast_can_compare_numbers`
- `__smelt_module_firstBy_test::test_runtime_datalast_can_compare_strings`
- `__smelt_module_firstBy_test::test_runtime_datalast_can_compare_valueofs`
- `__smelt_module_firstBy_test::test_runtime_datalast_finds_the_max_with_desc_order_rules`
- `__smelt_module_firstBy_test::test_runtime_datalast_finds_the_max_with_non_trivial_desc_order_rules`
- `__smelt_module_firstBy_test::test_runtime_datalast_finds_the_minimum`
- `__smelt_module_firstBy_test::test_runtime_datalast_finds_the_minimum_with_a_non_trivial_order_rule`
- `__smelt_module_firstBy_test::test_runtime_datalast_returns_the_item_on_a_single_item_array`
- `__smelt_module_firstBy_test::test_runtime_datalast_returns_undefined_on_empty`
- `__smelt_module_first_test::test_pipe_2_x_first`
- `__smelt_module_first_test::test_pipe_as_fn`
- `__smelt_module_first_test::test_pipe_complex`
- `__smelt_module_first_test::test_pipe_with_filter`
- `__smelt_module_flatMap_test::test_datalast_pipe_with_find`
- `__smelt_module_flat_test::test_legacy_flatten_equivalent_depth_1_datalast_lazy`
- `__smelt_module_flat_test::test_legacy_flattendeep_equivalent_depth_4_lazy`
- `__smelt_module_forEach_test::test_datalast_521`
- `__smelt_module_fromEntries_test::test_datalast_532`
- `__smelt_module_fromKeys_test::test_datalast_uses_the_last_value`
- `__smelt_module_fromKeys_test::test_datalast_works_on_regular_arrays`
- `__smelt_module_fromKeys_test::test_datalast_works_on_trivially_empty_arrays`
- `__smelt_module_fromKeys_test::test_datalast_works_with_a_mix_of_key_types`
- `__smelt_module_fromKeys_test::test_datalast_works_with_duplicates`
- `__smelt_module_fromKeys_test::test_datalast_works_with_number_keys`
- `__smelt_module_fromKeys_test::test_datalast_works_with_symbols`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_number`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_string`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_symbol`
- `__smelt_module_groupBy_test::test_data_last_groupby`
- `__smelt_module_identity_test::test_can_be_put_in_a_pipe_734`
- `__smelt_module_identity_test::test_works_with_more_than_one_argument_732`
- `__smelt_module_indexBy_test::test_datalast_916`
- `__smelt_module_intersectionWith_test::test_data_last_checks_if_items_are_equal_based_on_remeda_s_imported_util_function_as_a_comparator`
- `__smelt_module_intersectionWith_test::test_data_last_evaluates_lazily_1047`
- `__smelt_module_intersection_test::test_piping_lazy`
- `__smelt_module_invert_test::test_data_last_numeric_values`
- `__smelt_module_isDefined_test::test_should_work_as_type_guard_in_filter_1146`
- `__smelt_module_isEmptyish_test::test_nullish_undefined`
- `__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_arrays`
- `__smelt_module_isIncludedIn_test::test_datalast_works_with_strings`
- `__smelt_module_isNonNull_test::test_should_work_as_type_guard_in_filter_1215`
- `__smelt_module_isNot_test::test_should_work_as_type_guard_in_filter_1219`
- `__smelt_module_isPromise_test::test_should_work_as_type_guard_1240`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays`
- `__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays`
- `__smelt_module_isStrictEqual_test::test_built_ins_promises_1273`
- `__smelt_module_last_test::test_data_last_empty_array_1300`
- `__smelt_module_last_test::test_data_last_should_return_last`
- `__smelt_module_length_test::test_curried_array`
- `__smelt_module_length_test::test_curried_iterable`
- `__smelt_module_mapToObj_test::test_data_last_indexed_1327`
- `__smelt_module_mapToObj_test::test_data_last_maptoobj`
- `__smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator`
- `__smelt_module_mapWithFeedback_test::test_data_last_evaluates_lazily_1338`
- `__smelt_module_mapWithFeedback_test::test_data_last_should_return_an_array_of_successively_accumulated_values`
- `__smelt_module_mapWithFeedback_test::test_data_last_should_track_index_and_progressively_include_elements_from_the_original_array_in_the_items_array_during_each_iteration_forming_a_growing_window`
- `__smelt_module_meanBy_test::test_data_last_indexed_1348`
- `__smelt_module_meanBy_test::test_data_last_meanby`
- `__smelt_module_mean_test::test_datalast_should_return_the_mean_of_numbers_in_an_array`
- `__smelt_module_mean_test::test_datalast_should_return_undefined_for_an_empty_array_1343`
- `__smelt_module_median_test::test_datalast_arrays_of_even_length`
- `__smelt_module_median_test::test_datalast_arrays_of_odd_length`
- `__smelt_module_median_test::test_datalast_should_return_undefined_for_an_empty_array_1355`
- `__smelt_module_mergeAll_test::test_merge_objects`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_work_with_weird_object_types_functions`
- `__smelt_module_nthBy_test::test_runtime_datalast_handles_negative_indexes`
- `__smelt_module_nthBy_test::test_runtime_datalast_handles_overflows_gracefully`
- `__smelt_module_nthBy_test::test_runtime_datalast_works_1378`
- `__smelt_module_nthBy_test::test_runtime_datalast_works_with_complex_order_rules`
- `__smelt_module_only_test::test_data_last_empty_array_1398`
- `__smelt_module_only_test::test_data_last_length_1_array`
- `__smelt_module_only_test::test_data_last_length_2_array`
- `__smelt_module_partition_test::test_data_first_partition_with_type_guard_in_pipe`
- `__smelt_module_partition_test::test_data_last_indexed_1416`
- `__smelt_module_partition_test::test_data_last_partition`
- `__smelt_module_pipe_test::test_lazy_break_lazy`
- `__smelt_module_pipe_test::test_lazy_multiple_lazy`
- `__smelt_module_product_test::test_datalast_should_return_1_for_an_empty_array`
- `__smelt_module_product_test::test_datalast_should_return_the_product_of_numbers_in_the_array`
- `__smelt_module_prop_test::test_lodash_spec_should_return_undefined_for_deep_paths_when_object_is_nullish`
- `__smelt_module_prop_test::test_lodash_spec_should_return_undefined_if_parts_of_path_are_missing`
- `__smelt_module_prop_test::test_lodash_spec_should_return_undefined_when_object_is_nullish`
- `__smelt_module_prop_test::test_stops_at_optional_props`
- `__smelt_module_pullObject_test::test_datafirst_undefined_values`
- `__smelt_module_pullObject_test::test_datalast_empty_array`
- `__smelt_module_pullObject_test::test_datalast_guaranteed_to_run_on_each_item`
- `__smelt_module_pullObject_test::test_datalast_last_value_wins`
- `__smelt_module_pullObject_test::test_datalast_number_items`
- `__smelt_module_pullObject_test::test_datalast_number_keys`
- `__smelt_module_pullObject_test::test_datalast_object_items`
- `__smelt_module_pullObject_test::test_datalast_string_items`
- `__smelt_module_pullObject_test::test_datalast_undefined_values`
- `__smelt_module_reduce_test::test_data_first_indexed_1550`
- `__smelt_module_reduce_test::test_data_last_reduce`
- `__smelt_module_reverse_test::test_data_last_reverse`
- `__smelt_module_shuffle_test::test_data_last_1604`
- `__smelt_module_sortBy_test::test_data_last_sort_correctly`
- `__smelt_module_sortBy_test::test_data_last_sort_correctly_using_pipe_and_desc`
- `__smelt_module_sortBy_test::test_data_last_sort_objects_correctly`
- `__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc`
- `__smelt_module_sort_test::test_data_last_sort`
- `__smelt_module_splice_test::test_a_purried_data_last_implementation`
- `__smelt_module_split_test::test_datalast_limited_split`
- `__smelt_module_split_test::test_datalast_regex_split`
- `__smelt_module_split_test::test_datalast_regex_with_limit`
- `__smelt_module_split_test::test_datalast_undefined_limit`
- `__smelt_module_split_test::test_datalast_useful_split`
- `__smelt_module_startsWith_test::test_data_last_1796`
- `__smelt_module_sumBy_test::test_data_last_indexed_1861`
- `__smelt_module_sumBy_test::test_data_last_should_return_0_for_an_empty_array`
- `__smelt_module_sumBy_test::test_data_last_sumby`
- `__smelt_module_sumBy_test::test_data_last_works_with_bigint`
- `__smelt_module_sum_test::test_datalast_should_return_0_for_an_empty_array`
- `__smelt_module_sum_test::test_datalast_should_return_the_sum_of_numbers_in_an_array`
- `__smelt_module_takeFirstBy_test::test_runtime_datalast_clones_the_array_when_needed`
- `__smelt_module_takeFirstBy_test::test_runtime_datalast_handles_negative_numbers_gracefully_1889`
- `__smelt_module_takeFirstBy_test::test_runtime_datalast_handles_overflowing_numbers_gracefully_1890`
- `__smelt_module_takeFirstBy_test::test_runtime_datalast_works_1887`
- `__smelt_module_takeFirstBy_test::test_runtime_datalast_works_with_complex_compare_rules_1892`
- `__smelt_module_takeLastWhile_test::test_data_last_should_return_a_copy_of_the_original_array_when_all_items_pass_the_predicate`
- `__smelt_module_takeLastWhile_test::test_data_last_should_return_an_empty_array_when_the_last_item_fails_the_predicate`
- `__smelt_module_takeLastWhile_test::test_data_last_should_return_items_after_the_last_predicate_failure`
- `__smelt_module_takeLastWhile_test::test_data_last_should_return_rest_of_the_items_when_first_item_fails_the_predicate`
- `__smelt_module_takeWhile_test::test_data_last_takewhile`
- `__smelt_module_tap_test::test_data_first_should_return_input_value`
- `__smelt_module_tap_test::test_data_last_should_call_function_with_input_value`
- `__smelt_module_tap_test::test_data_last_should_infer_types_after_tapping_function_reference_with_parameter_type_any`
- `__smelt_module_tap_test::test_data_last_should_return_input_value`
- `__smelt_module_tap_test::test_data_last_should_work_in_the_middle_of_pipe_sequence`
- `__smelt_module_toCamelCase_test::test_data_last_with_options_preserveconsecutiveuppercase_false`
- `__smelt_module_toCamelCase_test::test_data_last_with_options_preserveconsecutiveuppercase_true`
- `__smelt_module_toCamelCase_test::test_data_last_without_options`
- `__smelt_module_toKebabCase_test::test_data_last_on_camel_case_1962`
- `__smelt_module_toKebabCase_test::test_data_last_on_kebab_case_1963`
- `__smelt_module_toKebabCase_test::test_data_last_on_lower_case_1958`
- `__smelt_module_toKebabCase_test::test_data_last_on_mixed_case_1960`
- `__smelt_module_toKebabCase_test::test_data_last_on_snake_case_1961`
- `__smelt_module_toKebabCase_test::test_data_last_on_upper_case_1959`
- `__smelt_module_toLowerCase_test::test_data_last_on_lower_case_1988`
- `__smelt_module_toLowerCase_test::test_data_last_on_mixed_case_1990`
- `__smelt_module_toLowerCase_test::test_data_last_on_upper_case_1989`
- `__smelt_module_toSnakeCase_test::test_data_last_on_camel_case_2009`
- `__smelt_module_toSnakeCase_test::test_data_last_on_kebab_case_2010`
- `__smelt_module_toSnakeCase_test::test_data_last_on_lower_case_2005`
- `__smelt_module_toSnakeCase_test::test_data_last_on_mixed_case_2007`
- `__smelt_module_toSnakeCase_test::test_data_last_on_snake_case_2008`
- `__smelt_module_toSnakeCase_test::test_data_last_on_upper_case_2006`
- `__smelt_module_toTitleCase_test::test_data_last_2043`
- `__smelt_module_toTitleCase_test::test_preserveconsecutiveuppercase_option_data_last`
- `__smelt_module_toUpperCase_test::test_data_last_on_lower_case_2063`
- `__smelt_module_toUpperCase_test::test_data_last_on_mixed_case_2065`
- `__smelt_module_toUpperCase_test::test_data_last_on_upper_case_2064`
- `__smelt_module_truncate_test::test_data_last_accepts_an_options_object`
- `__smelt_module_truncate_test::test_data_last_has_an_implicit_default_options_object`
- `__smelt_module_uncapitalize_test::test_data_last_empty_string_2101`
- `__smelt_module_uncapitalize_test::test_data_last_on_lower_case_2102`
- `__smelt_module_uncapitalize_test::test_data_last_on_mixed_case_2104`
- `__smelt_module_uncapitalize_test::test_data_last_on_upper_case_2103`
- `__smelt_module_uniqueBy_test::test_pipe_get_executed_3_times_when_take_before_uniqueby`
- `__smelt_module_uniqueBy_test::test_pipe_gets_executed_until_target_length_is_reached`
- `__smelt_module_uniqueWith_test::test_data_last_lazy_2124`
- `__smelt_module_uniqueWith_test::test_data_last_take_before_uniq`
- `__smelt_module_unique_test::test_pipe_take_before_unique`
- `__smelt_module_unique_test::test_pipe_unique`
- `__smelt_module_when_test::test_datalast_with_else_returns_the_else_path_when_false`
- `__smelt_module_when_test::test_datalast_with_else_returns_the_happy_path_when_true`
- `__smelt_module_when_test::test_datalast_without_else_returns_the_happy_path_when_true`
- `__smelt_module_when_test::test_datalast_without_else_returns_the_identity_when_false`

</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `274`

## Summary By Code

1. **warning** `unused_mut` - 140 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic
6. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 140 occurrences
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
6. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:108`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/isShallowEqual.rs:259`
12. **warning** `unused_must_use` - 1 occurrence
   - Message: unused `Result` that must be used
   - Examples:
     - `src/funnel_reference_batch_test.rs:159`
13. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.16s
```
