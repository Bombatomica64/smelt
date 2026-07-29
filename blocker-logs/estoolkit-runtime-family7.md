# Generated Rust Test Report

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 791 passed; 268 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.63s`
- Failing tests: `268`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 23 | `__smelt_module_isEqualWith_spec` |
| 17 | `__smelt_module_cloneDeep_spec` |
| 15 | `__smelt_module_clone_spec` |
| 10 | `__smelt_module_debounce_spec` |
| 10 | `__smelt_module_toMerged_spec` |
| 7 | `__smelt_module_throttle_spec` |
| 6 | `__smelt_module_cloneDeepWith_spec` |
| 6 | `__smelt_module_memoize_spec` |
| 6 | `__smelt_module_pullAt_spec` |
| 5 | `__smelt_module_partialRight_spec` |
| 5 | `__smelt_module_partial_spec` |
| 5 | `__smelt_module_retry_spec` |
| 4 | `__smelt_module_allKeyed_spec` |
| 4 | `__smelt_module_ary_spec` |
| 4 | `__smelt_module_attemptAsync_spec` |
| 4 | `__smelt_module_flow_spec` |
| 4 | `__smelt_module_invariant_spec` |
| 4 | `__smelt_module_mergeWith_spec` |
| 4 | `__smelt_module_rest_spec` |
| 4 | `__smelt_module_trimStart_spec` |

### Delta From Baseline

- Baseline report: `blocker-logs/estoolkit-runtime-family7-before.md`
- Resolved tests: `2`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_AbortError_spec::test_aborterror_uses_the_default_message_when_none_is_provided`
- `__smelt_module_AbortError_spec::test_aborterror_uses_the_provided_message`
- `__smelt_module_AbortError_spec::test_aborterror_when_domexception_is_unavailable_e_g_hermes_loads_without_throwing_and_falls_back_to_error`
- `__smelt_module_TimeoutError_spec::test_timeouterror_uses_the_default_message_when_none_is_provided`
- `__smelt_module_TimeoutError_spec::test_timeouterror_uses_the_provided_message`
- `__smelt_module_TimeoutError_spec::test_timeouterror_when_domexception_is_unavailable_e_g_hermes_loads_without_throwing_and_falls_back_to_error`
- `__smelt_module_allKeyed_spec::test_allkeyed_should_handle_a_mix_of_promises_and_plain_values`
- `__smelt_module_allKeyed_spec::test_allkeyed_should_preserve_key_value_associations`
- `__smelt_module_allKeyed_spec::test_allkeyed_should_reject_if_any_promise_rejects`
- `__smelt_module_allKeyed_spec::test_allkeyed_should_resolve_an_object_of_promises_concurrently`
- `__smelt_module_ary_spec::test_ary_should_cap_the_number_of_arguments_provided_to_func`
- `__smelt_module_ary_spec::test_ary_should_not_force_a_minimum_argument_count`
- `__smelt_module_ary_spec::test_ary_should_use_the_existing_ary_if_smaller`
- `__smelt_module_ary_spec::test_ary_should_use_this_binding_of_function`
- `__smelt_module_at_spec::test_at_should_return_undefined_for_non_integer_indices`
- `__smelt_module_at_spec::test_at_should_return_undefined_for_nonexistent_keys`
- `__smelt_module_attemptAsync_spec::test_attemptasync_should_return_the_error_of_the_async_function`
- `__smelt_module_attemptAsync_spec::test_attemptasync_should_return_the_error_of_the_async_function_that_rejects_after_a_delay`
- `__smelt_module_attemptAsync_spec::test_attemptasync_should_return_the_result_of_a_complex_async_operation`
- `__smelt_module_attemptAsync_spec::test_attemptasync_should_work_with_non_error_thrown_objects`
- `__smelt_module_attempt_spec::test_attempt_should_return_the_error_of_the_function`
- `__smelt_module_attempt_spec::test_attempt_should_return_the_result_of_the_promise`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_allow_customizer_to_handle_nested_objects`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_allow_customizer_to_modify_values`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_allow_customizer_to_replace_values`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_allow_customizer_to_replace_values_with_null`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_deep_clone_objects`
- `__smelt_module_cloneDeepWith_spec::test_clonedeepwith_should_handle_circular_references`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_arguments_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_arraybuffer_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_blob_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_boolean_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_buffers`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_class_instance`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_custom_error`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_file_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_instance`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_maps`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_number_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_read_only_properties`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_regexp_arrays`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_regular_expressions`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_clone_string_objects`
- `__smelt_module_cloneDeep_spec::test_clonedeep_should_deep_clone_nested_objects`
- `__smelt_module_clone_spec::test_clone_should_clone_arraybuffer`
- `__smelt_module_clone_spec::test_clone_should_clone_blob`
- `__smelt_module_clone_spec::test_clone_should_clone_buffers`
- `__smelt_module_clone_spec::test_clone_should_clone_custom_classes`
- `__smelt_module_clone_spec::test_clone_should_clone_custom_error`
- `__smelt_module_clone_spec::test_clone_should_clone_data_views`
- `__smelt_module_clone_spec::test_clone_should_clone_error`
- `__smelt_module_clone_spec::test_clone_should_clone_file`
- `__smelt_module_clone_spec::test_clone_should_clone_maps`
- `__smelt_module_clone_spec::test_clone_should_clone_objects`
- `__smelt_module_clone_spec::test_clone_should_clone_objects_with_a_null_prototype`
- `__smelt_module_clone_spec::test_clone_should_clone_regular_expressions`
- `__smelt_module_clone_spec::test_clone_should_clone_sharedarraybuffer`
- `__smelt_module_clone_spec::test_clone_should_return_functions_as_is`
- `__smelt_module_clone_spec::test_clone_should_shallow_clone_nested_objects`
- `__smelt_module_compact_spec::test_compact_removes_falsey_values_from_array`
- `__smelt_module_debounce_spec::test_debounce_should_call_the_function_with_correct_arguments`
- `__smelt_module_debounce_spec::test_debounce_should_cancel_the_debounced_function_call`
- `__smelt_module_debounce_spec::test_debounce_should_debounce_function_calls`
- `__smelt_module_debounce_spec::test_debounce_should_delay_the_function_call_by_the_specified_wait_time`
- `__smelt_module_debounce_spec::test_debounce_should_have_no_effect_if_we_call_cancel_when_the_function_is_not_executed`
- `__smelt_module_debounce_spec::test_debounce_should_immediately_invoke_the_pending_function_when_flush_is_called`
- `__smelt_module_debounce_spec::test_debounce_should_invoke_the_function_on_the_leading_edge_when_edges_includes_leading`
- `__smelt_module_debounce_spec::test_debounce_should_not_add_multiple_abort_event_listeners`
- `__smelt_module_debounce_spec::test_debounce_should_reset_the_wait_time_if_called_again_before_wait_time_ends`
- `__smelt_module_debounce_spec::test_debounce_should_work_correctly_if_the_debounced_function_is_called_after_the_wait_time`
- `__smelt_module_delay_spec::test_delay_should_cancel_the_delay_if_aborted_via_abortsignal`
- `__smelt_module_delay_spec::test_delay_should_clear_timeout_when_aborted_by_abortsignal`
- `__smelt_module_delay_spec::test_delay_should_not_call_the_delay_if_it_is_already_aborted_by_abortsignal`
- `__smelt_module_dropRightWhile_spec::test_droprightwhile_should_drop_elements_from_an_array_until_cancontinuedropping_returns_false_from_the_end`
- `__smelt_module_dropWhile_spec::test_dropwhile_should_drop_elements_from_an_array_until_cancontinuedropping_returns_false_from_the_beginning`
- `__smelt_module_escapeRegExp_spec::test_escaperegexp_should_escape_values`
- `__smelt_module_escape_spec::test_escape_should_escape_the_same_characters_unescaped_by_unescape`
- `__smelt_module_escape_spec::test_escape_should_escape_values`
- `__smelt_module_fill_spec::test_fill_fills_a_new_array_with_a_specified_value`
- `__smelt_module_filterAsync_spec::test_filterasync_uses_full_concurrency_when_not_specified`
- `__smelt_module_findKey_spec::test_findkey_should_return_the_first_key_if_all_elements_satisfy_the_predicate`
- `__smelt_module_findKey_spec::test_findkey_should_return_the_key_of_the_first_element_that_satisfies_the_predicate`
- `__smelt_module_flatMapAsync_spec::test_flatmapasync_uses_full_concurrency_when_not_specified`
- `__smelt_module_flattenObject_spec::test_flattenobject_handles_typedarray_s_correctly`
- `__smelt_module_flowRight_spec::test_flowright_flowright_should_supply_each_function_with_the_return_value_of_the_previous`
- `__smelt_module_flowRight_spec::test_flowright_flowright_should_work_with_a_curried_function_and_head`
- `__smelt_module_flowRight_spec::test_flowright_flowright_should_work_with_curried_functions_with_placeholders`
- `__smelt_module_flow_spec::test_flow_flow_should_preserve_this_context`
- `__smelt_module_flow_spec::test_flow_flow_should_supply_each_function_with_the_return_value_of_the_previous`
- `__smelt_module_flow_spec::test_flow_flow_should_work_with_a_curried_function_and_head`
- `__smelt_module_flow_spec::test_flow_flow_should_work_with_curried_functions_with_placeholders`
- `__smelt_module_forEachAsync_spec::test_foreachasync_uses_full_concurrency_when_not_specified`
- `__smelt_module_head_spec::test_head_returns_the_first_element_of_an_array_or_undefined_for_empty_arrays`
- `__smelt_module_intersectionWith_spec::test_intersectionwith_should_return_the_intersection_of_two_arrays_with_mapper`
- `__smelt_module_invariant_spec::test_invariant_should_not_throw_an_error_when_the_condition_is_true`
- `__smelt_module_invariant_spec::test_invariant_should_throw_a_custom_error_when_the_condition_is_false_and_the_message_is_an_error`
- `__smelt_module_invariant_spec::test_invariant_should_throw_an_error_when_the_condition_is_false`
- `__smelt_module_invariant_spec::test_invariant_should_throw_an_error_when_the_condition_is_false_and_the_message_is_an_error`
- `__smelt_module_invert_spec::test_invert_should_not_invert_inherited_properties`
- `__smelt_module_isBrowser_spec::test_isbrowser_should_return_true_in_browser_environment`
- `__smelt_module_isBuffer_spec::test_isbuffer_should_return_true_for_buffer_instances`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_avoid_common_type_coercions_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_arguments_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_array_views_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_arrays_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_arrays_with_circular_references_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_buffers_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_date_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_error_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_functions_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_maps_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_maps_with_circular_references_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_nested_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_object_instances_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_objects_with_circular_references_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_objects_with_constructor_properties_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_objects_with_multiple_circular_references_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_primitives_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_sparse_arrays_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_compare_symbol_properties_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_have_transitive_equivalence_for_circular_references_of_arrays_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_treat_arguments_objects_like_object_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_treat_arrays_with_identical_values_but_different_non_index_properties_as_equal_when_customizer_returns_undefined`
- `__smelt_module_isEqualWith_spec::test_isequalwith_should_treat_objects_created_by_object_create_null_like_plain_objects_when_customizer_returns_undefined`
- `__smelt_module_isEqual_spec::test_isequal_should_return_false_for_different_array_buffers`
- `__smelt_module_isEqual_spec::test_isequal_should_return_false_for_different_date_objects`
- `__smelt_module_isError_spec::test_iserror_should_return_true_for_subclassed_values`
- `__smelt_module_isFile_spec::test_isfile_can_be_used_with_typescript_as_a_type_predicate`
- `__smelt_module_isFile_spec::test_isfile_returns_true_if_the_value_is_a_file`
- `__smelt_module_isFunction_spec::test_isfunction_should_return_true_for_functions`
- `__smelt_module_isJSONValue_spec::test_isjsonobject_isjsonobject_should_return_false_for_not_valid_value`
- `__smelt_module_isJSONValue_spec::test_isjsonobject_isjsonobject_should_return_false_when_key_is_not_a_string`
- `__smelt_module_isJSON_spec::test_isjson_returns_false_if_the_value_is_not_a_valid_json_string`
- `__smelt_module_isLength_spec::test_islength_should_return_true_for_lengths`
- `__smelt_module_isNode_spec::test_isnode_should_return_true_in_node_environment`
- `__smelt_module_isNull_spec::test_isnull_can_be_used_with_typescript_as_a_type_predicate`
- `__smelt_module_isPlainObject_spec::test_isplainobject_should_return_false_for_invalid_plain_objects`
- `__smelt_module_isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects`
- `__smelt_module_isRegExp_spec::test_isregexp_returns_true_for_regexp`
- `__smelt_module_isSymbol_spec::test_issymbol_returns_true_for_symbols`
- `__smelt_module_isTypedArray_spec::test_istypedarray_returns_true_for_typed_arrays`
- `__smelt_module_isUndefined_spec::test_isundefined_can_be_used_with_typescript_as_a_type_predicate`
- `__smelt_module_last_spec::test_last_returns_the_last_element_of_a_large_array`
- `__smelt_module_last_spec::test_last_returns_the_last_element_of_an_array_or_undefined_for_empty_array`
- `__smelt_module_limitAsync_spec::test_limitasync_limits_concurrency_of_async_callbacks`
- `__smelt_module_limitAsync_spec::test_limitasync_propagates_callback_errors`
- `__smelt_module_limitAsync_spec::test_limitasync_returns_correct_values_in_correct_order`
- `__smelt_module_mapAsync_spec::test_mapasync_uses_full_concurrency_when_not_specified`
- `__smelt_module_maxBy_spec::test_maxby_if_array_is_empty_return_undefined`
- `__smelt_module_meanBy_spec::test_meanby_returns_nan_for_empty_arrays`
- `__smelt_module_mean_spec::test_mean_returns_nan_for_empty_arrays`
- `__smelt_module_medianBy_spec::test_medianby_returns_nan_for_empty_arrays`
- `__smelt_module_median_spec::test_median_returns_nan_for_empty_arrays`
- `__smelt_module_memoize_spec::test_memoize_should_allow_custom_cache_implementation`
- `__smelt_module_memoize_spec::test_memoize_should_check_cache_for_built_in_properties`
- `__smelt_module_memoize_spec::test_memoize_should_memoize_results_of_an_unary_function`
- `__smelt_module_memoize_spec::test_memoize_should_memoize_results_using_a_custom_resolver_function`
- `__smelt_module_memoize_spec::test_memoize_should_use_this_context_for_resolver_function`
- `__smelt_module_memoize_spec::test_memoize_should_work_with_an_immutable_cache_implementation`
- `__smelt_module_mergeWith_spec::test_mergewith_should_merge_properties_from_source_object_into_target_object_using_custom_merge_function`
- `__smelt_module_mergeWith_spec::test_mergewith_should_respect_null_returned_from_customizer`
- `__smelt_module_mergeWith_spec::test_mergewith_should_skip_unsafe_properties_like_proto`
- `__smelt_module_mergeWith_spec::test_mergewith_should_use_custom_merge_function_for_nested_objects`
- `__smelt_module_merge_spec::test_merge_should_behave_like_recursive_object_assign_applying_the_same_logic_to_nested_properties`
- `__smelt_module_merge_spec::test_merge_should_skip_unsafe_properties_like_proto`
- `__smelt_module_minBy_spec::test_minby_if_array_is_empty_return_undefined`
- `__smelt_module_negate_spec::test_negate_should_negate_the_given_predicate_function`
- `__smelt_module_omitBy_spec::test_omitby_should_omit_properties_based_on_the_predicate_function`
- `__smelt_module_omit_spec::test_omit_should_omit_properties_from_an_object`
- `__smelt_module_omit_spec::test_omit_should_return_an_empty_object_if_all_keys_are_omitted`
- `__smelt_module_once_spec::test_once_should_handle_functions_with_no_return_value`
- `__smelt_module_partialRight_spec::test_partialright_partialright_creates_a_function_with_a_length_of_0`
- `__smelt_module_partialRight_spec::test_partialright_partialright_ensures_new_par_is_an_instance_of_func`
- `__smelt_module_partialRight_spec::test_partialright_partialright_should_work_with_curried_functions`
- `__smelt_module_partialRight_spec::test_partialright_partialright_should_work_with_placeholders_and_curried_functions`
- `__smelt_module_partialRight_spec::test_partialright_partialright_supports_placeholders`
- `__smelt_module_partial_spec::test_partial_partial_creates_a_function_with_a_length_of_0`
- `__smelt_module_partial_spec::test_partial_partial_ensures_new_par_is_an_instance_of_func`
- `__smelt_module_partial_spec::test_partial_partial_should_work_with_curried_functions`
- `__smelt_module_partial_spec::test_partial_partial_should_work_with_placeholders_and_curried_functions`
- `__smelt_module_partial_spec::test_partial_partial_supports_placeholders`
- `__smelt_module_pickBy_spec::test_pickby_should_pick_properties_based_on_the_predicate_function`
- `__smelt_module_pick_spec::test_pick_should_pick_properties_from_an_object`
- `__smelt_module_pick_spec::test_pick_should_return_the_same_object_if_all_keys_are_picked`
- `__smelt_module_pick_spec::test_pick_should_work_with_nested_objects`
- `__smelt_module_pullAt_spec::test_pullat_even_if_there_are_duplicate_index_values_must_return_an_array_containing_duplicate_index_values`
- `__smelt_module_pullAt_spec::test_pullat_even_if_there_are_not_index_value_must_return_an_array_containing_undefined_value`
- `__smelt_module_pullAt_spec::test_pullat_even_if_there_are_other_instance_or_type_must_return_an_array_containing_other_instance_or_type_values`
- `__smelt_module_pullAt_spec::test_pullat_should_returns_index_searched_of_original_array_and_changed_original_array`
- `__smelt_module_pullAt_spec::test_pullat_should_work_with_objects`
- `__smelt_module_pullAt_spec::test_pullat_should_work_with_unsorted_indexes`
- `__smelt_module_pull_spec::test_pull_should_remove_all_occurrences_of_specified_values_from_the_array`
- `__smelt_module_pull_spec::test_pull_should_remove_duplicate_values_only_if_they_match_the_specified_values`
- `__smelt_module_pull_spec::test_pull_should_return_the_modified_array_after_removing_specified_values`
- `__smelt_module_randomInt_spec::test_randomint_generates_a_random_integer_between_0_inclusive_and_max_exclusive`
- `__smelt_module_reduceAsync_spec::test_reduceasync_without_initial_value_returns_undefined_for_empty_array_without_initial_value`
- `__smelt_module_remove_spec::test_remove_should_handle_sparse_arrays_correctly`
- `__smelt_module_remove_spec::test_remove_should_remove_elements_based_on_the_predicate_function`
- `__smelt_module_remove_spec::test_remove_should_return_all_elements_if_all_elements_are_removed`
- `__smelt_module_rest_spec::test_rest_should_apply_a_rest_parameter_to_func`
- `__smelt_module_rest_spec::test_rest_should_use_an_empty_array_when_start_is_not_reached`
- `__smelt_module_rest_spec::test_rest_should_work_on_functions_with_more_than_three_parameters`
- `__smelt_module_rest_spec::test_rest_should_work_with_start`
- `__smelt_module_retry_spec::test_retry_should_not_retry_when_shouldretry_returns_false`
- `__smelt_module_retry_spec::test_retry_should_retry_when_shouldretry_returns_true`
- `__smelt_module_retry_spec::test_retry_should_retry_with_a_dynamic_delay_function_based_on_attempt_count`
- `__smelt_module_retry_spec::test_retry_should_retry_with_the_specified_delay_between_attempts`
- `__smelt_module_retry_spec::test_retry_should_throw_an_error_after_the_specified_number_of_retries`
- `__smelt_module_round_spec::test_round_function_handles_negative_numbers_properly`
- `__smelt_module_round_spec::test_round_function_rounds_a_number_to_zero_decimal_places_by_default`
- `__smelt_module_round_spec::test_round_function_rounds_correctly_with_edge_cases`
- `__smelt_module_sampleSize_spec::test_samplesize_returns_a_sample_element_array_of_a_specified_size`
- `__smelt_module_semaphore_spec::test_semaphore_should_resolve_requests_in_the_order_they_were_made_when_permits_are_released`
- `__smelt_module_sortKeys_spec::test_sortkeys_should_preserve_values_after_sorting`
- `__smelt_module_sortKeys_spec::test_sortkeys_should_sort_keys_with_a_custom_compare_function`
- `__smelt_module_spread_spec::test_spread_should_maintain_the_context_of_this_when_calling_the_original_function`
- `__smelt_module_sumBy_spec::test_sumby_function_ensures_that_adding_the_sums_of_two_arrays_equals_the_sum_of_their_concatenation`
- `__smelt_module_throttle_spec::test_throttle_should_call_the_function_with_correct_arguments`
- `__smelt_module_throttle_spec::test_throttle_should_execute_on_leading_and_trailing_when_called_multiple_times_with_leading_and_trailing`
- `__smelt_module_throttle_spec::test_throttle_should_execute_the_function_immediately_if_not_called_within_the_wait_time`
- `__smelt_module_throttle_spec::test_throttle_should_invoke_function_periodically_with_leading_edge_only`
- `__smelt_module_throttle_spec::test_throttle_should_invoke_function_periodically_with_trailing_edge_only`
- `__smelt_module_throttle_spec::test_throttle_should_preserve_this_context_when_called_as_a_method`
- `__smelt_module_throttle_spec::test_throttle_should_throttle_function_calls`
- `__smelt_module_timeout_spec::test_timeout_rejects_with_a_timeouterror_when_a_non_aborted_signal_is_provided`
- `__smelt_module_timeout_spec::test_timeout_returns_a_reason_if_a_response_is_received_after_the_specified_wait_time`
- `__smelt_module_toCamelCaseKeys_spec::test_camelizekeys_should_handle_arrays_inside_objects`
- `__smelt_module_toCamelCaseKeys_spec::test_camelizekeys_should_handle_arrays_of_objects`
- `__smelt_module_toMerged_spec::test_tomerged_should_deeply_clone_untouched_nested_subtrees_even_when_a_sibling_key_is_merged`
- `__smelt_module_toMerged_spec::test_tomerged_should_deeply_merge_nested_objects`
- `__smelt_module_toMerged_spec::test_tomerged_should_deeply_merge_nested_objects_if_they_are_shared`
- `__smelt_module_toMerged_spec::test_tomerged_should_handle_merging_of_deeply_nested_objects_with_arrays_and_objects`
- `__smelt_module_toMerged_spec::test_tomerged_should_handle_merging_with_null_values`
- `__smelt_module_toMerged_spec::test_tomerged_should_handle_nested_case_where_non_plain_object_is_replaced_with_plain_object`
- `__smelt_module_toMerged_spec::test_tomerged_should_merge_arrays_deeply`
- `__smelt_module_toMerged_spec::test_tomerged_should_merge_properties_from_source_object_into_target_object`
- `__smelt_module_toMerged_spec::test_tomerged_should_not_overwrite_existing_values_with_undefined_from_source`
- `__smelt_module_toMerged_spec::test_tomerged_should_replace_non_plain_object_target_value_with_plain_object_from_source`
- `__smelt_module_toSnakeCaseKeys_spec::test_snakeizekeys_should_handle_arrays_inside_objects`
- `__smelt_module_toSnakeCaseKeys_spec::test_snakeizekeys_should_handle_arrays_of_objects`
- `__smelt_module_toSnakeCaseKeys_spec::test_snakeizekeys_should_preserve_object_prototype_methods`
- `__smelt_module_trimEnd_spec::test_trimend_should_handle_cases_where_multiple_trailing_characters_in_the_array_need_removal`
- `__smelt_module_trimEnd_spec::test_trimend_should_remove_trailing_characters_from_the_string_when_multiple_characters_are_provided_in_an_array`
- `__smelt_module_trimEnd_spec::test_trimend_should_remove_trailing_characters_when_chars_is_an_array`
- `__smelt_module_trimStart_spec::test_trimstart_should_handle_cases_where_multiple_leading_characters_in_the_array_need_removal`
- `__smelt_module_trimStart_spec::test_trimstart_should_remove_leading_characters_from_the_string_when_multiple_characters_are_provided_in_an_array`
- `__smelt_module_trimStart_spec::test_trimstart_should_remove_leading_characters_when_chars_is_an_array`
- `__smelt_module_trimStart_spec::test_trimstart_should_remove_leading_spaces_and_other_characters_when_specified_in_an_array`
- `__smelt_module_trim_spec::test_trim_should_remove_all_occurrences_of_multiple_characters`
- `__smelt_module_trim_spec::test_trim_should_remove_numbers_from_a_string`
- `__smelt_module_trim_spec::test_trim_should_return_the_string_without_special_characters`
- `__smelt_module_unary_spec::test_unary_should_not_force_a_minimum_argument_count`
- `__smelt_module_unary_spec::test_unary_should_use_this_binding_of_function`
- `__smelt_module_unescape_spec::test_unescape_should_unescape_entities_in_order`
- `__smelt_module_unescape_spec::test_unescape_should_unescape_the_proper_entities`
- `__smelt_module_unescape_spec::test_unescape_should_unescape_the_same_characters_escaped_by_escape`
- `__smelt_module_uniq_spec::test_uniq_should_handle_arrays_with_special_values`
- `__smelt_module_uniq_spec::test_uniq_should_handle_arrays_with_undefined_and_hole`
- `__smelt_module_unzip_spec::test_unzip_should_handle_arrays_of_different_lengths`
- `__smelt_module_unzip_spec::test_unzip_should_unzip_arrays_correctly`
- `__smelt_module_withTimeout_spec::test_withtimeout_lifts_the_time_limit_when_the_signal_is_aborted_resolving_with_the_run_result`
- `__smelt_module_withTimeout_spec::test_withtimeout_returns_a_reason_if_a_response_is_received_after_the_specified_wait_time`
- `__smelt_module_withTimeout_spec::test_withtimeout_returns_the_result_value_if_a_response_is_received_before_the_specified_wait_time`
- `__smelt_module_withTimeout_spec::test_withtimeout_times_out_when_a_non_aborted_signal_is_provided`
- `__smelt_module_zipWith_spec::test_zipwith_should_provide_index_parameter_to_combine_function`
- `__smelt_module_zipWith_spec::test_zipwith_zips_multiple_arrays_with_the_given_combine_function`
- `__smelt_module_zip_spec::test_zip_zips_multiple_arrays_to_create_a_tuple`

</details>

---

# Family 7 — Exception-Payload ABI

Measured at es-toolkit ref `e008a2818cd8d07469a5cc12ee0c02405d523e07` with the
fixture manifest `third_party/es-toolkit/Smelt.toml`, on top of Smelt commit
`bfb68f18`.

| Suite | Before | After |
|---|---|---|
| es-toolkit generated tests | 789 passed / 270 failed | **791 passed / 268 failed** (2 resolved, 0 newly failing) |
| Remeda regression guard | 1789 passed / 0 failed | **1789 passed / 0 failed** |
| `smelt-codegen-rust --lib` | 710 passed | **716 passed / 0 failed** (6 new) |
| es-toolkit avoidable erasure | 35846 | **35789 (-57)**; legitimate boundary +3488 |

## Confirmed root cause

The prior analysis was right about the mechanism and wrong about the line numbers
and the blast radius. There were **four** sites that destroyed a thrown payload,
not two:

| Site | Old code |
|---|---|
| `crates/smelt-codegen-rust/src/emitter/control_flow.rs:713` (function-level `Terminator::Throw`) | `std::io::Error::new(ErrorKind::Other, format!("{}", value))` |
| `crates/smelt-codegen-rust/src/emitter/closures.rs:1261` (closure-body `Terminator::Throw`) | same |
| `crates/smelt-codegen-rust/src/emitter/call.rs:243` (`new Promise` reject bridge) | hand-extracted `message`, else `format!("{}", error)` |
| `crates/smelt-codegen-rust/src/emitter/control_flow.rs:964` (`await`-with-unwind catch binding) | `let __smelt_error = __smelt_error.to_string();` |

Because an erased JavaScript `Error` is a marker-bearing `SmeltUnknown::Object`,
`Display` rendered it as the literal text `[object Object]`. The catch side then
*rebuilt* a synthetic `{__smelt_error: true, message: <Display text>}` record
(`control_flow.rs:853/898/913/970`), so class, `name`, `cause` and every custom
field were unrecoverable.

Cleanest witness, before the fix — `invariant(false, new CustomError(msg))`
re-throwing a caller-supplied error (`dist-smelt/src/invariant.rs`):

```rust
return Err::<_, Box<dyn std::error::Error>>(
    std::io::Error::new(std::io::ErrorKind::Other, format!("{}", _smelt_tmp_5)).into());
//                                                          ^ an Error object -> "[object Object]"
```

Three `retry_spec` tests failed with the bare text `[object Object]` as their
own panic message, which is the defect printing itself.

## What changed, and why it is a general rule

New `crates/smelt-codegen-rust/src/thrown.rs` emits a payload-carrying error type
and the two adapters that bracket the boundary:

```rust
struct SmeltThrown { value: SmeltUnknown }
fn smelt_throw(value: SmeltUnknown) -> Box<dyn std::error::Error>
fn smelt_thrown_value(error: &(dyn std::error::Error + 'static)) -> SmeltUnknown
fn smelt_thrown_message(value: &SmeltUnknown) -> String
```

New `crates/smelt-codegen-rust/src/emitter/throw.rs` holds the two emitter-side
helpers (`throw_terminator_text`, `caught_error_value_text`) that both the
function-level and closure-body emitters now share, so the two throw paths cannot
drift apart again.

This is a general rule, not a per-library patch: it applies to *every* `throw`,
`reject`, and `catch` in any input program. Three properties keep it behavior
preserving:

- `SmeltThrown`'s `Display` projects the payload's `message` field, which is
  exactly the text the old reject bridge built by hand. String-typed `catch`
  bindings and `.toThrow("substring")` matchers observe byte-identical text.
- Payload recovery is a `downcast_ref`, so the channel stays open to foreign
  errors arriving via `?`; those still present as the old rebuilt record.
- Programs with no erased values have no `SmeltUnknown` in their prelude, so the
  throw site falls back to the old stringified form (nothing is lost — without
  `SmeltUnknown` there is no erased `catch` binding to observe a payload).

## Why the payload is a `SmeltUnknown`

A `throw` payload is the textbook dynamic boundary. `throw` accepts any value;
TypeScript types a `catch` binding `any`/`unknown` by construction; there is no
throws-clause, so nothing propagates a payload type from throw site to catch
site; and the error channel is *one* type shared crate-wide, including by erased
callbacks whose signature is fixed at
`Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn Error>>>` and so
cannot mention a caller-specific error type.

Documented in a comment at the emit site (`thrown.rs` module docs and the
`SmeltThrown` doc comment in the generated prelude) and proven by
`one_channel_carries_structurally_unrelated_payloads` in
`crates/smelt-codegen-rust/src/tests/thrown_tests.rs`, which sends a field-bearing
record and a bare string through one function's channel on a run-time branch and
recovers both at a single `catch`.

`classify_line` in `crates/smelt-transpiler/src/unknown_report.rs` was extended to
score `smelt_throw(` / `smelt_thrown_value(` as legitimate boundary, per the
CLAUDE.md reclassify-and-re-snapshot procedure; the baseline
`blocker-logs/smelt-unknown-baseline-es-toolkit.json` was re-snapshotted in the
same change because avoidable erasure *fell* by 57 (the deleted catch-site record
rebuilds and reject-bridge tag matches were real avoidable erasure).

## Honest accounting: why only +2 tests

The ABI is now correct at all four sites, but almost every remaining
throw-assertion failure is gated behind a *different* defect. Verified
individually in the regenerated output:

1. **`AbortError` / `TimeoutError` / `delay` / `timeout` / `withTimeout` (~13
   tests)** — `class AbortError extends DOMException { constructor(message = '...') }`
   emits `fn new(message: String) -> Self` whose body sets every field to `Null`:
   the `super(message)` call and the default parameter value are both dropped. The
   base is a *const-valued* `DOMException` (a `typeof globalThis.DOMException`
   expression with an `Error` fallback), not a class declaration, so class
   lowering never models the constructor. `rejects.toThrow('The operation was
   aborted')` fails because the message is empty, not because the payload is lost.
   **Frontend/class-lowering work, not a bounded codegen patch.**
2. **`invariant` (4 tests)** — the `invariant(...)` call is elided from the test
   callback entirely; the generated closure body constructs `CustomError`, then
   evaluates a dead `false`. An `asserts condition` overload signature is losing
   its call. **Frontend work.**
3. **`retry` (4 tests)** — now propagates a *structured* payload
   (`{__smelt_error, message: "Server Error"}`) instead of `[object Object]`, so
   the ABI works; the error escaping at all is a `retry` loop/`shouldRetry`
   defect.
4. **`attemptAsync` / `attempt` (5 tests)** — `throw new Error(msg)` is collapsed
   to its bare message *string* in the frontend before MIR, so
   `error instanceof Error` is false at the catch. Separately,
   `expect(a && b).toBe('text')` folds to `!(false)` because JS `a && b` is
   lowered as a boolean rather than returning `b`. **Both frontend.**

The two tests that did flip (`retry_spec::should_abort_the_retry_operation_if_the_signal_is_already_aborted`,
`retry_spec::should_pass_attempt_number_to_shouldretry`) are the ones whose only
blocker was the payload.

The payload ABI is a prerequisite for all of the above — none of them can pass
while `catch` can only see a string — so this is enabling work whose test yield
shows up once families 9 and the frontend items land.

## Next lever

Item 1 above (`super(...)` and default parameters for a class extending a
const-valued base) is the highest-leverage remaining item in this cluster: ~13
tests, one coherent cause, and it is what family 9 was actually describing.
