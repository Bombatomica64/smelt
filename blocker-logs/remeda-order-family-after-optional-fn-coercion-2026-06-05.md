# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `1`
- Full suite executed: `true`

## Focused Runs

- `__smelt_module_firstBy_test`: `failed` - `test result: FAILED. 20 passed; 2 failed; 0 ignored; 0 measured; 1767 filtered out; finished in 0.00s`

```text

running 22 tests
..... 5/22
__smelt_module_firstBy_test::test_runtime_datafirst_can_compare_valueofs --- FAILED
............... 21/22
__smelt_module_firstBy_test::test_runtime_datalast_can_compare_valueofs --- FAILED

failures:

failures:
    __smelt_module_firstBy_test::test_runtime_datafirst_can_compare_valueofs
    __smelt_module_firstBy_test::test_runtime_datalast_can_compare_valueofs

test result: FAILED. 20 passed; 2 failed; 0 ignored; 0 measured; 1767 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_dropFirstBy_test`: `passed` - `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1777 filtered out; finished in 0.00s`
- `__smelt_module_nthBy_test`: `failed` - `test result: FAILED. 6 passed; 2 failed; 0 ignored; 0 measured; 1781 filtered out; finished in 0.01s`

```text

running 8 tests
__smelt_module_nthBy_test::test_runtime_datafirst_handles_overflows_gracefully --- FAILED
... 4/8
__smelt_module_nthBy_test::test_runtime_datalast_handles_overflows_gracefully --- FAILED
...
failures:

failures:
    __smelt_module_nthBy_test::test_runtime_datafirst_handles_overflows_gracefully
    __smelt_module_nthBy_test::test_runtime_datalast_handles_overflows_gracefully

test result: FAILED. 6 passed; 2 failed; 0 ignored; 0 measured; 1781 filtered out; finished in 0.01s


thread '__smelt_module_nthBy_test::test_runtime_datafirst_handles_overflows_gracefully' (3853590) panicked at src/nthBy_test.rs:120:49:
optional value was absent after narrowing
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '__smelt_module_nthBy_test::test_runtime_datalast_handles_overflows_gracefully' (3853594) panicked at src/nthBy_test.rs:413:49:
optional value was absent after narrowing
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_flat_test`: `passed` - `test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 1761 filtered out; finished in 0.01s`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1590 passed; 199 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.67s`
- Failing tests: `199`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 16 | `__smelt_module_funnel_lodash_debounce_test` |
| 12 | `__smelt_module_randomBigInt_test` |
| 10 | `__smelt_module_isEmptyish_test` |
| 9 | `__smelt_module_setPath_test` |
| 8 | `__smelt_module_funnel_lodash_debounce_with_cached_value_test` |
| 8 | `__smelt_module_groupByProp_test` |
| 8 | `__smelt_module_isDeepEqual_test` |
| 7 | `__smelt_module_isShallowEqual_test` |
| 7 | `__smelt_module_isStrictEqual_test` |
| 6 | `__smelt_module_evolve_test` |
| 6 | `__smelt_module_mergeDeep_test` |
| 6 | `__smelt_module_split_test` |
| 6 | `__smelt_module_uniqueBy_test` |
| 6 | `__smelt_module_when_test` |
| 5 | `__smelt_module_funnel_lodash_throttle_with_cached_value_test` |
| 5 | `__smelt_module_groupBy_test` |
| 5 | `__smelt_module_randomInteger_test` |
| 4 | `__smelt_module_median_test` |
| 3 | `__smelt_module_funnel_reference_batch_test` |
| 3 | `__smelt_module_intersection_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-order-family-before-2026-06-05.md`
- Resolved tests: `20`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_constant_test::test_can_completely_change_the_type_of_the_pipe`
- `__smelt_module_constant_test::test_returns_identity_doesn_t_clone`
- `__smelt_module_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_196`
- `__smelt_module_evolve_test::test_data_first_accept_function_whose_second_and_subsequent_arguments_are_optional`
- `__smelt_module_evolve_test::test_data_first_can_handle_data_that_is_complex_nested_objects`
- `__smelt_module_evolve_test::test_data_first_creates_a_new_object_by_evolving_the_data_according_to_the_transformation_functions`
- `__smelt_module_evolve_test::test_data_last_accept_function_whose_second_and_subsequent_arguments_are_optional`
- `__smelt_module_evolve_test::test_data_last_can_handle_data_that_is_complex_nested_objects`
- `__smelt_module_evolve_test::test_data_last_creates_a_new_object_by_evolving_the_data_according_to_the_transformation_functions`
- `__smelt_module_filter_test::test_data_last_filter_indexed`
- `__smelt_module_firstBy_test::test_runtime_datafirst_can_compare_valueofs`
- `__smelt_module_firstBy_test::test_runtime_datalast_can_compare_valueofs`
- `__smelt_module_first_test::test_readonly_tuple_with_last`
- `__smelt_module_first_test::test_tuple_with_last_421`
- `__smelt_module_forEach_test::test_datalast_521`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_noop_cancel_and_flush_when_nothing_is_queued_583`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_reset_lastcalled_after_cancelling_581`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_cancelling_delayed_calls_580`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_flushing_delayed_calls_582`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_use_a_default_wait_of_0_578`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_supports_recursive_calls_579`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_apply_default_options`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_cancel_maxdelayed_when_delayed_is_invoked`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_debounce_a_function_568`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_invoke_the_trailing_call_with_the_correct_arguments_and_this_binding_577`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_not_immediately_call_func_when_wait_is_0`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_queue_a_trailing_call_for_subsequent_debounced_calls_after_maxwait`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_leading_option`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_maxwait_option`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_trailing_option`
- `__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_maxwait_in_a_tight_loop`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_features_not_tested_by_lodash_does_nothing_when_neither_leading_nor_trailing_are_enabled_565`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_noop_cancel_and_flush_when_nothing_is_queued_564`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_reset_lastcalled_after_cancelling_562`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_flushing_delayed_calls_563`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_debounce_a_function_558`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_invoke_the_trailing_call_with_the_correct_arguments_and_this_binding_561`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_subsequent_debounced_calls_return_the_last_func_result`
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_subsequent_leading_debounced_calls_return_the_last_func_result`
- `__smelt_module_funnel_lodash_throttle_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_should_support_a_leading_option_586`
- `__smelt_module_funnel_lodash_throttle_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_should_support_a_trailing_option_587`
- `__smelt_module_funnel_lodash_throttle_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_subsequent_calls_should_return_the_result_of_the_first_call`
- `__smelt_module_funnel_lodash_throttle_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_reset_lastcalled_after_cancelling_588`
- `__smelt_module_funnel_lodash_throttle_with_cached_value_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_flushing_delayed_calls_589`
- `__smelt_module_funnel_reference_batch_test::test_showcase_error_handling`
- `__smelt_module_funnel_reference_batch_test::test_showcase_results_as_array`
- `__smelt_module_funnel_reference_batch_test::test_showcase_results_as_object`
- `__smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_634`
- `__smelt_module_groupByProp_test::test_data_first_must_be_grouped_correctly_by_number`
- `__smelt_module_groupByProp_test::test_data_first_must_be_grouped_correctly_by_string`
- `__smelt_module_groupByProp_test::test_data_first_must_be_grouped_correctly_by_symbol`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_number`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_string`
- `__smelt_module_groupByProp_test::test_data_last_must_be_grouped_correctly_by_symbol`
- `__smelt_module_groupByProp_test::test_grouping_value_is_a_key_of_object_prototype_issue_1049`
- `__smelt_module_groupByProp_test::test_handles_undefined_as_optional_elements`
- `__smelt_module_groupBy_test::test_data_first_groupby`
- `__smelt_module_groupBy_test::test_data_last_groupby`
- `__smelt_module_groupBy_test::test_filtering_on_undefined_grouper_result_regular`
- `__smelt_module_groupBy_test::test_filtering_on_undefined_grouper_result_regular_indexed`
- `__smelt_module_groupBy_test::test_key_is_an_object_instance_method_name_groupby`
- `__smelt_module_hasProp_test::test_arrays_returns_true_for_the_array_length_own_property`
- `__smelt_module_intersection_test::test_maintains_multi_set_semantics_returns_as_many_copies_as_available`
- `__smelt_module_intersection_test::test_maintains_multi_set_semantics_returns_only_one_copy`
- `__smelt_module_intersection_test::test_maintains_order_for_multiple_copies`
- `__smelt_module_isDeepEqual_test::test_functions_same_function_is_equal`
- `__smelt_module_isDeepEqual_test::test_null_prototype_objects_objects_with_different_non_null_prototypes_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_objects_empty_array_and_empty_object_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_objects_null_and_undefined_are_not_equal`
- `__smelt_module_isDeepEqual_test::test_sample_objects_big_object`
- `__smelt_module_isDeepEqual_test::test_sets_two_sets_of_the_same_lenght_should_not_be_equal`
- `__smelt_module_isDeepEqual_test::test_sets_two_sets_of_with_different_primitive_content_should_not_be_equal`
- `__smelt_module_isDeepEqual_test::test_sets_two_sets_with_duplicated_non_primitive_content_should_not_be_equal`
- `__smelt_module_isDefined_test::test_should_work_as_type_guard_in_filter_1146`
- `__smelt_module_isEmpty_test::test_returns_false_for_a_non_empty_string`
- `__smelt_module_isEmptyish_test::test_arrays_array_like_e_g_arguments`
- `__smelt_module_isEmptyish_test::test_arrays_buffers`
- `__smelt_module_isEmptyish_test::test_arrays_sets`
- `__smelt_module_isEmptyish_test::test_keyed_collections_prototype_chains`
- `__smelt_module_isEmptyish_test::test_keyed_collections_symbol_props`
- `__smelt_module_isEmptyish_test::test_keyed_collections_url_search_params`
- `__smelt_module_isEmptyish_test::test_unsupported_types_always_true_dates`
- `__smelt_module_isEmptyish_test::test_unsupported_types_always_true_regexp`
- `__smelt_module_isEmptyish_test::test_unsupported_types_classes`
- `__smelt_module_isEmptyish_test::test_unsupported_types_errors`
- `__smelt_module_isError_test::test_should_work_as_type_guard_1190`
- `__smelt_module_isIncludedIn_test::test_datafirst_only_tests_reference_equality_arrays`
- `__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_arrays`
- `__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_objects`
- `__smelt_module_isNonNull_test::test_should_work_as_type_guard_in_filter_1215`
- `__smelt_module_isNonNullish_test::test_should_work_as_type_guard_in_filter_1217`
- `__smelt_module_isNot_test::test_should_work_as_type_guard_in_filter_1219`
- `__smelt_module_isObjectType_test::test_accepts_null_prototypes_1231`
- `__smelt_module_isPlainObject_test::test_accepts_null_prototypes_1238`
- `__smelt_module_isPlainObject_test::test_rejects_arrays`
- `__smelt_module_isPlainObject_test::test_rejects_classes`
- `__smelt_module_isPromise_test::test_should_work_as_type_guard_1240`
- `__smelt_module_isShallowEqual_test::test_built_ins_dates_1254`
- `__smelt_module_isShallowEqual_test::test_built_ins_regex_1253`
- `__smelt_module_isShallowEqual_test::test_objects_sets_1252`
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
- `__smelt_module_last_test::test_data_first_empty_array_1298`
- `__smelt_module_last_test::test_data_last_empty_array_1300`
- `__smelt_module_length_test::test_curried_iterable`
- `__smelt_module_length_test::test_data_first_iterable`
- `__smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator`
- `__smelt_module_median_test::test_datafirst_arrays_of_even_length`
- `__smelt_module_median_test::test_datafirst_arrays_of_odd_length`
- `__smelt_module_median_test::test_datalast_arrays_of_even_length`
- `__smelt_module_median_test::test_datalast_arrays_of_odd_length`
- `__smelt_module_mergeAll_test::test_merge_objects`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_doesn_t_recurse_into_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_doesn_t_spread_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_not_merge_array_and_object`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_not_merge_arrays`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_not_merge_object_and_array`
- `__smelt_module_mergeDeep_test::test_runtime_datafirst_should_work_with_weird_object_types_functions`
- `__smelt_module_nthBy_test::test_runtime_datafirst_handles_overflows_gracefully`
- `__smelt_module_nthBy_test::test_runtime_datalast_handles_overflows_gracefully`
- `__smelt_module_omitBy_test::test_symbols_are_passed_through`
- `__smelt_module_omit_test::test_datafirst_1384`
- `__smelt_module_omit_test::test_datalast_1386`
- `__smelt_module_pickBy_test::test_symbols_are_filtered_out_1432`
- `__smelt_module_pullObject_test::test_datalast_undefined_values`
- `__smelt_module_randomBigInt_test::test_bigints_with_same_value`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_bigints_with_same_value`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_huge_bigints`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_negative_bigints`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_non_negative_bigints`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_results_are_varied`
- `__smelt_module_randomBigInt_test::test_crypto_module_polyfill_tiny_ranges_with_huge_numbers`
- `__smelt_module_randomBigInt_test::test_huge_bigints`
- `__smelt_module_randomBigInt_test::test_negative_bigints`
- `__smelt_module_randomBigInt_test::test_non_negative_bigints`
- `__smelt_module_randomBigInt_test::test_results_are_varied`
- `__smelt_module_randomBigInt_test::test_tiny_ranges_with_huge_numbers`
- `__smelt_module_randomInteger_test::test_integers_with_same_value`
- `__smelt_module_randomInteger_test::test_negative_decimals`
- `__smelt_module_randomInteger_test::test_negative_integers`
- `__smelt_module_randomInteger_test::test_non_negative_integers`
- `__smelt_module_randomInteger_test::test_positive_decimals`
- `__smelt_module_reduce_test::test_data_first_indexed_1550`
- `__smelt_module_reduce_test::test_data_first_reduce`
- `__smelt_module_reduce_test::test_data_last_reduce`
- `__smelt_module_setPath_test::test_data_first_should_combo_well_with_stringtopath`
- `__smelt_module_setPath_test::test_data_first_should_set_a_deeply_nested_value`
- `__smelt_module_setPath_test::test_data_first_should_support_partial_paths`
- `__smelt_module_setPath_test::test_data_first_should_work_nested_arrays`
- `__smelt_module_setPath_test::test_data_first_should_work_with_undefined_optional_types`
- `__smelt_module_setPath_test::test_data_last_should_set_a_deeply_nested_value`
- `__smelt_module_setPath_test::test_data_last_should_support_partial_paths`
- `__smelt_module_setPath_test::test_data_last_should_work_nested_arrays`
- `__smelt_module_setPath_test::test_data_last_should_work_with_undefined_optional_types`
- `__smelt_module_shuffle_test::test_data_first_1603`
- `__smelt_module_shuffle_test::test_data_last_1604`
- `__smelt_module_sortBy_test::test_data_first_sort_dates_correctly`
- `__smelt_module_sortBy_test::test_data_last_sort_correctly_using_pipe_and_desc`
- `__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc`
- `__smelt_module_sort_test::test_data_first_sort`
- `__smelt_module_sort_test::test_data_last_sort`
- `__smelt_module_splitAt_test::test_data_first_split`
- `__smelt_module_splitAt_test::test_data_first_split_at_1`
- `__smelt_module_splitWhen_test::test_should_split_array`
- `__smelt_module_splitWhen_test::test_should_with_no_matches`
- `__smelt_module_split_test::test_datalast_limited_split`
- `__smelt_module_split_test::test_datalast_regex_with_limit`
- `__smelt_module_split_test::test_empty_string_empty_separator`
- `__smelt_module_split_test::test_empty_string_separator`
- `__smelt_module_split_test::test_multiple_types_of_separators`
- `__smelt_module_split_test::test_negative_limit`
- `__smelt_module_tap_test::test_data_first_should_return_input_value`
- `__smelt_module_tap_test::test_data_last_should_infer_types_after_tapping_function_reference_with_parameter_type_any`
- `__smelt_module_tap_test::test_data_last_should_return_input_value`
- `__smelt_module_uniqueBy_test::test_handles_uniq_by_identity`
- `__smelt_module_uniqueBy_test::test_pipe_get_executed_3_times_when_take_before_uniqueby`
- `__smelt_module_uniqueBy_test::test_pipe_gets_executed_until_target_length_is_reached`
- `__smelt_module_uniqueBy_test::test_returns_people_with_uniq_ages`
- `__smelt_module_uniqueBy_test::test_returns_people_with_uniq_first_letter_of_name`
- `__smelt_module_uniqueBy_test::test_returns_people_with_uniq_names`
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

1. **warning** `unused_mut` - 133 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 133 occurrences
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
4. **warning** `unused_parens` - 23 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:116`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:98`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:97`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:79`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
7. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
8. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:37`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
16. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.11s
```
