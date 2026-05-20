# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `2`
- Warnings: `913`

## Summary By Code

1. **warning** `unused_imports` - 362 diagnostics
2. **warning** `unused_assignments` - 258 diagnostics
3. **warning** `unused_mut` - 193 diagnostics
4. **warning** `unreachable_code` - 64 diagnostics
5. **warning** `unused_parens` - 36 diagnostics
6. **error** `E0308` - 2 diagnostics

## Groups

1. **warning** `unused_mut` - 193 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/add.rs:11`
     - `src/addProp.rs:11`
     - `src/capitalize.rs:11`
     - `src/ceil.rs:12`
     - `src/chunk.rs:11`
2. **warning** `unreachable_code` - 64 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/binarySearchCutoffIndex.rs:33`
     - `src/clone.rs:119`
     - `src/clone.rs:153`
     - `src/conditional.rs:53`
     - `src/countBy.rs:70`
3. **warning** `unused_assignments` - 22 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/evolve.rs:18`
     - `src/forEachObj.rs:17`
     - `src/fromKeys.rs:36`
     - `src/groupBy.rs:17`
     - `src/groupByProp.rs:17`
4. **warning** `unused_assignments` - 20 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/difference.rs:14`
     - `src/evolve.rs:19`
     - `src/forEachObj.rs:18`
     - `src/hasSubObject.rs:18`
     - `src/intersection.rs:14`
5. **warning** `unused_assignments` - 18 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/clone.rs:124`
     - `src/countBy.rs:17`
     - `src/dropWhile.rs:17`
     - `src/fromKeys.rs:17`
     - `src/indexBy.rs:17`
6. **warning** `unused_assignments` - 18 occurrences
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/clone.rs:125`
     - `src/countBy.rs:18`
     - `src/dropFirstBy.rs:19`
     - `src/dropWhile.rs:18`
     - `src/findLast.rs:16`
7. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:40`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
8. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:85`
     - `src/allPass_test.rs:86`
     - `src/anyPass_test.rs:85`
     - `src/anyPass_test.rs:86`
     - `src/purryOrderRules.rs:206`
9. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/conditional.rs:59`
     - `src/dropFirstBy.rs:18`
     - `src/firstBy.rs:17`
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
10. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:26`
     - `src/truncate.rs:160`
     - `src/truncate.rs:145`
     - `src/truncate.rs:119`
     - `src/truncate.rs:104`
11. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
     - `src/times.rs:18`
12. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `out` is never read
   - Examples:
     - `src/dropFirstBy.rs:17`
     - `src/evolve.rs:16`
     - `src/omit.rs:18`
     - `src/product.rs:16`
     - `src/sum.rs:16`
13. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `remaining` is never read
   - Examples:
     - `src/difference.rs:13`
     - `src/intersection.rs:13`
     - `src/omit.rs:17`
     - `src/setPath.rs:18`
     - `src/take.rs:17`
14. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:218`
     - `src/debounce.rs:195`
     - `src/debounce.rs:190`
     - `src/debounce.rs:116`
     - `src/debounce.rs:68`
15. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
16. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:7`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_test.rs:7`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
17. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `items` is never read
   - Examples:
     - `src/groupBy.rs:18`
     - `src/groupByProp.rs:18`
     - `src/pipe.rs:256`
18. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:29`
     - `src/truncate.rs:151`
     - `src/truncate.rs:110`
19. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/debounce.rs:13`
     - `src/randomBigInt.rs:13`
     - `src/main.rs:1182`
20. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
     - `src/meanBy.rs:16`
     - `src/sumBy.rs:17`
21. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
22. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
23. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
24. **error** `E0308` - 2 occurrences
   - Message: mismatched types
   - Examples:
     - `src/allPass.rs:11`
     - `src/anyPass.rs:11`
25. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
26. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:8`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:8`
27. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:130`
     - `src/truncate.rs:89`
28. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `heap` is never read
   - Examples:
     - `src/dropFirstBy.rs:16`
     - `src/takeFirstBy.rs:16`
29. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `length` is never read
   - Examples:
     - `src/range.rs:18`
     - `src/times.rs:16`
30. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `pivot_index` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:7`
     - `src/quickSelect.rs:7`
31. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rounded` is never read
   - Examples:
     - `src/range.rs:55`
     - `src/withPrecision.rs:46`
32. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
     - `src/conditional.rs:58`
33. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
34. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `value_b` is never read
   - Examples:
     - `src/isShallowEqual.rs:19`
     - `src/isShallowEqual.rs:127`
35. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
     - `src/conditional.rs:57`
36. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `accumulator` is never read
   - Examples:
     - `src/pipe.rs:12`
37. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `actual_sample_size` is never read
   - Examples:
     - `src/sample.rs:16`
38. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
39. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
40. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `args` is never read
   - Examples:
     - `src/debounce.rs:60`
41. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `as_set` is never read
   - Examples:
     - `src/isIncludedIn.rs:7`
42. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
43. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `byte` is never read
   - Examples:
     - `src/randomBigInt.rs:69`
44. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cached_value` is never read
   - Examples:
     - `src/funnel_remeda_debounce_test.rs:7`
45. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `category` is never read
   - Examples:
     - `src/countBy.rs:19`
46. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `character` is never read
   - Examples:
     - `src/words.rs:7`
47. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `chunks` is never read
   - Examples:
     - `src/main.rs:1181`
48. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
49. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
50. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
51. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cool_down_timeout_id` is never read
   - Examples:
     - `src/debounce.rs:10`
52. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `copy` is never read
   - Examples:
     - `src/setPath.rs:16`
53. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `count` is never read
   - Examples:
     - `src/countBy.rs:20`
54. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_first` is never read
   - Examples:
     - `src/firstBy.rs:16`
55. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
56. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_item` is never read
   - Examples:
     - `src/pipe.rs:249`
57. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_value` is never read
   - Examples:
     - `src/setPath.rs:17`
58. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current` is never read
   - Examples:
     - `src/conditional.rs:20`
59. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_item` is never read
   - Examples:
     - `src/isDeepEqual.rs:246`
60. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
61. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data` is never read
   - Examples:
     - `src/purryFromLazy.rs:7`
62. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `debouncing_funnel` is never read
   - Examples:
     - `src/funnel_remeda_debounce_test.rs:8`
63. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `destination_value` is never read
   - Examples:
     - `src/mergeDeep.rs:17`
64. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:75`
65. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `double_quoted` is never read
   - Examples:
     - `src/stringToPath.rs:9`
66. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `effective_index` is never read
   - Examples:
     - `src/splitAt.rs:16`
67. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `element` is never read
   - Examples:
     - `src/mapToObj.rs:18`
68. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/range.rs:17`
69. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `excess_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:10`
70. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `first_child_index` is never read
   - Examples:
     - `src/heap.rs:46`
71. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `first_value` is never read
   - Examples:
     - `src/sumBy.rs:16`
72. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/pipe.rs:8`
73. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:303`
74. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_cool_down_end` is never read
   - Examples:
     - `src/debounce.rs:8`
75. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_debounced_call` is never read
   - Examples:
     - `src/debounce.rs:9`
76. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_invoke` is never read
   - Examples:
     - `src/debounce.rs:7`
77. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `head` is never read
   - Examples:
     - `src/heap.rs:22`
78. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `idx` is never read
   - Examples:
     - `src/clone.rs:17`
79. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_done` is never read
   - Examples:
     - `src/pipe.rs:251`
80. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_found` is never read
   - Examples:
     - `src/isDeepEqual.rs:247`
81. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_single` is never read
   - Examples:
     - `src/pipe.rs:15`
82. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `k` is never read
   - Examples:
     - `src/clone.rs:91`
83. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `keys` is never read
   - Examples:
     - `src/isShallowEqual.rs:16`
84. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `last_character` is never read
   - Examples:
     - `src/words.rs:8`
85. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `latest_call_args` is never read
   - Examples:
     - `src/debounce.rs:12`
86. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition_1` is never read
   - Examples:
     - `src/purryFromLazy.rs:10`
87. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
88. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_fn` is never read
   - Examples:
     - `src/pipe.rs:254`
89. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_function` is never read
   - Examples:
     - `src/pipe.rs:7`
90. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_op` is never read
   - Examples:
     - `src/pipe.rs:11`
91. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_result` is never read
   - Examples:
     - `src/pipe.rs:250`
92. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_sequence` is never read
   - Examples:
     - `src/pipe.rs:9`
93. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `left` is never read
   - Examples:
     - `src/drop.rs:17`
94. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_key` is never read
   - Examples:
     - `src/mapKeys.rs:19`
95. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_value` is never read
   - Examples:
     - `src/mapValues.rs:19`
96. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
97. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:8`
98. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:9`
99. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_wait_timeout_id` is never read
   - Examples:
     - `src/debounce.rs:11`
100. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:154`
101. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_projection` is never read
   - Examples:
     - `src/purryOrderRules.rs:153`
102. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `middle_index` is never read
   - Examples:
     - `src/median.rs:17`
103. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `next_comparer` is never read
   - Examples:
     - `src/purryOrderRules.rs:77`
104. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `now` is never read
   - Examples:
     - `src/funnel.rs:74`
105. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `other_copy` is never read
   - Examples:
     - `src/isDeepEqual.rs:245`
106. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `other_item` is never read
   - Examples:
     - `src/isDeepEqual.rs:250`
107. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:47`
108. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `pivot` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:8`
109. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_a` is never read
   - Examples:
     - `src/swapIndices.rs:16`
110. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_b` is never read
   - Examples:
     - `src/swapIndices.rs:17`
111. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `previous_head` is never read
   - Examples:
     - `src/dropFirstBy.rs:20`
112. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
113. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prop_name` is never read
   - Examples:
     - `src/stringToPath.rs:7`
114. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prop` is never read
   - Examples:
     - `src/pathOr.rs:16`
115. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `proto` is never read
   - Examples:
     - `src/isPlainObject.rs:7`
116. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prototype` is never read
   - Examples:
     - `src/clone.rs:16`
117. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `quoted` is never read
   - Examples:
     - `src/stringToPath.rs:8`
118. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `rand` is never read
   - Examples:
     - `src/shuffle.rs:16`
119. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:11`
120. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_char` is never read
   - Examples:
     - `src/randomString.rs:16`
121. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_index` is never read
   - Examples:
     - `src/sample.rs:18`
122. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `range` is never read
   - Examples:
     - `src/randomBigInt.rs:7`
123. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `raw` is never read
   - Examples:
     - `src/randomBigInt.rs:12`
124. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `res` is never read
   - Examples:
     - `src/times.rs:17`
125. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sample_indices` is never read
   - Examples:
     - `src/sample.rs:17`
126. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `second_child_index` is never read
   - Examples:
     - `src/heap.rs:48`
127. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_exponent` is never read
   - Examples:
     - `src/withPrecision.rs:7`
128. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value_as_string` is never read
   - Examples:
     - `src/withPrecision.rs:8`
129. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value` is never read
   - Examples:
     - `src/withPrecision.rs:45`
130. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `should_exit_early` is never read
   - Examples:
     - `src/pipe.rs:14`
131. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sorted_data` is never read
   - Examples:
     - `src/median.rs:16`
132. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `source_value` is never read
   - Examples:
     - `src/mergeDeep.rs:18`
133. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/main.rs:1187`
134. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `step` is never read
   - Examples:
     - `src/range.rs:16`
135. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `summand` is never read
   - Examples:
     - `src/sumBy.rs:21`
136. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `swap_index` is never read
   - Examples:
     - `src/heap.rs:47`
137. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `unquoted` is never read
   - Examples:
     - `src/stringToPath.rs:10`
138. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `v` is never read
   - Examples:
     - `src/clone.rs:92`
139. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_a` is never read
   - Examples:
     - `src/isShallowEqual.rs:18`
140. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
141. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
142. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
143. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
144. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
145. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
146. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
147. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
148. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
149. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
150. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
151. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
152. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
153. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
154. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
155. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
156. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
157. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
158. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
159. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
160. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
161. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
162. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
163. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
164. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
165. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
166. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
167. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
168. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
169. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
170. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
171. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
172. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
173. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
174. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
175. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
176. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
177. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
178. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
179. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp_test::*`
   - Examples:
     - `src/main.rs:83`
180. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add_test::*`
   - Examples:
     - `src/main.rs:85`
181. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:87`
182. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass_test::*`
   - Examples:
     - `src/main.rs:89`
183. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:91`
184. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass_test::*`
   - Examples:
     - `src/main.rs:93`
185. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `binarySearchCutoffIndex_test::*`
   - Examples:
     - `src/main.rs:97`
186. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:99`
187. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize_test::*`
   - Examples:
     - `src/main.rs:101`
188. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:103`
189. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil_test::*`
   - Examples:
     - `src/main.rs:105`
190. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:107`
191. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk_test::*`
   - Examples:
     - `src/main.rs:109`
192. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:111`
193. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp_test::*`
   - Examples:
     - `src/main.rs:113`
194. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:115`
195. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone_test::*`
   - Examples:
     - `src/main.rs:117`
196. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:119`
197. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat_test::*`
   - Examples:
     - `src/main.rs:121`
198. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:123`
199. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional_test::*`
   - Examples:
     - `src/main.rs:125`
200. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:127`
201. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant_test::*`
   - Examples:
     - `src/main.rs:129`
202. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:131`
203. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy_test::*`
   - Examples:
     - `src/main.rs:133`
204. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:135`
205. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce_test::*`
   - Examples:
     - `src/main.rs:137`
206. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:139`
207. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo_test::*`
   - Examples:
     - `src/main.rs:141`
208. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:143`
209. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:145`
210. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith_test::*`
   - Examples:
     - `src/main.rs:147`
211. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference_test::*`
   - Examples:
     - `src/main.rs:149`
212. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:151`
213. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide_test::*`
   - Examples:
     - `src/main.rs:153`
214. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:155`
215. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing_test::*`
   - Examples:
     - `src/main.rs:157`
216. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:159`
217. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:161`
218. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy_test::*`
   - Examples:
     - `src/main.rs:163`
219. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:165`
220. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:167`
221. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile_test::*`
   - Examples:
     - `src/main.rs:169`
222. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast_test::*`
   - Examples:
     - `src/main.rs:171`
223. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:173`
224. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile_test::*`
   - Examples:
     - `src/main.rs:175`
225. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop_test::*`
   - Examples:
     - `src/main.rs:177`
226. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:179`
227. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith_test::*`
   - Examples:
     - `src/main.rs:181`
228. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:183`
229. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries_test::*`
   - Examples:
     - `src/main.rs:185`
230. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:187`
231. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve_test::*`
   - Examples:
     - `src/main.rs:189`
232. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:191`
233. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter_test::*`
   - Examples:
     - `src/main.rs:193`
234. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:195`
235. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:197`
236. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex_test::*`
   - Examples:
     - `src/main.rs:199`
237. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:201`
238. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:203`
239. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex_test::*`
   - Examples:
     - `src/main.rs:205`
240. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast_test::*`
   - Examples:
     - `src/main.rs:207`
241. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find_test::*`
   - Examples:
     - `src/main.rs:209`
242. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:211`
243. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:213`
244. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy_test::*`
   - Examples:
     - `src/main.rs:215`
245. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first_test::*`
   - Examples:
     - `src/main.rs:217`
246. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:219`
247. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:221`
248. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap_test::*`
   - Examples:
     - `src/main.rs:223`
249. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat_test::*`
   - Examples:
     - `src/main.rs:225`
250. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:227`
251. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor_test::*`
   - Examples:
     - `src/main.rs:229`
252. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:231`
253. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:233`
254. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj_test::*`
   - Examples:
     - `src/main.rs:235`
255. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach_test::*`
   - Examples:
     - `src/main.rs:237`
256. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:239`
257. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries_test::*`
   - Examples:
     - `src/main.rs:241`
258. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:243`
259. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys_test::*`
   - Examples:
     - `src/main.rs:245`
260. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_test::*`
   - Examples:
     - `src/main.rs:249`
261. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:251`
262. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_test::*`
   - Examples:
     - `src/main.rs:253`
263. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:255`
264. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_reference_batch_test::*`
   - Examples:
     - `src/main.rs:257`
265. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_remeda_debounce_test::*`
   - Examples:
     - `src/main.rs:259`
266. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_test::*`
   - Examples:
     - `src/main.rs:261`
267. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:263`
268. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:265`
269. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp_test::*`
   - Examples:
     - `src/main.rs:267`
270. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy_test::*`
   - Examples:
     - `src/main.rs:269`
271. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasAtLeast_test::*`
   - Examples:
     - `src/main.rs:273`
272. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:275`
273. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp_test::*`
   - Examples:
     - `src/main.rs:277`
274. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:279`
275. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject_test::*`
   - Examples:
     - `src/main.rs:281`
276. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `heap_test::*`
   - Examples:
     - `src/main.rs:285`
277. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:287`
278. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity_test::*`
   - Examples:
     - `src/main.rs:289`
279. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:291`
280. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy_test::*`
   - Examples:
     - `src/main.rs:293`
281. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:295`
282. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:297`
283. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith_test::*`
   - Examples:
     - `src/main.rs:299`
284. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection_test::*`
   - Examples:
     - `src/main.rs:301`
285. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:303`
286. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert_test::*`
   - Examples:
     - `src/main.rs:305`
287. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:307`
288. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray_test::*`
   - Examples:
     - `src/main.rs:309`
289. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:311`
290. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt_test::*`
   - Examples:
     - `src/main.rs:313`
291. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:315`
292. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean_test::*`
   - Examples:
     - `src/main.rs:317`
293. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:319`
294. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate_test::*`
   - Examples:
     - `src/main.rs:321`
295. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDeepEqual_test::*`
   - Examples:
     - `src/main.rs:325`
296. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:327`
297. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined_test::*`
   - Examples:
     - `src/main.rs:329`
298. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:331`
299. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty_test::*`
   - Examples:
     - `src/main.rs:333`
300. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:335`
301. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish_test::*`
   - Examples:
     - `src/main.rs:337`
302. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:339`
303. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError_test::*`
   - Examples:
     - `src/main.rs:341`
304. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:343`
305. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction_test::*`
   - Examples:
     - `src/main.rs:345`
306. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:347`
307. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn_test::*`
   - Examples:
     - `src/main.rs:349`
308. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:351`
309. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull_test::*`
   - Examples:
     - `src/main.rs:353`
310. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:355`
311. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish_test::*`
   - Examples:
     - `src/main.rs:357`
312. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:359`
313. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot_test::*`
   - Examples:
     - `src/main.rs:361`
314. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:363`
315. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish_test::*`
   - Examples:
     - `src/main.rs:365`
316. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:367`
317. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber_test::*`
   - Examples:
     - `src/main.rs:369`
318. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:371`
319. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType_test::*`
   - Examples:
     - `src/main.rs:373`
320. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPlainObject_test::*`
   - Examples:
     - `src/main.rs:377`
321. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:379`
322. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise_test::*`
   - Examples:
     - `src/main.rs:381`
323. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:383`
324. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual_test::*`
   - Examples:
     - `src/main.rs:385`
325. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:387`
326. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual_test::*`
   - Examples:
     - `src/main.rs:389`
327. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:391`
328. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString_test::*`
   - Examples:
     - `src/main.rs:393`
329. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:395`
330. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol_test::*`
   - Examples:
     - `src/main.rs:397`
331. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:399`
332. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy_test::*`
   - Examples:
     - `src/main.rs:401`
333. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:403`
334. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join_test::*`
   - Examples:
     - `src/main.rs:405`
335. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:407`
336. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys_test::*`
   - Examples:
     - `src/main.rs:409`
337. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:411`
338. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last_test::*`
   - Examples:
     - `src/main.rs:413`
339. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `lazyInvocationCounter::*`
   - Examples:
     - `src/main.rs:417`
340. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:419`
341. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length_test::*`
   - Examples:
     - `src/main.rs:421`
342. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:425`
343. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys_test::*`
   - Examples:
     - `src/main.rs:427`
344. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:429`
345. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj_test::*`
   - Examples:
     - `src/main.rs:431`
346. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:433`
347. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues_test::*`
   - Examples:
     - `src/main.rs:435`
348. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:437`
349. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback_test::*`
   - Examples:
     - `src/main.rs:439`
350. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map_test::*`
   - Examples:
     - `src/main.rs:441`
351. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:443`
352. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:445`
353. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy_test::*`
   - Examples:
     - `src/main.rs:447`
354. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean_test::*`
   - Examples:
     - `src/main.rs:449`
355. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:451`
356. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median_test::*`
   - Examples:
     - `src/main.rs:453`
357. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:455`
358. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:457`
359. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll_test::*`
   - Examples:
     - `src/main.rs:459`
360. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:461`
361. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep_test::*`
   - Examples:
     - `src/main.rs:463`
362. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge_test::*`
   - Examples:
     - `src/main.rs:465`
363. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:467`
364. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply_test::*`
   - Examples:
     - `src/main.rs:469`
365. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:471`
366. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy_test::*`
   - Examples:
     - `src/main.rs:473`
367. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:475`
368. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf_test::*`
   - Examples:
     - `src/main.rs:477`
369. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:479`
370. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:481`
371. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy_test::*`
   - Examples:
     - `src/main.rs:483`
372. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit_test::*`
   - Examples:
     - `src/main.rs:485`
373. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:487`
374. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once_test::*`
   - Examples:
     - `src/main.rs:489`
375. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:491`
376. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only_test::*`
   - Examples:
     - `src/main.rs:493`
377. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:495`
378. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind_test::*`
   - Examples:
     - `src/main.rs:497`
379. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:499`
380. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind_test::*`
   - Examples:
     - `src/main.rs:501`
381. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:503`
382. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition_test::*`
   - Examples:
     - `src/main.rs:505`
383. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:507`
384. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr_test::*`
   - Examples:
     - `src/main.rs:509`
385. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:511`
386. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:513`
387. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy_test::*`
   - Examples:
     - `src/main.rs:515`
388. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick_test::*`
   - Examples:
     - `src/main.rs:517`
389. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pipe_test::*`
   - Examples:
     - `src/main.rs:521`
390. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:523`
391. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped_test::*`
   - Examples:
     - `src/main.rs:525`
392. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:527`
393. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product_test::*`
   - Examples:
     - `src/main.rs:529`
394. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:531`
395. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop_test::*`
   - Examples:
     - `src/main.rs:533`
396. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:535`
397. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject_test::*`
   - Examples:
     - `src/main.rs:537`
398. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry::*`
   - Examples:
     - `src/main.rs:539`
399. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purryFromLazy_test::*`
   - Examples:
     - `src/main.rs:543`
400. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry_test::*`
   - Examples:
     - `src/main.rs:549`
401. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt_test::*`
   - Examples:
     - `src/main.rs:555`
402. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger_test::*`
   - Examples:
     - `src/main.rs:559`
403. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:561`
404. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString_test::*`
   - Examples:
     - `src/main.rs:563`
405. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:565`
406. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range_test::*`
   - Examples:
     - `src/main.rs:567`
407. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:569`
408. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy_test::*`
   - Examples:
     - `src/main.rs:571`
409. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce_test::*`
   - Examples:
     - `src/main.rs:575`
410. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:577`
411. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse_test::*`
   - Examples:
     - `src/main.rs:579`
412. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:581`
413. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round_test::*`
   - Examples:
     - `src/main.rs:583`
414. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:585`
415. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample_test::*`
   - Examples:
     - `src/main.rs:587`
416. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:589`
417. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:591`
418. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath_test::*`
   - Examples:
     - `src/main.rs:593`
419. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set_test::*`
   - Examples:
     - `src/main.rs:595`
420. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:597`
421. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle_test::*`
   - Examples:
     - `src/main.rs:599`
422. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:603`
423. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString_test::*`
   - Examples:
     - `src/main.rs:605`
424. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:607`
425. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:609`
426. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy_test::*`
   - Examples:
     - `src/main.rs:611`
427. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort_test::*`
   - Examples:
     - `src/main.rs:613`
428. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:615`
429. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy_test::*`
   - Examples:
     - `src/main.rs:619`
430. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:621`
431. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith_test::*`
   - Examples:
     - `src/main.rs:623`
432. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex_test::*`
   - Examples:
     - `src/main.rs:625`
433. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:627`
434. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy_test::*`
   - Examples:
     - `src/main.rs:631`
435. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex_test::*`
   - Examples:
     - `src/main.rs:633`
436. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:635`
437. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice_test::*`
   - Examples:
     - `src/main.rs:637`
438. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:639`
439. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:641`
440. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt_test::*`
   - Examples:
     - `src/main.rs:643`
441. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:645`
442. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen_test::*`
   - Examples:
     - `src/main.rs:647`
443. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split_test::*`
   - Examples:
     - `src/main.rs:649`
444. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:651`
445. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:653`
446. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith_test::*`
   - Examples:
     - `src/main.rs:655`
447. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:657`
448. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath_test::*`
   - Examples:
     - `src/main.rs:659`
449. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:661`
450. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract_test::*`
   - Examples:
     - `src/main.rs:663`
451. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:667`
452. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy_test::*`
   - Examples:
     - `src/main.rs:669`
453. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sum_test::*`
   - Examples:
     - `src/main.rs:671`
454. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:675`
455. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices_test::*`
   - Examples:
     - `src/main.rs:677`
456. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:679`
457. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps_test::*`
   - Examples:
     - `src/main.rs:681`
458. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:683`
459. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:685`
460. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy_test::*`
   - Examples:
     - `src/main.rs:687`
461. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:689`
462. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:691`
463. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile_test::*`
   - Examples:
     - `src/main.rs:693`
464. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast_test::*`
   - Examples:
     - `src/main.rs:695`
465. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:697`
466. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile_test::*`
   - Examples:
     - `src/main.rs:699`
467. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take_test::*`
   - Examples:
     - `src/main.rs:701`
468. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:703`
469. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap_test::*`
   - Examples:
     - `src/main.rs:705`
470. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times_test::*`
   - Examples:
     - `src/main.rs:709`
471. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:711`
472. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase_test::*`
   - Examples:
     - `src/main.rs:713`
473. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:715`
474. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase_test::*`
   - Examples:
     - `src/main.rs:717`
475. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:719`
476. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase_test::*`
   - Examples:
     - `src/main.rs:721`
477. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:725`
478. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase_test::*`
   - Examples:
     - `src/main.rs:727`
479. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:729`
480. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase_test::*`
   - Examples:
     - `src/main.rs:731`
481. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:733`
482. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase_test::*`
   - Examples:
     - `src/main.rs:735`
483. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:737`
484. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate_test::*`
   - Examples:
     - `src/main.rs:739`
485. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `typesDataProvider::*`
   - Examples:
     - `src/main.rs:741`
486. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:743`
487. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize_test::*`
   - Examples:
     - `src/main.rs:745`
488. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:749`
489. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy_test::*`
   - Examples:
     - `src/main.rs:751`
490. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:753`
491. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith_test::*`
   - Examples:
     - `src/main.rs:755`
492. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique_test::*`
   - Examples:
     - `src/main.rs:757`
493. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:761`
494. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values_test::*`
   - Examples:
     - `src/main.rs:763`
495. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:765`
496. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when_test::*`
   - Examples:
     - `src/main.rs:767`
497. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `withPrecision_test::*`
   - Examples:
     - `src/main.rs:771`
498. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `words_test::*`
   - Examples:
     - `src/main.rs:775`
499. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:777`
500. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:779`
501. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith_test::*`
   - Examples:
     - `src/main.rs:781`
502. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip_test::*`
   - Examples:
     - `src/main.rs:783`
503. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:88`
504. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 2 previous errors; 913 warnings emitted
```
