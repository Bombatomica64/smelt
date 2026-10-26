# TypeScript 7.1.0-beta type parity

Inputs: examples/typescript/end-to-end/01_number/input.ts, examples/typescript/end-to-end/02_string/input.ts, examples/typescript/end-to-end/03_boolean/input.ts, examples/typescript/end-to-end/04_null/input.ts, examples/typescript/end-to-end/05_alias/input.ts, examples/typescript/end-to-end/06_array_literal/input.ts, examples/typescript/end-to-end/07_tuple_literal/input.ts, examples/typescript/end-to-end/08_record_literal/input.ts, examples/typescript/end-to-end/09_index_access/input.ts, examples/typescript/end-to-end/100_elided_type_parameters/input.ts, examples/typescript/end-to-end/101_arrow_const_infers_its_return/input.ts, examples/typescript/end-to-end/102_body_locals_are_body_scoped/input.ts, examples/typescript/end-to-end/103_exclude_over_a_literal_union/input.ts, examples/typescript/end-to-end/104_erased_callback_returns_a_host_class/input.ts, examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts, examples/typescript/end-to-end/106_host_value_at_a_record_type/input.ts, examples/typescript/end-to-end/107_never_branch_at_a_string_return/input.ts, examples/typescript/end-to-end/108_headers_init_at_a_headers_slot/input.ts, examples/typescript/end-to-end/109_nested_function_declaration_hoists/input.ts, examples/typescript/end-to-end/10_unary_logical/input.ts, examples/typescript/end-to-end/110_captured_callback_handle_is_cloned/input.ts, examples/typescript/end-to-end/112_recursive_binding_keeps_its_type/input.ts, examples/typescript/end-to-end/115_transitive_closure_captures/input.ts, examples/typescript/end-to-end/116_generator_array_from/input.ts, examples/typescript/end-to-end/117_forward_exported_arrow/input.ts, examples/typescript/end-to-end/118_concise_callback_call_args/input.ts, examples/typescript/end-to-end/119_arrow_param_default/input.ts, examples/typescript/end-to-end/11_console_log_expressions/input.ts, examples/typescript/end-to-end/120_forward_arrow_after_class/input.ts, examples/typescript/end-to-end/121_callback_into_union_function_arm/input.ts, examples/typescript/end-to-end/122_loop_inside_branch_inside_loop/input.ts, examples/typescript/end-to-end/123_string_from_codes_and_search/input.ts, examples/typescript/end-to-end/124_optional_argument_and_receiver/input.ts, examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts, examples/typescript/end-to-end/126_optional_presence_through_assertion/input.ts, examples/typescript/end-to-end/127_nested_closure_shared_write/input.ts, examples/typescript/end-to-end/128_match_array_groups/input.ts, examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts, examples/typescript/end-to-end/12_while_sum/input.ts, examples/typescript/end-to-end/130_this_bound_field_write_through/input.ts, examples/typescript/end-to-end/131_reduce_right_and_declared_const_shapes/input.ts, examples/typescript/end-to-end/132_user_methods_named_like_array_builtins/input.ts, examples/typescript/end-to-end/133_presence_rules/input.ts, examples/typescript/end-to-end/134_absence_and_host_boundaries/input.ts, examples/typescript/end-to-end/13_for_of_sum/input.ts, examples/typescript/end-to-end/140_user_type_guard_narrowing/input.ts, examples/typescript/end-to-end/142_module_global_reads_see_module_values/input.ts, examples/typescript/end-to-end/143_module_arrows_are_items/input.ts, examples/typescript/end-to-end/14_c_for_loop/input.ts, examples/typescript/end-to-end/151_derived_constructor_runs_on_instance/input.ts, examples/typescript/end-to-end/152_callable_interface_fields/input.ts, examples/typescript/end-to-end/153_this_param_function_as_method/input.ts, examples/typescript/end-to-end/154_getter_receivers_in_closures/input.ts, examples/typescript/end-to-end/155_erased_instance_identity/input.ts, examples/typescript/end-to-end/156_runtime_shape_behind_static_types/input.ts, examples/typescript/end-to-end/157_node_path_hash_and_runtime_regexp/input.ts, examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts, examples/typescript/end-to-end/159_spread_join_tuple_return_finally/input.ts, examples/typescript/end-to-end/15_break_continue/input.ts, examples/typescript/end-to-end/160_function_item_typed_identity/input.ts, examples/typescript/end-to-end/162_alias_names_generic_class/input.ts, examples/typescript/end-to-end/165_void_operand_runs/input.ts, examples/typescript/end-to-end/166_closure_stored_on_its_receiver/input.ts, examples/typescript/end-to-end/167_proven_integer_counters/input.ts, examples/typescript/end-to-end/168_bun_int32_slots/input.ts, examples/typescript/end-to-end/169_json_typed_entry_decode/input.ts, examples/typescript/end-to-end/16_switch_break_no_fallthrough/input.ts, examples/typescript/end-to-end/170_bigint_arbitrary_precision/input.ts, examples/typescript/end-to-end/171_module_const_with_async_callback/input.ts, examples/typescript/end-to-end/17_mutating_array/input.ts, examples/typescript/end-to-end/18_class_fields/input.ts, examples/typescript/end-to-end/19_constructor/input.ts, examples/typescript/end-to-end/20_method_call/input.ts, examples/typescript/end-to-end/21_this_field/input.ts, examples/typescript/end-to-end/22_mutating_method/input.ts, examples/typescript/end-to-end/23_interface_shape/input.ts, examples/typescript/end-to-end/24_interface_method_signature/input.ts, examples/typescript/end-to-end/25_private_protected_metadata/input.ts, examples/typescript/end-to-end/26_interface_inheritance_optional_computed/input.ts, examples/typescript/end-to-end/27_optional_chains/input.ts, examples/typescript/end-to-end/28_regex_match_result/input.ts, examples/typescript/end-to-end/29_callable_object/input.ts, examples/typescript/end-to-end/30_nullish_union_join/input.ts, examples/typescript/end-to-end/31_headers_fetch_type/input.ts, examples/typescript/end-to-end/32_url_search_params/input.ts, examples/typescript/end-to-end/33_console_optional_value/input.ts, examples/typescript/end-to-end/34_optional_field_interface_literal/input.ts, examples/typescript/end-to-end/35_top_level_await/input.ts, examples/typescript/end-to-end/36_floating_promise_drained/input.ts, examples/typescript/end-to-end/38_interface_literal_key_spellings/input.ts, examples/typescript/end-to-end/39_module_scope_reassignment/input.ts, examples/typescript/end-to-end/40_computed_method_over_known_members/input.ts, examples/typescript/end-to-end/41_symbol_keyed_class_members/input.ts, examples/typescript/end-to-end/42_module_arrow_shared_capture/input.ts, examples/typescript/end-to-end/43_await_value_or_promise_union/input.ts, examples/typescript/end-to-end/44_node_http_echo/input.ts, examples/typescript/end-to-end/45_text_codec/input.ts, examples/typescript/end-to-end/46_blob_file/input.ts, examples/typescript/end-to-end/47_module_const_host_value/input.ts, examples/typescript/end-to-end/48_form_data/input.ts, examples/typescript/end-to-end/49_sibling_method_call/input.ts, examples/typescript/end-to-end/50_union_arm_narrowing/input.ts, examples/typescript/end-to-end/51_web_crypto/input.ts, examples/typescript/end-to-end/53_top_level_try_tail/input.ts, examples/typescript/end-to-end/54_regex_test_predicate/input.ts, examples/typescript/end-to-end/55_headers_init_union/input.ts, examples/typescript/end-to-end/56_union_member_read/input.ts, examples/typescript/end-to-end/57_reassigned_method/input.ts, examples/typescript/end-to-end/58_absent_value_stringify/input.ts, examples/typescript/end-to-end/59_ambient_response_init/input.ts, examples/typescript/end-to-end/60_response_body_handle/input.ts, examples/typescript/end-to-end/61_tuple_spread_arguments/input.ts, examples/typescript/end-to-end/63_generic_init_union_arm/input.ts, examples/typescript/end-to-end/64_error_subclass_optional_message/input.ts, examples/typescript/end-to-end/65_typeof_keyof_alias_union/input.ts, examples/typescript/end-to-end/66_json_stringify_shapes/input.ts, examples/typescript/end-to-end/67_compound_bitwise_assignment/input.ts, examples/typescript/end-to-end/68_switch_continue_in_loop/input.ts, examples/typescript/end-to-end/69_asserted_callback_name/input.ts, examples/typescript/end-to-end/70_set_from_iterable/input.ts, examples/typescript/end-to-end/71_const_arrow_literal_hint/input.ts, examples/typescript/end-to-end/72_form_data_for_each/input.ts, examples/typescript/end-to-end/73_set_insertion_order/input.ts, examples/typescript/end-to-end/74_typed_array_views/input.ts, examples/typescript/end-to-end/75_callback_block_return_type/input.ts, examples/typescript/end-to-end/76_base64_globals/input.ts, examples/typescript/end-to-end/77_body_init_buffer_source/input.ts, examples/typescript/end-to-end/78_request_input_forms/input.ts, examples/typescript/end-to-end/79_logical_assignment_store/input.ts, examples/typescript/end-to-end/80_optional_chain_union_and_throw/input.ts, examples/typescript/end-to-end/81_receiver_capture_in_callback/input.ts, examples/typescript/end-to-end/82_data_view_shared_buffer/input.ts, examples/typescript/end-to-end/83_class_expression_binding/input.ts, examples/typescript/end-to-end/84_same_named_methods_across_classes/input.ts, examples/typescript/end-to-end/85_promise_continuations/input.ts, examples/typescript/end-to-end/86_literal_into_union_arm/input.ts, examples/typescript/end-to-end/87_number_to_string/input.ts, examples/typescript/end-to-end/88_data_view_range_error/input.ts, examples/typescript/end-to-end/89_derived_default_constructor/input.ts, examples/typescript/end-to-end/90_type_parameter_defaults/input.ts, examples/typescript/end-to-end/91_union_string_concat/input.ts, examples/typescript/end-to-end/92_request_init_members/input.ts, examples/typescript/end-to-end/93_nullish_assign_and_optional_receiver/input.ts, examples/typescript/end-to-end/94_throwing_getter_in_callback/input.ts, examples/typescript/end-to-end/95_union_value_dispatch/input.ts, examples/typescript/end-to-end/96_union_read_is_not_a_move/input.ts, examples/typescript/end-to-end/97_generic_class_default_bounds/input.ts, examples/typescript/end-to-end/98_async_iife_return_channel/input.ts, examples/typescript/end-to-end/99_substituted_parameter_keeps_its_abi/input.ts

**Parity: 96.5%** (10492 of 10870 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 10492 |
| smelt-erased | 76 |
| wider | 143 |
| mismatch | 159 |
| ts-dynamic | 212 |
| ts-deferred | 14 |
| no-checker-node | 312 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 32 | `Unknown ← number` | examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts:28:19 `number` (erasure: by-name) |
| 19 | `Unknown ← string` | examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts:78:60 `string` |
| 6 | `Unknown ← Named(RangeError)` | examples/typescript/end-to-end/88_data_view_range_error/input.ts:46:19 `RangeError` |
| 4 | `Unknown ← Named(Counter)` | examples/typescript/end-to-end/130_this_bound_field_write_through/input.ts:12:3 `Counter` (erasure: by-name) |
| 4 | `Unknown ← Named(DOMException)` | examples/typescript/end-to-end/76_base64_globals/input.ts:53:19 `DOMException` |
| 3 | `Unknown ← Named(UnsupportedPathError)` | examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts:78:37 `UnsupportedPathError` |
| 2 | `Unknown ← Function` | examples/typescript/end-to-end/130_this_bound_field_write_through/input.ts:15:3 `(prefix: string) => string` |
| 2 | `Unknown ← TypeParam` | examples/typescript/end-to-end/153_this_param_function_as_method/input.ts:13:44 `R` (erasure: by-name) |
| 1 | `Unknown ← Function & Function` | examples/typescript/end-to-end/153_this_param_function_as_method/input.ts:97:7 `((path: string) => number) & ((path: string) => number)` |
| 1 | `Unknown ← Named(Failure)` | examples/typescript/end-to-end/159_spread_join_tuple_return_finally/input.ts:84:60 `Failure` |
| 1 | `Unknown ← Object{1 props}` | examples/typescript/end-to-end/153_this_param_function_as_method/input.ts:62:10 `{ prefix: string; }` (erasure: by-name) |
| 1 | `Unknown ← string-literal \| string-literal` | examples/typescript/end-to-end/140_user_type_guard_narrowing/input.ts:21:46 `"raw" \| "req"` (erasure: by-name) |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 37 | `Optional<String> ← string` | examples/typescript/end-to-end/120_forward_arrow_after_class/input.ts:17:12 `string` |
| 11 | `Optional<Class(HandlerSet)> ← Named(HandlerSet)` | examples/typescript/end-to-end/126_optional_presence_through_assertion/input.ts:24:20 `HandlerSet` |
| 10 | `Optional<Optional<String>> ← string` | examples/typescript/end-to-end/128_match_array_groups/input.ts:33:12 `string` |
| 8 | `Optional<Class(TrieNode)> ← Named(TrieNode)` | examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts:16:7 `TrieNode` |
| 8 | `String \| Float ← string` | examples/typescript/end-to-end/91_union_string_concat/input.ts:43:1 `string` |
| 7 | `Optional<Float> ← number` | examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts:48:38 `number` |
| 6 | `Optional<Class(Response)> ← Named(Response)` | examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts:48:38 `Response` |
| 5 | `Optional<Class(Headers)> ← Named(Headers)` | examples/typescript/end-to-end/93_nullish_assign_and_optional_receiver/input.ts:33:36 `Headers` |
| 4 | `Optional<(String, String, Bool)> ← [string, string, boolean-literal]` | examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts:22:8 `Pattern` |
| 4 | `Optional<Class(Leaf)> ← Named(Leaf)` | examples/typescript/end-to-end/79_logical_assignment_store/input.ts:39:15 `Leaf` |
| 4 | `Optional<Function> ← Function` | examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts:42:7 `(tokens: string[]) => void` |
| 4 | `Unknown \| Float ← number` | examples/typescript/end-to-end/169_json_typed_entry_decode/input.ts:38:33 `number` (erasure: by-name) |
| 3 | `Optional<Class(User)> ← Named(User)` | examples/typescript/end-to-end/27_optional_chains/input.ts:18:14 `User` |
| 3 | `String \| Future<String> ← string` | examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts:16:20 `string` |
| 2 | `Class(Circle) \| Class(Square) ← Named(Circle)` | examples/typescript/end-to-end/169_json_typed_entry_decode/input.ts:38:33 `Circle` |
| 2 | `Class(Circle) \| Class(Square) ← Named(Square)` | examples/typescript/end-to-end/169_json_typed_entry_decode/input.ts:40:10 `Square` |
| 2 | `Class(Response) \| Class(InitLike) ← Named(InitLike)` | examples/typescript/end-to-end/63_generic_init_union_arm/input.ts:37:64 `InitLike<StatusCode>` |
| 2 | `Optional<Class(HandlerParamsSet)> ← Named(HandlerParamsSet)` | examples/typescript/end-to-end/126_optional_presence_through_assertion/input.ts:24:19 `HandlerParamsSet` |
| 2 | `Optional<Optional<List<String>>> ← null \| Array<string>` | examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts:23:5 `string[] \| null` |
| 2 | `String \| List<String> ← Array<string>` | examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts:21:20 `string[]` |
| 1 | `Bool \| Class(RegExp) ← Named(RegExp)` | examples/typescript/end-to-end/133_presence_rules/input.ts:60:30 `RegExp` |
| 1 | `Float \| JsMap ← Named(Map)` | examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts:28:19 `Map<string, number>` |
| 1 | `Float \| JsMap ← number` | examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts:30:17 `number` |
| 1 | `Optional<(Float, List<(…, …)>)> ← [number, Array<[…, …]>]` | examples/typescript/end-to-end/131_reduce_right_and_declared_const_shapes/input.ts:13:20 `[number, ParamAssocArray]` |
| 1 | `Optional<(Float, String)> ← [number, string]` | examples/typescript/end-to-end/142_module_global_reads_see_module_values/input.ts:13:17 `[number, string]` |
| 1 | `Optional<Bool> ← boolean` | examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts:49:23 `boolean` |
| 1 | `Optional<Class(Request)> ← Named(Request)` | examples/typescript/end-to-end/154_getter_receivers_in_closures/input.ts:46:12 `Request` |
| 1 | `Optional<List<(…, …)> \| Dict \| Class(Headers)> ← Array<[string, string]>` | examples/typescript/end-to-end/55_headers_init_union/input.ts:53:36 `[string, string][]` |
| 1 | `Optional<List<(…, …)> \| Dict \| Class(Headers)> ← Array<[…, …]> \| Named(Headers) \| Object{0 props}` | examples/typescript/end-to-end/63_generic_init_union_arm/input.ts:30:32 `HeadersInitLike` |
| 1 | `Optional<List<Float>> ← Array<number>` | examples/typescript/end-to-end/27_optional_chains/input.ts:20:15 `number[]` (erasure: by-name) |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 11 | `Class(__SmeltMatch) ← Named(RegExpExecArray)` | examples/typescript/end-to-end/28_regex_match_result/input.ts:13:17 `RegExpExecArray` |
| 11 | `Class(__SmeltMatch) ← Named(RegExpMatchArray)` | examples/typescript/end-to-end/128_match_array_groups/input.ts:16:17 `RegExpMatchArray` |
| 10 | `Optional<String \| Class(File)> ← null \| string \| Named(File)` | examples/typescript/end-to-end/48_form_data/input.ts:8:13 `FormDataEntryValue \| null` |
| 10 | `String ← Named(RegExp)` | examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts:60:9 `RegExp` |
| 7 | `String ← Named(Error)` | examples/typescript/end-to-end/140_user_type_guard_narrowing/input.ts:94:11 `Error` (erasure: untyped-builtin) |
| 5 | `Class(Boxed) ← Named(class)` | examples/typescript/end-to-end/83_class_expression_binding/input.ts:62:17 `Boxed<number>` |
| 5 | `List<Optional<String>> ← Array<string>` | examples/typescript/end-to-end/128_match_array_groups/input.ts:28:16 `string[]` |
| 5 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpMatchArray)` | examples/typescript/end-to-end/128_match_array_groups/input.ts:14:13 `RegExpMatchArray \| null` |
| 4 | `Class(Reporting) ← Named(class)` | examples/typescript/end-to-end/83_class_expression_binding/input.ts:32:19 `Reporting` |
| 4 | `Class(Uint8Array) \| Class(Int8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(DataView) \| Class(ArrayBuffer) ← Named(ArrayBuffer) \| Named(ArrayBufferView)` | examples/typescript/end-to-end/134_absence_and_host_boundaries/input.ts:28:5 `ArrayBuffer \| ArrayBufferView<ArrayBufferLike>` |
| 4 | `None ← Object{0 props}` | examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts:18:7 `{ [key: string]: Pattern; }` |
| 4 | `Optional<List<String>> ← null \| Named(RegExpMatchArray)` | examples/typescript/end-to-end/128_match_array_groups/input.ts:42:15 `RegExpMatchArray \| null` |
| 3 | `List<(String, String \| Class(File))> ← Named(FormDataIterator)` | examples/typescript/end-to-end/48_form_data/input.ts:15:29 `FormDataIterator<[string, FormDataEntryValue]>` |
| 2 | `Class(ArrayBuffer) ← Named(SharedArrayBuffer)` | examples/typescript/end-to-end/82_data_view_shared_buffer/input.ts:35:28 `SharedArrayBuffer` |
| 2 | `Class(__SmeltMatchGroups) ← undefined \| Object{0 props}` | examples/typescript/end-to-end/28_regex_match_result/input.ts:14:18 `{ [key: string]: string; } \| undefined` |
| 2 | `Class(__smelt_anon_class_1394) ← TypeParam` | examples/typescript/end-to-end/83_class_expression_binding/input.ts:57:5 `this` |
| 2 | `List<Class(__SmeltMatch)> ← Named(RegExpStringIterator)` | examples/typescript/end-to-end/28_regex_match_result/input.ts:11:17 `RegExpStringIterator<RegExpExecArray>` |
| 2 | `List<Float \| String> ← Array<string> \| Array<number>` | examples/typescript/end-to-end/86_literal_into_union_arm/input.ts:52:10 `Either` |
| 2 | `List<Optional<String>> ← Named(RegExpMatchArray) \| []` | examples/typescript/end-to-end/128_match_array_groups/input.ts:51:18 `RegExpMatchArray \| []` |
| 2 | `List<String \| Class(File)> ← Named(FormDataIterator)` | examples/typescript/end-to-end/48_form_data/input.ts:28:21 `FormDataIterator<FormDataEntryValue>` |
| 2 | `List<String> ← Named(FormDataIterator)` | examples/typescript/end-to-end/48_form_data/input.ts:14:17 `FormDataIterator<string>` |
| 2 | `List<String> ← Named(SetIterator)` | examples/typescript/end-to-end/73_set_insertion_order/input.ts:40:17 `SetIterator<string>` |
| 2 | `None ← Array<string>` | examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts:19:7 `string[]` |
| 2 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpExecArray)` | examples/typescript/end-to-end/28_regex_match_result/input.ts:28:15 `RegExpExecArray \| null` |
| 2 | `Optional<String> ← undefined` | examples/typescript/end-to-end/58_absent_value_stringify/input.ts:24:20 `undefined` |
| 1 | `(Float, Optional<Class(Accept)>) ← [number, undefined]` | examples/typescript/end-to-end/159_spread_join_tuple_return_finally/input.ts:33:12 `[number, undefined]` |
| 1 | `Bool ← string-literal \| boolean-literal \| boolean-literal` | examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts:41:13 `"" \| boolean` |
| 1 | `Bool ← undefined \| boolean-literal \| Array<[…, …]> \| Named(Headers) \| Object{0 props}` | examples/typescript/end-to-end/63_generic_init_union_arm/input.ts:29:7 `false \| HeadersInitLike \| undefined` |
| 1 | `Bool ← undefined \| boolean-literal \| boolean-literal` | examples/typescript/end-to-end/152_callable_interface_fields/input.ts:52:9 `boolean \| undefined` |
| 1 | `Bool ← undefined \| number-literal \| number-literal \| number-literal \| boolean-literal` | examples/typescript/end-to-end/63_generic_init_union_arm/input.ts:37:7 `false \| StatusCode \| undefined` |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| examples/typescript/end-to-end/28_regex_match_result/input.ts | 45.9% | 28 | 0 | 16 | 17 | 0 |
| examples/typescript/end-to-end/27_optional_chains/input.ts | 57.1% | 16 | 0 | 11 | 1 | 2 |
| examples/typescript/end-to-end/128_match_array_groups/input.ts | 71.5% | 93 | 0 | 6 | 31 | 0 |
| examples/typescript/end-to-end/125_module_const_state_and_record_union/input.ts | 79.8% | 71 | 0 | 15 | 3 | 0 |
| examples/typescript/end-to-end/91_union_string_concat/input.ts | 83.6% | 61 | 0 | 11 | 1 | 0 |
| examples/typescript/end-to-end/130_this_bound_field_write_through/input.ts | 84.2% | 48 | 9 | 0 | 0 | 0 |
| examples/typescript/end-to-end/83_class_expression_binding/input.ts | 84.3% | 75 | 0 | 0 | 14 | 0 |
| examples/typescript/end-to-end/58_absent_value_stringify/input.ts | 85.4% | 35 | 0 | 3 | 3 | 0 |
| examples/typescript/end-to-end/105_union_instanceof_through_arm_tags/input.ts | 87.5% | 42 | 1 | 5 | 0 | 0 |
| examples/typescript/end-to-end/63_generic_init_union_arm/input.ts | 87.5% | 42 | 0 | 3 | 3 | 0 |
| examples/typescript/end-to-end/158_joins_absence_and_host_reads/input.ts | 87.7% | 100 | 0 | 13 | 1 | 1 |
| examples/typescript/end-to-end/126_optional_presence_through_assertion/input.ts | 90.2% | 120 | 0 | 13 | 0 | 0 |
| examples/typescript/end-to-end/129_keyed_presence_and_throw_sentinel/input.ts | 91.2% | 166 | 5 | 10 | 1 | 7 |
| examples/typescript/end-to-end/76_base64_globals/input.ts | 91.3% | 84 | 8 | 0 | 0 | 3 |
| examples/typescript/end-to-end/50_union_arm_narrowing/input.ts | 91.7% | 88 | 0 | 1 | 7 | 0 |
| examples/typescript/end-to-end/142_module_global_reads_see_module_values/input.ts | 91.8% | 56 | 0 | 1 | 4 | 0 |
| examples/typescript/end-to-end/33_console_optional_value/input.ts | 92.0% | 23 | 0 | 2 | 0 | 0 |
| examples/typescript/end-to-end/88_data_view_range_error/input.ts | 92.1% | 139 | 12 | 0 | 0 | 7 |
| examples/typescript/end-to-end/74_typed_array_views/input.ts | 93.0% | 277 | 21 | 0 | 0 | 0 |
| examples/typescript/end-to-end/169_json_typed_entry_decode/input.ts | 93.7% | 118 | 0 | 8 | 0 | 10 |
| examples/typescript/end-to-end/54_regex_test_predicate/input.ts | 93.9% | 108 | 0 | 0 | 7 | 0 |
| examples/typescript/end-to-end/64_error_subclass_optional_message/input.ts | 94.3% | 33 | 0 | 0 | 2 | 0 |
| examples/typescript/end-to-end/94_throwing_getter_in_callback/input.ts | 94.3% | 83 | 3 | 0 | 2 | 0 |
| examples/typescript/end-to-end/47_module_const_host_value/input.ts | 94.4% | 67 | 0 | 0 | 4 | 0 |
| examples/typescript/end-to-end/72_form_data_for_each/input.ts | 94.7% | 71 | 0 | 0 | 4 | 0 |
| examples/typescript/end-to-end/42_module_arrow_shared_capture/input.ts | 94.7% | 18 | 0 | 0 | 1 | 0 |
| examples/typescript/end-to-end/77_body_init_buffer_source/input.ts | 95.7% | 154 | 5 | 0 | 2 | 0 |
| examples/typescript/end-to-end/48_form_data/input.ts | 95.9% | 186 | 0 | 0 | 8 | 0 |
| examples/typescript/end-to-end/79_logical_assignment_store/input.ts | 96.0% | 192 | 0 | 8 | 0 | 0 |
| examples/typescript/end-to-end/159_spread_join_tuple_return_finally/input.ts | 96.2% | 100 | 2 | 0 | 2 | 5 |

## TypeScript checker errors

| code | count | first message |
| --- | ---: | --- |
| TS18046 | 1 | 'reply' is of type 'unknown'. |
| TS18048 | 2 | 'found.groups' is possibly 'undefined'. |
| TS2307 | 1 | Cannot find module 'bun' or its corresponding type declarations. |
| TS2339 | 2 | Property 'status' does not exist on type 'never'. |
| TS2345 | 1 | Argument of type 'string \| Uint8Array<ArrayBufferLike>' is not assignable to parameter of type 'BodyInit \| null \| undefined'. |
| TS2367 | 2 | This comparison appears to be unintentional because the types 'bigint' and 'number' have no overlap. |
| TS2558 | 1 | Expected 0 type arguments, but got 1. |
| TS2591 | 8 | Cannot find name 'node:path'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig. |
| TS2678 | 1 | Type '1' is not comparable to type '2'. |
| TS2794 | 1 | Expected 1 arguments, but got 0. Did you forget to include 'void' in your type argument to 'Promise'? |
| TS2845 | 1 | This condition will always return 'false'. |
| TS2872 | 1 | This kind of expression is always truthy. |
| TS7006 | 3 | Parameter 'req' implicitly has an 'any' type. |
