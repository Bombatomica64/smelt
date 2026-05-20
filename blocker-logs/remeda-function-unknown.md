# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `162`
- Warnings: `705`

## Summary By Code

1. **warning** `unused_imports` - 362 diagnostics
2. **warning** `unused_assignments` - 196 diagnostics
3. **error** `E0282` - 93 diagnostics
4. **warning** `unreachable_code` - 64 diagnostics
5. **error** `E0004` - 58 diagnostics
6. **warning** `unused_mut` - 47 diagnostics
7. **warning** `unused_parens` - 36 diagnostics
8. **error** `E0790` - 9 diagnostics
9. **error** `E0308` - 2 diagnostics

## Groups

1. **error** `E0282` - 93 occurrences
   - Message: type annotations needed for `std::option::Option<_>`
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
3. **error** `E0004` - 54 occurrences
   - Message: non-exhaustive patterns: `SmeltUnknown::Function(_)` not covered
   - Examples:
     - `src/capitalize.rs:16`
     - `src/capitalize.rs:36`
     - `src/countBy.rs:68`
     - `src/countBy.rs:69`
     - `src/debounce.rs:40`
4. **warning** `unused_mut` - 47 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/countBy.rs:34`
     - `src/dropFirstBy.rs:7`
     - `src/evolve.rs:39`
     - `src/firstBy.rs:7`
     - `src/fromKeys.rs:25`
5. **warning** `unused_assignments` - 19 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/evolve.rs:18`
     - `src/forEachObj.rs:17`
     - `src/fromKeys.rs:36`
     - `src/groupBy.rs:17`
     - `src/groupByProp.rs:17`
6. **warning** `unused_assignments` - 19 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/difference.rs:14`
     - `src/evolve.rs:51`
     - `src/forEachObj.rs:18`
     - `src/hasSubObject.rs:18`
     - `src/intersection.rs:14`
7. **warning** `unused_assignments` - 17 occurrences
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/clone.rs:125`
     - `src/countBy.rs:18`
     - `src/dropFirstBy.rs:19`
     - `src/dropWhile.rs:18`
     - `src/findLast.rs:16`
8. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:40`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
9. **warning** `unused_assignments` - 13 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/clone.rs:124`
     - `src/countBy.rs:17`
     - `src/dropWhile.rs:17`
     - `src/fromKeys.rs:17`
     - `src/indexBy.rs:17`
10. **error** `E0790` - 9 occurrences
   - Message: cannot call associated function on trait without specifying the corresponding `impl` type
   - Examples:
     - `src/drop.rs:12`
     - `src/filter.rs:12`
     - `src/find.rs:13`
     - `src/first.rs:13`
     - `src/flatMap.rs:12`
11. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:85`
     - `src/allPass_test.rs:86`
     - `src/anyPass_test.rs:85`
     - `src/anyPass_test.rs:86`
     - `src/purryOrderRules.rs:206`
12. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/conditional.rs:59`
     - `src/dropFirstBy.rs:18`
     - `src/firstBy.rs:17`
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
13. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
     - `src/times.rs:18`
14. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:68`
     - `src/debounce.rs:116`
     - `src/debounce.rs:195`
     - `src/debounce.rs:190`
     - `src/debounce.rs:218`
15. **error** `E0004` - 4 occurrences
   - Message: non-exhaustive patterns: `&SmeltUnknown::Function(_)` not covered
   - Examples:
     - `src/join.rs:16`
     - `src/swapIndices.rs:91`
     - `src/toKebabCase.rs:21`
     - `src/toSnakeCase.rs:21`
16. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
17. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:7`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_test.rs:7`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
18. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `out` is never read
   - Examples:
     - `src/dropFirstBy.rs:17`
     - `src/evolve.rs:16`
     - `src/product.rs:16`
     - `src/sum.rs:16`
19. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `remaining` is never read
   - Examples:
     - `src/difference.rs:13`
     - `src/intersection.rs:13`
     - `src/take.rs:17`
20. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
21. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
22. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
23. **error** `E0308` - 2 occurrences
   - Message: mismatched types
   - Examples:
     - `src/allPass.rs:11`
     - `src/anyPass.rs:11`
24. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
25. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:8`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:8`
26. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `heap` is never read
   - Examples:
     - `src/dropFirstBy.rs:16`
     - `src/takeFirstBy.rs:16`
27. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `items` is never read
   - Examples:
     - `src/groupBy.rs:18`
     - `src/groupByProp.rs:18`
28. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `length` is never read
   - Examples:
     - `src/range.rs:18`
     - `src/times.rs:16`
29. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `pivot_index` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:7`
     - `src/quickSelect.rs:7`
30. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/randomBigInt.rs:13`
     - `src/main.rs:1182`
31. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rounded` is never read
   - Examples:
     - `src/range.rs:55`
     - `src/withPrecision.rs:46`
32. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
     - `src/meanBy.rs:16`
33. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
     - `src/conditional.rs:79`
34. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
35. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `value_b` is never read
   - Examples:
     - `src/isShallowEqual.rs:19`
     - `src/isShallowEqual.rs:127`
36. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
     - `src/conditional.rs:78`
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
   - Message: value assigned to `category` is never read
   - Examples:
     - `src/countBy.rs:19`
45. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `chunks` is never read
   - Examples:
     - `src/main.rs:1181`
46. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
47. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
48. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
49. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `count` is never read
   - Examples:
     - `src/countBy.rs:20`
50. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_first` is never read
   - Examples:
     - `src/firstBy.rs:16`
51. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
52. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current` is never read
   - Examples:
     - `src/conditional.rs:37`
53. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_item` is never read
   - Examples:
     - `src/isDeepEqual.rs:246`
54. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
55. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data` is never read
   - Examples:
     - `src/purryFromLazy.rs:7`
56. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `destination_value` is never read
   - Examples:
     - `src/mergeDeep.rs:17`
57. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:75`
58. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `effective_index` is never read
   - Examples:
     - `src/splitAt.rs:16`
59. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `element` is never read
   - Examples:
     - `src/mapToObj.rs:18`
60. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/range.rs:17`
61. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `excess_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:10`
62. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `first_child_index` is never read
   - Examples:
     - `src/heap.rs:46`
63. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `head` is never read
   - Examples:
     - `src/heap.rs:22`
64. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `idx` is never read
   - Examples:
     - `src/clone.rs:17`
65. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_found` is never read
   - Examples:
     - `src/isDeepEqual.rs:247`
66. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `k` is never read
   - Examples:
     - `src/clone.rs:91`
67. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `keys` is never read
   - Examples:
     - `src/isShallowEqual.rs:16`
68. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition_1` is never read
   - Examples:
     - `src/purryFromLazy.rs:10`
69. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
70. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `left` is never read
   - Examples:
     - `src/drop.rs:17`
71. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_key` is never read
   - Examples:
     - `src/mapKeys.rs:19`
72. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_value` is never read
   - Examples:
     - `src/mapValues.rs:19`
73. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:8`
74. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:9`
75. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:154`
76. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_projection` is never read
   - Examples:
     - `src/purryOrderRules.rs:153`
77. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `next_comparer` is never read
   - Examples:
     - `src/purryOrderRules.rs:77`
78. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `now` is never read
   - Examples:
     - `src/funnel.rs:74`
79. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `other_copy` is never read
   - Examples:
     - `src/isDeepEqual.rs:245`
80. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `other_item` is never read
   - Examples:
     - `src/isDeepEqual.rs:250`
81. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `pivot` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:8`
82. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_a` is never read
   - Examples:
     - `src/swapIndices.rs:16`
83. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_b` is never read
   - Examples:
     - `src/swapIndices.rs:17`
84. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `previous_head` is never read
   - Examples:
     - `src/dropFirstBy.rs:20`
85. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
86. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `proto` is never read
   - Examples:
     - `src/isPlainObject.rs:7`
87. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prototype` is never read
   - Examples:
     - `src/clone.rs:16`
88. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `rand` is never read
   - Examples:
     - `src/shuffle.rs:16`
89. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:11`
90. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_char` is never read
   - Examples:
     - `src/randomString.rs:16`
91. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_index` is never read
   - Examples:
     - `src/sample.rs:18`
92. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `range` is never read
   - Examples:
     - `src/randomBigInt.rs:7`
93. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `raw` is never read
   - Examples:
     - `src/randomBigInt.rs:12`
94. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `res` is never read
   - Examples:
     - `src/times.rs:17`
95. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sample_indices` is never read
   - Examples:
     - `src/sample.rs:17`
96. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `second_child_index` is never read
   - Examples:
     - `src/heap.rs:48`
97. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_exponent` is never read
   - Examples:
     - `src/withPrecision.rs:7`
98. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value_as_string` is never read
   - Examples:
     - `src/withPrecision.rs:8`
99. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value` is never read
   - Examples:
     - `src/withPrecision.rs:45`
100. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `source_value` is never read
   - Examples:
     - `src/mergeDeep.rs:18`
101. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/main.rs:1187`
102. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `step` is never read
   - Examples:
     - `src/range.rs:16`
103. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `swap_index` is never read
   - Examples:
     - `src/heap.rs:47`
104. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `v` is never read
   - Examples:
     - `src/clone.rs:92`
105. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_a` is never read
   - Examples:
     - `src/isShallowEqual.rs:18`
106. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
107. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
108. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
109. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
110. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
111. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
112. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
113. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
114. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
115. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
116. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
117. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
118. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
119. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
120. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
121. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
122. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
123. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
124. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
125. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
126. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
127. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
128. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
129. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
130. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
131. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
132. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
133. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
134. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
135. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
136. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
137. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
138. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
139. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
140. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
141. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
142. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
143. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
144. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp_test::*`
   - Examples:
     - `src/main.rs:83`
145. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add_test::*`
   - Examples:
     - `src/main.rs:85`
146. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:87`
147. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass_test::*`
   - Examples:
     - `src/main.rs:89`
148. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:91`
149. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass_test::*`
   - Examples:
     - `src/main.rs:93`
150. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `binarySearchCutoffIndex_test::*`
   - Examples:
     - `src/main.rs:97`
151. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:99`
152. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize_test::*`
   - Examples:
     - `src/main.rs:101`
153. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:103`
154. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil_test::*`
   - Examples:
     - `src/main.rs:105`
155. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:107`
156. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk_test::*`
   - Examples:
     - `src/main.rs:109`
157. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:111`
158. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp_test::*`
   - Examples:
     - `src/main.rs:113`
159. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:115`
160. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone_test::*`
   - Examples:
     - `src/main.rs:117`
161. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:119`
162. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat_test::*`
   - Examples:
     - `src/main.rs:121`
163. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:123`
164. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional_test::*`
   - Examples:
     - `src/main.rs:125`
165. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:127`
166. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant_test::*`
   - Examples:
     - `src/main.rs:129`
167. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:131`
168. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy_test::*`
   - Examples:
     - `src/main.rs:133`
169. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:135`
170. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce_test::*`
   - Examples:
     - `src/main.rs:137`
171. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:139`
172. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo_test::*`
   - Examples:
     - `src/main.rs:141`
173. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:143`
174. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:145`
175. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith_test::*`
   - Examples:
     - `src/main.rs:147`
176. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference_test::*`
   - Examples:
     - `src/main.rs:149`
177. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:151`
178. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide_test::*`
   - Examples:
     - `src/main.rs:153`
179. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:155`
180. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing_test::*`
   - Examples:
     - `src/main.rs:157`
181. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:159`
182. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:161`
183. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy_test::*`
   - Examples:
     - `src/main.rs:163`
184. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:165`
185. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:167`
186. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile_test::*`
   - Examples:
     - `src/main.rs:169`
187. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast_test::*`
   - Examples:
     - `src/main.rs:171`
188. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:173`
189. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile_test::*`
   - Examples:
     - `src/main.rs:175`
190. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop_test::*`
   - Examples:
     - `src/main.rs:177`
191. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:179`
192. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith_test::*`
   - Examples:
     - `src/main.rs:181`
193. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:183`
194. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries_test::*`
   - Examples:
     - `src/main.rs:185`
195. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:187`
196. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve_test::*`
   - Examples:
     - `src/main.rs:189`
197. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:191`
198. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter_test::*`
   - Examples:
     - `src/main.rs:193`
199. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:195`
200. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:197`
201. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex_test::*`
   - Examples:
     - `src/main.rs:199`
202. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:201`
203. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:203`
204. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex_test::*`
   - Examples:
     - `src/main.rs:205`
205. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast_test::*`
   - Examples:
     - `src/main.rs:207`
206. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find_test::*`
   - Examples:
     - `src/main.rs:209`
207. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:211`
208. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:213`
209. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy_test::*`
   - Examples:
     - `src/main.rs:215`
210. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first_test::*`
   - Examples:
     - `src/main.rs:217`
211. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:219`
212. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:221`
213. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap_test::*`
   - Examples:
     - `src/main.rs:223`
214. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat_test::*`
   - Examples:
     - `src/main.rs:225`
215. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:227`
216. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor_test::*`
   - Examples:
     - `src/main.rs:229`
217. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:231`
218. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:233`
219. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj_test::*`
   - Examples:
     - `src/main.rs:235`
220. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach_test::*`
   - Examples:
     - `src/main.rs:237`
221. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:239`
222. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries_test::*`
   - Examples:
     - `src/main.rs:241`
223. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:243`
224. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys_test::*`
   - Examples:
     - `src/main.rs:245`
225. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_test::*`
   - Examples:
     - `src/main.rs:249`
226. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:251`
227. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_test::*`
   - Examples:
     - `src/main.rs:253`
228. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:255`
229. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_reference_batch_test::*`
   - Examples:
     - `src/main.rs:257`
230. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_remeda_debounce_test::*`
   - Examples:
     - `src/main.rs:259`
231. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_test::*`
   - Examples:
     - `src/main.rs:261`
232. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:263`
233. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:265`
234. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp_test::*`
   - Examples:
     - `src/main.rs:267`
235. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy_test::*`
   - Examples:
     - `src/main.rs:269`
236. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasAtLeast_test::*`
   - Examples:
     - `src/main.rs:273`
237. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:275`
238. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp_test::*`
   - Examples:
     - `src/main.rs:277`
239. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:279`
240. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject_test::*`
   - Examples:
     - `src/main.rs:281`
241. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `heap_test::*`
   - Examples:
     - `src/main.rs:285`
242. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:287`
243. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity_test::*`
   - Examples:
     - `src/main.rs:289`
244. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:291`
245. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy_test::*`
   - Examples:
     - `src/main.rs:293`
246. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:295`
247. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:297`
248. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith_test::*`
   - Examples:
     - `src/main.rs:299`
249. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection_test::*`
   - Examples:
     - `src/main.rs:301`
250. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:303`
251. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert_test::*`
   - Examples:
     - `src/main.rs:305`
252. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:307`
253. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray_test::*`
   - Examples:
     - `src/main.rs:309`
254. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:311`
255. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt_test::*`
   - Examples:
     - `src/main.rs:313`
256. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:315`
257. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean_test::*`
   - Examples:
     - `src/main.rs:317`
258. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:319`
259. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate_test::*`
   - Examples:
     - `src/main.rs:321`
260. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDeepEqual_test::*`
   - Examples:
     - `src/main.rs:325`
261. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:327`
262. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined_test::*`
   - Examples:
     - `src/main.rs:329`
263. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:331`
264. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty_test::*`
   - Examples:
     - `src/main.rs:333`
265. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:335`
266. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish_test::*`
   - Examples:
     - `src/main.rs:337`
267. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:339`
268. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError_test::*`
   - Examples:
     - `src/main.rs:341`
269. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:343`
270. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction_test::*`
   - Examples:
     - `src/main.rs:345`
271. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:347`
272. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn_test::*`
   - Examples:
     - `src/main.rs:349`
273. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:351`
274. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull_test::*`
   - Examples:
     - `src/main.rs:353`
275. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:355`
276. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish_test::*`
   - Examples:
     - `src/main.rs:357`
277. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:359`
278. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot_test::*`
   - Examples:
     - `src/main.rs:361`
279. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:363`
280. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish_test::*`
   - Examples:
     - `src/main.rs:365`
281. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:367`
282. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber_test::*`
   - Examples:
     - `src/main.rs:369`
283. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:371`
284. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType_test::*`
   - Examples:
     - `src/main.rs:373`
285. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPlainObject_test::*`
   - Examples:
     - `src/main.rs:377`
286. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:379`
287. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise_test::*`
   - Examples:
     - `src/main.rs:381`
288. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:383`
289. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual_test::*`
   - Examples:
     - `src/main.rs:385`
290. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:387`
291. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual_test::*`
   - Examples:
     - `src/main.rs:389`
292. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:391`
293. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString_test::*`
   - Examples:
     - `src/main.rs:393`
294. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:395`
295. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol_test::*`
   - Examples:
     - `src/main.rs:397`
296. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:399`
297. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy_test::*`
   - Examples:
     - `src/main.rs:401`
298. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:403`
299. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join_test::*`
   - Examples:
     - `src/main.rs:405`
300. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:407`
301. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys_test::*`
   - Examples:
     - `src/main.rs:409`
302. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:411`
303. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last_test::*`
   - Examples:
     - `src/main.rs:413`
304. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `lazyInvocationCounter::*`
   - Examples:
     - `src/main.rs:417`
305. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:419`
306. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length_test::*`
   - Examples:
     - `src/main.rs:421`
307. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:425`
308. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys_test::*`
   - Examples:
     - `src/main.rs:427`
309. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:429`
310. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj_test::*`
   - Examples:
     - `src/main.rs:431`
311. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:433`
312. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues_test::*`
   - Examples:
     - `src/main.rs:435`
313. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:437`
314. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback_test::*`
   - Examples:
     - `src/main.rs:439`
315. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map_test::*`
   - Examples:
     - `src/main.rs:441`
316. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:443`
317. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:445`
318. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy_test::*`
   - Examples:
     - `src/main.rs:447`
319. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean_test::*`
   - Examples:
     - `src/main.rs:449`
320. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:451`
321. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median_test::*`
   - Examples:
     - `src/main.rs:453`
322. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:455`
323. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:457`
324. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll_test::*`
   - Examples:
     - `src/main.rs:459`
325. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:461`
326. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep_test::*`
   - Examples:
     - `src/main.rs:463`
327. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge_test::*`
   - Examples:
     - `src/main.rs:465`
328. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:467`
329. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply_test::*`
   - Examples:
     - `src/main.rs:469`
330. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:471`
331. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy_test::*`
   - Examples:
     - `src/main.rs:473`
332. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:475`
333. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf_test::*`
   - Examples:
     - `src/main.rs:477`
334. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:479`
335. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:481`
336. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy_test::*`
   - Examples:
     - `src/main.rs:483`
337. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit_test::*`
   - Examples:
     - `src/main.rs:485`
338. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:487`
339. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once_test::*`
   - Examples:
     - `src/main.rs:489`
340. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:491`
341. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only_test::*`
   - Examples:
     - `src/main.rs:493`
342. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:495`
343. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind_test::*`
   - Examples:
     - `src/main.rs:497`
344. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:499`
345. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind_test::*`
   - Examples:
     - `src/main.rs:501`
346. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:503`
347. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition_test::*`
   - Examples:
     - `src/main.rs:505`
348. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:507`
349. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr_test::*`
   - Examples:
     - `src/main.rs:509`
350. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:511`
351. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:513`
352. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy_test::*`
   - Examples:
     - `src/main.rs:515`
353. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick_test::*`
   - Examples:
     - `src/main.rs:517`
354. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pipe_test::*`
   - Examples:
     - `src/main.rs:521`
355. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:523`
356. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped_test::*`
   - Examples:
     - `src/main.rs:525`
357. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:527`
358. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product_test::*`
   - Examples:
     - `src/main.rs:529`
359. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:531`
360. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop_test::*`
   - Examples:
     - `src/main.rs:533`
361. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:535`
362. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject_test::*`
   - Examples:
     - `src/main.rs:537`
363. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry::*`
   - Examples:
     - `src/main.rs:539`
364. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purryFromLazy_test::*`
   - Examples:
     - `src/main.rs:543`
365. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry_test::*`
   - Examples:
     - `src/main.rs:549`
366. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt_test::*`
   - Examples:
     - `src/main.rs:555`
367. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger_test::*`
   - Examples:
     - `src/main.rs:559`
368. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:561`
369. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString_test::*`
   - Examples:
     - `src/main.rs:563`
370. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:565`
371. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range_test::*`
   - Examples:
     - `src/main.rs:567`
372. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:569`
373. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy_test::*`
   - Examples:
     - `src/main.rs:571`
374. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce_test::*`
   - Examples:
     - `src/main.rs:575`
375. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:577`
376. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse_test::*`
   - Examples:
     - `src/main.rs:579`
377. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:581`
378. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round_test::*`
   - Examples:
     - `src/main.rs:583`
379. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:585`
380. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample_test::*`
   - Examples:
     - `src/main.rs:587`
381. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:589`
382. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:591`
383. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath_test::*`
   - Examples:
     - `src/main.rs:593`
384. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set_test::*`
   - Examples:
     - `src/main.rs:595`
385. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:597`
386. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle_test::*`
   - Examples:
     - `src/main.rs:599`
387. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:603`
388. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString_test::*`
   - Examples:
     - `src/main.rs:605`
389. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:607`
390. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:609`
391. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy_test::*`
   - Examples:
     - `src/main.rs:611`
392. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort_test::*`
   - Examples:
     - `src/main.rs:613`
393. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:615`
394. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy_test::*`
   - Examples:
     - `src/main.rs:619`
395. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:621`
396. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith_test::*`
   - Examples:
     - `src/main.rs:623`
397. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex_test::*`
   - Examples:
     - `src/main.rs:625`
398. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:627`
399. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy_test::*`
   - Examples:
     - `src/main.rs:631`
400. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex_test::*`
   - Examples:
     - `src/main.rs:633`
401. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:635`
402. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice_test::*`
   - Examples:
     - `src/main.rs:637`
403. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:639`
404. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:641`
405. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt_test::*`
   - Examples:
     - `src/main.rs:643`
406. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:645`
407. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen_test::*`
   - Examples:
     - `src/main.rs:647`
408. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split_test::*`
   - Examples:
     - `src/main.rs:649`
409. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:651`
410. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:653`
411. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith_test::*`
   - Examples:
     - `src/main.rs:655`
412. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:657`
413. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath_test::*`
   - Examples:
     - `src/main.rs:659`
414. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:661`
415. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract_test::*`
   - Examples:
     - `src/main.rs:663`
416. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:667`
417. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy_test::*`
   - Examples:
     - `src/main.rs:669`
418. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sum_test::*`
   - Examples:
     - `src/main.rs:671`
419. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:675`
420. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices_test::*`
   - Examples:
     - `src/main.rs:677`
421. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:679`
422. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps_test::*`
   - Examples:
     - `src/main.rs:681`
423. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:683`
424. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:685`
425. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy_test::*`
   - Examples:
     - `src/main.rs:687`
426. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:689`
427. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:691`
428. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile_test::*`
   - Examples:
     - `src/main.rs:693`
429. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast_test::*`
   - Examples:
     - `src/main.rs:695`
430. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:697`
431. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile_test::*`
   - Examples:
     - `src/main.rs:699`
432. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take_test::*`
   - Examples:
     - `src/main.rs:701`
433. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:703`
434. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap_test::*`
   - Examples:
     - `src/main.rs:705`
435. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times_test::*`
   - Examples:
     - `src/main.rs:709`
436. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:711`
437. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase_test::*`
   - Examples:
     - `src/main.rs:713`
438. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:715`
439. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase_test::*`
   - Examples:
     - `src/main.rs:717`
440. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:719`
441. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase_test::*`
   - Examples:
     - `src/main.rs:721`
442. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:725`
443. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase_test::*`
   - Examples:
     - `src/main.rs:727`
444. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:729`
445. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase_test::*`
   - Examples:
     - `src/main.rs:731`
446. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:733`
447. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase_test::*`
   - Examples:
     - `src/main.rs:735`
448. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:737`
449. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate_test::*`
   - Examples:
     - `src/main.rs:739`
450. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `typesDataProvider::*`
   - Examples:
     - `src/main.rs:741`
451. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:743`
452. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize_test::*`
   - Examples:
     - `src/main.rs:745`
453. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:749`
454. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy_test::*`
   - Examples:
     - `src/main.rs:751`
455. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:753`
456. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith_test::*`
   - Examples:
     - `src/main.rs:755`
457. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique_test::*`
   - Examples:
     - `src/main.rs:757`
458. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:761`
459. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values_test::*`
   - Examples:
     - `src/main.rs:763`
460. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:765`
461. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when_test::*`
   - Examples:
     - `src/main.rs:767`
462. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `withPrecision_test::*`
   - Examples:
     - `src/main.rs:771`
463. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `words_test::*`
   - Examples:
     - `src/main.rs:775`
464. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:777`
465. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:779`
466. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith_test::*`
   - Examples:
     - `src/main.rs:781`
467. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip_test::*`
   - Examples:
     - `src/main.rs:783`
468. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:88`
469. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 162 previous errors; 705 warnings emitted
```
