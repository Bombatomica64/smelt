# Generated Rust Test Report

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 789 passed; 270 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.99s`
- Failing tests: `270`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 23 | `__smelt_module_isEqualWith_spec` |
| 17 | `__smelt_module_cloneDeep_spec` |
| 15 | `__smelt_module_clone_spec` |
| 10 | `__smelt_module_debounce_spec` |
| 10 | `__smelt_module_toMerged_spec` |
| 7 | `__smelt_module_retry_spec` |
| 7 | `__smelt_module_throttle_spec` |
| 6 | `__smelt_module_cloneDeepWith_spec` |
| 6 | `__smelt_module_memoize_spec` |
| 6 | `__smelt_module_pullAt_spec` |
| 5 | `__smelt_module_partialRight_spec` |
| 5 | `__smelt_module_partial_spec` |
| 4 | `__smelt_module_allKeyed_spec` |
| 4 | `__smelt_module_ary_spec` |
| 4 | `__smelt_module_attemptAsync_spec` |
| 4 | `__smelt_module_flow_spec` |
| 4 | `__smelt_module_invariant_spec` |
| 4 | `__smelt_module_mergeWith_spec` |
| 4 | `__smelt_module_rest_spec` |
| 4 | `__smelt_module_trimStart_spec` |

### Delta From Baseline

- Baseline report: `blocker-logs/estoolkit-runtime-current.md`
- Resolved tests: `30`
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
- `__smelt_module_retry_spec::test_retry_should_abort_the_retry_operation_if_the_signal_is_already_aborted`
- `__smelt_module_retry_spec::test_retry_should_not_retry_when_shouldretry_returns_false`
- `__smelt_module_retry_spec::test_retry_should_pass_attempt_number_to_shouldretry`
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

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `354`

## Summary By Code

1. **warning** `unused_mut` - 228 diagnostics
2. **warning** `unused_assignments` - 79 diagnostics
3. **warning** `unused_parens` - 33 diagnostics
4. **warning** `noop_method_call` - 8 diagnostics
5. **warning** `path_statements` - 2 diagnostics
6. **warning** `private_interfaces` - 2 diagnostics
7. **warning** `non_camel_case_types` - 1 diagnostic
8. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 228 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/allKeyed.rs:70`
     - `src/retry.rs:50`
     - `src/retry.rs:53`
     - `src/retry.rs:56`
     - `src/retry.rs:58`
2. **warning** `unused_parens` - 21 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/chunk.rs:14`
     - `src/curryRight.rs:205`
     - `src/decimalAdjust.rs:60`
     - `src/dropRight_1.rs:11`
     - `src/drop_1.rs:8`
3. **warning** `unused_assignments` - 9 occurrences
   - Message: value assigned to `resolved_path` is never read
   - Examples:
     - `src/has.rs:37`
     - `src/has.rs:81`
     - `src/has.rs:78`
     - `src/hasIn.rs:121`
     - `src/hasIn.rs:118`
4. **warning** `noop_method_call` - 8 occurrences
   - Message: call to `.clone()` on a reference in this situation does nothing
   - Examples:
     - `src/differenceWith_1.rs:13`
     - `src/differenceWith_1.rs:22`
     - `src/differenceWith_1.rs:37`
     - `src/differenceWith_1.rs:46`
     - `src/intersectionWith_1.rs:13`
5. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `i_2` is never read
   - Examples:
     - `src/mergeWith_1.rs:26508`
     - `src/mergeWith_1.rs:22762`
     - `src/mergeWith_1.rs:19011`
     - `src/mergeWith_1.rs:15265`
     - `src/mergeWith_1.rs:11508`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:3877`
     - `src/main.rs:3933`
     - `src/main.rs:3945`
     - `src/main.rs:3956`
     - `src/main.rs:4273`
7. **warning** `unused_assignments` - 6 occurrences
   - Message: value assigned to `predicate` is never read
   - Examples:
     - `src/cond.rs:43`
     - `src/every.rs:166`
     - `src/every.rs:163`
     - `src/every.rs:172`
     - `src/every.rs:177`
8. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `keys` is never read
   - Examples:
     - `src/filter.rs:73`
     - `src/reduce.rs:63`
     - `src/reduce.rs:53`
     - `src/reduceRight.rs:66`
     - `src/reduceRight.rs:55`
9. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `get_value_by_criterion` is never read
   - Examples:
     - `src/orderBy.rs:4911`
     - `src/orderBy.rs:3470`
     - `src/orderBy.rs:2009`
     - `src/orderBy.rs:568`
10. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/overEvery.rs:77`
     - `src/overSome.rs:77`
     - `src/sumBy_1.rs:50`
11. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `intersections` is never read
   - Examples:
     - `src/xorBy.rs:150`
     - `src/xorBy.rs:98`
     - `src/xorWith.rs:88`
12. **warning** `path_statements` - 2 occurrences
   - Message: path statement drops value
   - Examples:
     - `src/main.rs:5819`
     - `src/main.rs:5876`
13. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/findIndex.rs:111`
     - `src/findLastIndex.rs:126`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `mapped` is never read
   - Examples:
     - `src/flatMap.rs:28`
     - `src/flatMap.rs:21`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_error` is never read
   - Examples:
     - `src/clone.rs:212`
     - `src/clone.rs:119`
16. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_object` is never read
   - Examples:
     - `src/clone.rs:245`
     - `src/clone.rs:152`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/filter.rs:69`
     - `src/sumBy_1.rs:49`
18. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `start_index` is never read
   - Examples:
     - `src/reduce.rs:54`
     - `src/reduceRight.rs:56`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/delay_1.rs:58`
     - `src/timeout.rs:46`
20. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/findIndex.rs:128`
     - `src/findLastIndex.rs:129`
21. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around `match` scrutinee expression
   - Examples:
     - `src/findIndex.rs:83`
     - `src/findLastIndex.rs:108`
22. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/template.rs:295`
     - `src/template.rs:320`
23. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_2461` should have an upper camel case name
   - Examples:
     - `src/main.rs:5188`
24. **warning** `private_interfaces` - 1 occurrence
   - Message: type `File` is more private than the item `SmeltUnion2007::M1::0`
   - Examples:
     - `src/main.rs:5664`
25. **warning** `private_interfaces` - 1 occurrence
   - Message: type `RetryOptions` is more private than the item `SmeltUnion1815::M1::0`
   - Examples:
     - `src/main.rs:5524`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `byte_offset` is never read
   - Examples:
     - `src/clone_1.rs:217`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cache_constructor` is never read
   - Examples:
     - `src/memoize_1.rs:143`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `customizer_fn` is never read
   - Examples:
     - `src/setWith.rs:11`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `delay` is never read
   - Examples:
     - `src/retry.rs:35`
30. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `dest_view` is never read
   - Examples:
     - `src/clone_1.rs:227`
31. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `elements` is never read
   - Examples:
     - `src/zipWith_1.rs:83`
32. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/slice.rs:59`
33. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `error` is never read
   - Examples:
     - `src/retry.rs:207`
34. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/debounce.rs:162`
35. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/cond.rs:45`
36. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `i_1` is never read
   - Examples:
     - `src/filter.rs:81`
37. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key_1` is never read
   - Examples:
     - `src/some.rs:208`
38. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `length_1` is never read
   - Examples:
     - `src/filter.rs:80`
39. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `new_value` is never read
   - Examples:
     - `src/updateWith.rs:97`
40. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `pair` is never read
   - Examples:
     - `src/cond.rs:41`
41. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `predicate_1` is never read
   - Examples:
     - `src/filter.rs:63`
42. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_1` is never read
   - Examples:
     - `src/filter.rs:77`
43. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_length` is never read
   - Examples:
     - `src/pullAllWith.rs:59`
44. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `retries` is never read
   - Examples:
     - `src/retry.rs:36`
45. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `set_low` is never read
   - Examples:
     - `src/sortedIndexBy.rs:99`
46. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `should_retry` is never read
   - Examples:
     - `src/retry.rs:38`
47. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `signal` is never read
   - Examples:
     - `src/retry.rs:37`
48. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `src_view` is never read
   - Examples:
     - `src/clone_1.rs:224`
49. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/slice.rs:58`
50. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_1` is never read
   - Examples:
     - `src/some.rs:210`
51. **warning** `unused_must_use` - 1 occurrence
   - Message: unused return value of `std::clone::Clone::clone` that must be used
   - Examples:
     - `src/delay_1.rs:43`

## Cargo Stderr

```text
Checking es_toolkit_probe v0.1.0 (/home/lollo/Playground/smelt/.claude/worktrees/estoolkit-regressions-6e38b1/third_party/es-toolkit/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.08s
```
