# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `1226`

## Summary By Code

1. **warning** `non_snake_case` - 411 diagnostics
2. **warning** `unused_imports` - 362 diagnostics
3. **warning** `unused_mut` - 283 diagnostics
4. **warning** `unreachable_code` - 64 diagnostics
5. **warning** `unused_assignments` - 64 diagnostics
6. **warning** `unused_parens` - 36 diagnostics
7. **warning** `unused_must_use` - 5 diagnostics
8. **warning** `path_statements` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 283 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/add.rs:11`
     - `src/addProp.rs:11`
     - `src/allPass.rs:11`
     - `src/anyPass.rs:11`
     - `src/capitalize.rs:11`
2. **warning** `unreachable_code` - 64 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/binarySearchCutoffIndex.rs:33`
     - `src/clone.rs:119`
     - `src/clone.rs:153`
     - `src/conditional.rs:53`
     - `src/countBy.rs:70`
3. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:40`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
4. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:85`
     - `src/allPass_test.rs:86`
     - `src/anyPass_test.rs:85`
     - `src/anyPass_test.rs:86`
     - `src/purryOrderRules.rs:206`
5. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/pipe.rs:10`
     - `src/pipe.rs:182`
     - `src/pipe.rs:255`
     - `src/randomBigInt.rs:93`
     - `src/truncate.rs:31`
6. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:218`
     - `src/debounce.rs:195`
     - `src/debounce.rs:190`
     - `src/debounce.rs:116`
     - `src/debounce.rs:68`
7. **warning** `unused_must_use` - 5 occurrences
   - Message: unused return value of `clone` that must be used
   - Examples:
     - `src/doNothing.rs:9`
     - `src/funnel_lodash_debounce_test.rs:92`
     - `src/once.rs:13`
     - `src/once.rs:13`
     - `src/take.rs:31`
8. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
9. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:7`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_test.rs:7`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
10. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:160`
     - `src/truncate.rs:145`
     - `src/truncate.rs:119`
     - `src/truncate.rs:104`
11. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
12. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
13. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
14. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
16. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:8`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:8`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:130`
     - `src/truncate.rs:89`
18. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/fromKeys.rs:36`
     - `src/omit.rs:129`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:151`
     - `src/truncate.rs:110`
20. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
21. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
22. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ArrayAt` should have a snake case name
   - Examples:
     - `src/ArrayAt.rs:6`
23. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ArrayRequiredPrefix` should have a snake case name
   - Examples:
     - `src/ArrayRequiredPrefix.rs:6`
24. **warning** `non_snake_case` - 1 occurrence
   - Message: function `BoundedPartial` should have a snake case name
   - Examples:
     - `src/BoundedPartial.rs:6`
25. **warning** `non_snake_case` - 1 occurrence
   - Message: function `BrandedReturn` should have a snake case name
   - Examples:
     - `src/BrandedReturn.rs:6`
26. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ClampedIntegerSubtract` should have a snake case name
   - Examples:
     - `src/ClampedIntegerSubtract.rs:6`
27. **warning** `non_snake_case` - 1 occurrence
   - Message: function `CoercedArray` should have a snake case name
   - Examples:
     - `src/CoercedArray.rs:6`
28. **warning** `non_snake_case` - 1 occurrence
   - Message: function `CompareFunction` should have a snake case name
   - Examples:
     - `src/CompareFunction.rs:6`
29. **warning** `non_snake_case` - 1 occurrence
   - Message: function `Deduped` should have a snake case name
   - Examples:
     - `src/Deduped.rs:6`
30. **warning** `non_snake_case` - 1 occurrence
   - Message: function `DisjointUnionFields` should have a snake case name
   - Examples:
     - `src/DisjointUnionFields.rs:6`
31. **warning** `non_snake_case` - 1 occurrence
   - Message: function `EnumerableStringKeyOf` should have a snake case name
   - Examples:
     - `src/EnumerableStringKeyOf.rs:6`
32. **warning** `non_snake_case` - 1 occurrence
   - Message: function `EnumerableStringKeyedValueOf` should have a snake case name
   - Examples:
     - `src/EnumerableStringKeyedValueOf.rs:6`
33. **warning** `non_snake_case` - 1 occurrence
   - Message: function `FilteredArray` should have a snake case name
   - Examples:
     - `src/FilteredArray.rs:6`
34. **warning** `non_snake_case` - 1 occurrence
   - Message: function `GuardType` should have a snake case name
   - Examples:
     - `src/GuardType.rs:6`
35. **warning** `non_snake_case` - 1 occurrence
   - Message: function `HasWritableKeys` should have a snake case name
   - Examples:
     - `src/HasWritableKeys.rs:6`
36. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IntRangeInclusive` should have a snake case name
   - Examples:
     - `src/IntRangeInclusive.rs:6`
37. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IsBoundedRecord` should have a snake case name
   - Examples:
     - `src/IsBoundedRecord.rs:6`
38. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IsBounded` should have a snake case name
   - Examples:
     - `src/IsBounded.rs:6`
39. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IterableContainer` should have a snake case name
   - Examples:
     - `src/IterableContainer.rs:6`
40. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyDefinition` should have a snake case name
   - Examples:
     - `src/LazyDefinition.rs:6`
41. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyEvaluator` should have a snake case name
   - Examples:
     - `src/LazyEvaluator.rs:6`
42. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyResult` should have a snake case name
   - Examples:
     - `src/LazyResult.rs:6`
43. **warning** `non_snake_case` - 1 occurrence
   - Message: function `Mapped` should have a snake case name
   - Examples:
     - `src/Mapped.rs:6`
44. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NTuple` should have a snake case name
   - Examples:
     - `src/NTuple.rs:6`
45. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NarrowedTo` should have a snake case name
   - Examples:
     - `src/NarrowedTo.rs:6`
46. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NonEmptyArray` should have a snake case name
   - Examples:
     - `src/NonEmptyArray.rs:6`
47. **warning** `non_snake_case` - 1 occurrence
   - Message: function `OptionalOptionsWithDefaults` should have a snake case name
   - Examples:
     - `src/OptionalOptionsWithDefaults.rs:6`
48. **warning** `non_snake_case` - 1 occurrence
   - Message: function `PartialArray` should have a snake case name
   - Examples:
     - `src/PartialArray.rs:6`
49. **warning** `non_snake_case` - 1 occurrence
   - Message: function `PartitionByUnion` should have a snake case name
   - Examples:
     - `src/PartitionByUnion.rs:6`
50. **warning** `non_snake_case` - 1 occurrence
   - Message: function `RemedaTypeError` should have a snake case name
   - Examples:
     - `src/RemedaTypeError.rs:6`
51. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ReorderedArray` should have a snake case name
   - Examples:
     - `src/ReorderedArray.rs:6`
52. **warning** `non_snake_case` - 1 occurrence
   - Message: function `SimplifiedWritable` should have a snake case name
   - Examples:
     - `src/SimplifiedWritable.rs:6`
53. **warning** `non_snake_case` - 1 occurrence
   - Message: function `StrictFunction` should have a snake case name
   - Examples:
     - `src/StrictFunction.rs:6`
54. **warning** `non_snake_case` - 1 occurrence
   - Message: function `StringLength` should have a snake case name
   - Examples:
     - `src/StringLength.rs:6`
55. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ToString` should have a snake case name
   - Examples:
     - `src/ToString.rs:6`
56. **warning** `non_snake_case` - 1 occurrence
   - Message: function `TupleParts` should have a snake case name
   - Examples:
     - `src/TupleParts.rs:6`
57. **warning** `non_snake_case` - 1 occurrence
   - Message: function `TupleSplits` should have a snake case name
   - Examples:
     - `src/TupleSplits.rs:6`
58. **warning** `non_snake_case` - 1 occurrence
   - Message: function `UpsertProp` should have a snake case name
   - Examples:
     - `src/UpsertProp.rs:6`
59. **warning** `non_snake_case` - 1 occurrence
   - Message: function `addProp` should have a snake case name
   - Examples:
     - `src/addProp.rs:22`
60. **warning** `non_snake_case` - 1 occurrence
   - Message: function `allPass_test` should have a snake case name
   - Examples:
     - `src/allPass_test.rs:84`
61. **warning** `non_snake_case` - 1 occurrence
   - Message: function `allPass` should have a snake case name
   - Examples:
     - `src/allPass.rs:24`
62. **warning** `non_snake_case` - 1 occurrence
   - Message: function `anyPass_test` should have a snake case name
   - Examples:
     - `src/anyPass_test.rs:84`
63. **warning** `non_snake_case` - 1 occurrence
   - Message: function `anyPass` should have a snake case name
   - Examples:
     - `src/anyPass.rs:24`
64. **warning** `non_snake_case` - 1 occurrence
   - Message: function `binarySearchCutoffIndex` should have a snake case name
   - Examples:
     - `src/binarySearchCutoffIndex.rs:36`
65. **warning** `non_snake_case` - 1 occurrence
   - Message: function `countBy` should have a snake case name
   - Examples:
     - `src/countBy.rs:73`
66. **warning** `non_snake_case` - 1 occurrence
   - Message: function `defaultTo` should have a snake case name
   - Examples:
     - `src/defaultTo.rs:21`
67. **warning** `non_snake_case` - 1 occurrence
   - Message: function `differenceWith_test` should have a snake case name
   - Examples:
     - `src/differenceWith_test.rs:166`
68. **warning** `non_snake_case` - 1 occurrence
   - Message: function `differenceWith` should have a snake case name
   - Examples:
     - `src/differenceWith.rs:12`
69. **warning** `non_snake_case` - 1 occurrence
   - Message: function `doNothing` should have a snake case name
   - Examples:
     - `src/doNothing.rs:18`
70. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropFirstBy` should have a snake case name
   - Examples:
     - `src/dropFirstBy.rs:64`
71. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLastWhile` should have a snake case name
   - Examples:
     - `src/dropLastWhile.rs:61`
72. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLast` should have a snake case name
   - Examples:
     - `src/dropLast.rs:35`
73. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropWhile` should have a snake case name
   - Examples:
     - `src/dropWhile.rs:50`
74. **warning** `non_snake_case` - 1 occurrence
   - Message: function `endsWith` should have a snake case name
   - Examples:
     - `src/endsWith.rs:22`
75. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findIndex` should have a snake case name
   - Examples:
     - `src/findIndex.rs:21`
76. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findLastIndex` should have a snake case name
   - Examples:
     - `src/findLastIndex.rs:38`
77. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findLast` should have a snake case name
   - Examples:
     - `src/findLast.rs:38`
78. **warning** `non_snake_case` - 1 occurrence
   - Message: function `firstBy` should have a snake case name
   - Examples:
     - `src/firstBy.rs:56`
79. **warning** `non_snake_case` - 1 occurrence
   - Message: function `flatMap` should have a snake case name
   - Examples:
     - `src/flatMap.rs:21`
80. **warning** `non_snake_case` - 1 occurrence
   - Message: function `forEachObj` should have a snake case name
   - Examples:
     - `src/forEachObj.rs:43`
81. **warning** `non_snake_case` - 1 occurrence
   - Message: function `forEach` should have a snake case name
   - Examples:
     - `src/forEach.rs:21`
82. **warning** `non_snake_case` - 1 occurrence
   - Message: function `fromEntries` should have a snake case name
   - Examples:
     - `src/fromEntries.rs:14`
83. **warning** `non_snake_case` - 1 occurrence
   - Message: function `fromKeys` should have a snake case name
   - Examples:
     - `src/fromKeys.rs:45`
84. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupByProp_test` should have a snake case name
   - Examples:
     - `src/groupByProp_test.rs:276`
85. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupByProp` should have a snake case name
   - Examples:
     - `src/groupByProp.rs:67`
86. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupBy` should have a snake case name
   - Examples:
     - `src/groupBy.rs:70`
87. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasAtLeast` should have a snake case name
   - Examples:
     - `src/hasAtLeast.rs:21`
88. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasProp` should have a snake case name
   - Examples:
     - `src/hasProp.rs:22`
89. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasSubObject` should have a snake case name
   - Examples:
     - `src/hasSubObject.rs:58`
90. **warning** `non_snake_case` - 1 occurrence
   - Message: function `indexBy` should have a snake case name
   - Examples:
     - `src/indexBy.rs:47`
91. **warning** `non_snake_case` - 1 occurrence
   - Message: function `intersectionWith_test` should have a snake case name
   - Examples:
     - `src/intersectionWith_test.rs:151`
92. **warning** `non_snake_case` - 1 occurrence
   - Message: function `intersectionWith` should have a snake case name
   - Examples:
     - `src/intersectionWith.rs:12`
93. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isArray` should have a snake case name
   - Examples:
     - `src/isArray.rs:10`
94. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isBigInt` should have a snake case name
   - Examples:
     - `src/isBigInt.rs:10`
95. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isBoolean` should have a snake case name
   - Examples:
     - `src/isBoolean.rs:10`
96. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDate` should have a snake case name
   - Examples:
     - `src/isDate.rs:10`
97. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDeepEqual` should have a snake case name
   - Examples:
     - `src/isDeepEqual.rs:311`
98. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDefined` should have a snake case name
   - Examples:
     - `src/isDefined.rs:12`
99. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isEmpty` should have a snake case name
   - Examples:
     - `src/isEmpty.rs:28`
100. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isEmptyish` should have a snake case name
   - Examples:
     - `src/isEmptyish.rs:66`
101. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isError` should have a snake case name
   - Examples:
     - `src/isError.rs:11`
102. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isFunction` should have a snake case name
   - Examples:
     - `src/isFunction.rs:10`
103. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isIncludedIn` should have a snake case name
   - Examples:
     - `src/isIncludedIn.rs:29`
104. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNonNull` should have a snake case name
   - Examples:
     - `src/isNonNull.rs:12`
105. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNonNullish` should have a snake case name
   - Examples:
     - `src/isNonNullish.rs:15`
106. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNot` should have a snake case name
   - Examples:
     - `src/isNot.rs:14`
107. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNullish` should have a snake case name
   - Examples:
     - `src/isNullish.rs:15`
108. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNumber` should have a snake case name
   - Examples:
     - `src/isNumber.rs:13`
109. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isObjectType` should have a snake case name
   - Examples:
     - `src/isObjectType.rs:14`
110. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isPlainObject` should have a snake case name
   - Examples:
     - `src/isPlainObject.rs:29`
111. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isPromise` should have a snake case name
   - Examples:
     - `src/isPromise.rs:11`
112. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isShallowEqual` should have a snake case name
   - Examples:
     - `src/isShallowEqual.rs:201`
113. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isStrictEqual` should have a snake case name
   - Examples:
     - `src/isStrictEqual.rs:22`
114. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isString` should have a snake case name
   - Examples:
     - `src/isString.rs:10`
115. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isSymbol` should have a snake case name
   - Examples:
     - `src/isSymbol.rs:11`
116. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isTruthy` should have a snake case name
   - Examples:
     - `src/isTruthy.rs:11`
117. **warning** `non_snake_case` - 1 occurrence
   - Message: function `lazyDataLastImpl` should have a snake case name
   - Examples:
     - `src/lazyDataLastImpl.rs:22`
118. **warning** `non_snake_case` - 1 occurrence
   - Message: function `lazyInvocationCounter` should have a snake case name
   - Examples:
     - `src/lazyInvocationCounter.rs:24`
119. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapKeys` should have a snake case name
   - Examples:
     - `src/mapKeys.rs:48`
120. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapToObj` should have a snake case name
   - Examples:
     - `src/mapToObj.rs:53`
121. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapValues` should have a snake case name
   - Examples:
     - `src/mapValues.rs:48`
122. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapWithFeedback` should have a snake case name
   - Examples:
     - `src/mapWithFeedback.rs:12`
123. **warning** `non_snake_case` - 1 occurrence
   - Message: function `meanBy` should have a snake case name
   - Examples:
     - `src/meanBy.rs:58`
124. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeAll` should have a snake case name
   - Examples:
     - `src/mergeAll.rs:29`
125. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeDeep` should have a snake case name
   - Examples:
     - `src/mergeDeep.rs:72`
126. **warning** `non_snake_case` - 1 occurrence
   - Message: function `nthBy` should have a snake case name
   - Examples:
     - `src/nthBy.rs:32`
127. **warning** `non_snake_case` - 1 occurrence
   - Message: function `objOf` should have a snake case name
   - Examples:
     - `src/objOf.rs:20`
128. **warning** `non_snake_case` - 1 occurrence
   - Message: function `omitBy` should have a snake case name
   - Examples:
     - `src/omitBy.rs:53`
129. **warning** `non_snake_case` - 1 occurrence
   - Message: function `partialBind` should have a snake case name
   - Examples:
     - `src/partialBind.rs:15`
130. **warning** `non_snake_case` - 1 occurrence
   - Message: function `partialLastBind` should have a snake case name
   - Examples:
     - `src/partialLastBind.rs:15`
131. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pathOr_test` should have a snake case name
   - Examples:
     - `src/pathOr_test.rs:161`
132. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pathOr` should have a snake case name
   - Examples:
     - `src/pathOr.rs:48`
133. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pickBy` should have a snake case name
   - Examples:
     - `src/pickBy.rs:52`
134. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pullObject` should have a snake case name
   - Examples:
     - `src/pullObject.rs:51`
135. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryFromLazy` should have a snake case name
   - Examples:
     - `src/purryFromLazy.rs:51`
136. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryOn` should have a snake case name
   - Examples:
     - `src/purryOn.rs:24`
137. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryOrderRules` should have a snake case name
   - Examples:
     - `src/purryOrderRules.rs:205`
138. **warning** `non_snake_case` - 1 occurrence
   - Message: function `quickSelect` should have a snake case name
   - Examples:
     - `src/quickSelect.rs:100`
139. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomBigInt_test` should have a snake case name
   - Examples:
     - `src/randomBigInt_test.rs:563`
140. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomBigInt` should have a snake case name
   - Examples:
     - `src/randomBigInt.rs:114`
141. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomInteger_test` should have a snake case name
   - Examples:
     - `src/randomInteger_test.rs:237`
142. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomInteger` should have a snake case name
   - Examples:
     - `src/randomInteger.rs:45`
143. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomString` should have a snake case name
   - Examples:
     - `src/randomString.rs:45`
144. **warning** `non_snake_case` - 1 occurrence
   - Message: function `rankBy` should have a snake case name
   - Examples:
     - `src/rankBy.rs:45`
145. **warning** `non_snake_case` - 1 occurrence
   - Message: function `setPath_test` should have a snake case name
   - Examples:
     - `src/setPath_test.rs:375`
146. **warning** `non_snake_case` - 1 occurrence
   - Message: function `setPath` should have a snake case name
   - Examples:
     - `src/setPath.rs:62`
147. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sliceString_test` should have a snake case name
   - Examples:
     - `src/sliceString_test.rs:503`
148. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sliceString` should have a snake case name
   - Examples:
     - `src/sliceString.rs:26`
149. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortBy_test` should have a snake case name
   - Examples:
     - `src/sortBy_test.rs:287`
150. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortBy` should have a snake case name
   - Examples:
     - `src/sortBy.rs:20`
151. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndexBy` should have a snake case name
   - Examples:
     - `src/sortedIndexBy.rs:23`
152. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndexWith` should have a snake case name
   - Examples:
     - `src/sortedIndexWith.rs:15`
153. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndex` should have a snake case name
   - Examples:
     - `src/sortedIndex.rs:24`
154. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedLastIndexBy` should have a snake case name
   - Examples:
     - `src/sortedLastIndexBy.rs:23`
155. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedLastIndex` should have a snake case name
   - Examples:
     - `src/sortedLastIndex.rs:24`
156. **warning** `non_snake_case` - 1 occurrence
   - Message: function `splitAt` should have a snake case name
   - Examples:
     - `src/splitAt.rs:45`
157. **warning** `non_snake_case` - 1 occurrence
   - Message: function `splitWhen` should have a snake case name
   - Examples:
     - `src/splitWhen.rs:41`
158. **warning** `non_snake_case` - 1 occurrence
   - Message: function `startsWith` should have a snake case name
   - Examples:
     - `src/startsWith.rs:22`
159. **warning** `non_snake_case` - 1 occurrence
   - Message: function `stringToPath` should have a snake case name
   - Examples:
     - `src/stringToPath.rs:83`
160. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sumBy` should have a snake case name
   - Examples:
     - `src/sumBy.rs:80`
161. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapInPlace` should have a snake case name
   - Examples:
     - `src/swapInPlace.rs:11`
162. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapIndices` should have a snake case name
   - Examples:
     - `src/swapIndices.rs:101`
163. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapProps` should have a snake case name
   - Examples:
     - `src/swapProps.rs:30`
164. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeFirstBy` should have a snake case name
   - Examples:
     - `src/takeFirstBy.rs:55`
165. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLastWhile` should have a snake case name
   - Examples:
     - `src/takeLastWhile.rs:59`
166. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLast` should have a snake case name
   - Examples:
     - `src/takeLast.rs:37`
167. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeWhile` should have a snake case name
   - Examples:
     - `src/takeWhile.rs:53`
168. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toCamelCase` should have a snake case name
   - Examples:
     - `src/toCamelCase.rs:51`
169. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toKebabCase` should have a snake case name
   - Examples:
     - `src/toKebabCase.rs:26`
170. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toLowerCase` should have a snake case name
   - Examples:
     - `src/toLowerCase.rs:20`
171. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toSingle` should have a snake case name
   - Examples:
     - `src/toSingle.rs:11`
172. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toSnakeCase` should have a snake case name
   - Examples:
     - `src/toSnakeCase.rs:26`
173. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toTitleCase` should have a snake case name
   - Examples:
     - `src/toTitleCase.rs:44`
174. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toUpperCase` should have a snake case name
   - Examples:
     - `src/toUpperCase.rs:20`
175. **warning** `non_snake_case` - 1 occurrence
   - Message: function `typesDataProvider` should have a snake case name
   - Examples:
     - `src/typesDataProvider.rs:6`
176. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueBy_test` should have a snake case name
   - Examples:
     - `src/uniqueBy_test.rs:203`
177. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueBy` should have a snake case name
   - Examples:
     - `src/uniqueBy.rs:40`
178. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueWith_test` should have a snake case name
   - Examples:
     - `src/uniqueWith_test.rs:220`
179. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueWith` should have a snake case name
   - Examples:
     - `src/uniqueWith.rs:12`
180. **warning** `non_snake_case` - 1 occurrence
   - Message: function `utilityEvaluators` should have a snake case name
   - Examples:
     - `src/utilityEvaluators.rs:16`
181. **warning** `non_snake_case` - 1 occurrence
   - Message: function `withPrecision` should have a snake case name
   - Examples:
     - `src/withPrecision.rs:106`
182. **warning** `non_snake_case` - 1 occurrence
   - Message: function `zipWith` should have a snake case name
   - Examples:
     - `src/zipWith.rs:56`
183. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ArrayAt` should have a snake case name
   - Examples:
     - `src/main.rs:4`
184. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ArrayRequiredPrefix` should have a snake case name
   - Examples:
     - `src/main.rs:6`
185. **warning** `non_snake_case` - 1 occurrence
   - Message: module `BoundedPartial` should have a snake case name
   - Examples:
     - `src/main.rs:8`
186. **warning** `non_snake_case` - 1 occurrence
   - Message: module `BrandedReturn` should have a snake case name
   - Examples:
     - `src/main.rs:10`
187. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ClampedIntegerSubtract` should have a snake case name
   - Examples:
     - `src/main.rs:12`
188. **warning** `non_snake_case` - 1 occurrence
   - Message: module `CoercedArray` should have a snake case name
   - Examples:
     - `src/main.rs:14`
189. **warning** `non_snake_case` - 1 occurrence
   - Message: module `CompareFunction` should have a snake case name
   - Examples:
     - `src/main.rs:16`
190. **warning** `non_snake_case` - 1 occurrence
   - Message: module `Deduped` should have a snake case name
   - Examples:
     - `src/main.rs:18`
191. **warning** `non_snake_case` - 1 occurrence
   - Message: module `DisjointUnionFields` should have a snake case name
   - Examples:
     - `src/main.rs:20`
192. **warning** `non_snake_case` - 1 occurrence
   - Message: module `EnumerableStringKeyOf` should have a snake case name
   - Examples:
     - `src/main.rs:22`
193. **warning** `non_snake_case` - 1 occurrence
   - Message: module `EnumerableStringKeyedValueOf` should have a snake case name
   - Examples:
     - `src/main.rs:24`
194. **warning** `non_snake_case` - 1 occurrence
   - Message: module `FilteredArray` should have a snake case name
   - Examples:
     - `src/main.rs:26`
195. **warning** `non_snake_case` - 1 occurrence
   - Message: module `GuardType` should have a snake case name
   - Examples:
     - `src/main.rs:28`
196. **warning** `non_snake_case` - 1 occurrence
   - Message: module `HasWritableKeys` should have a snake case name
   - Examples:
     - `src/main.rs:30`
197. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IntRangeInclusive` should have a snake case name
   - Examples:
     - `src/main.rs:32`
198. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IsBoundedRecord` should have a snake case name
   - Examples:
     - `src/main.rs:36`
199. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IsBounded` should have a snake case name
   - Examples:
     - `src/main.rs:34`
200. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IterableContainer` should have a snake case name
   - Examples:
     - `src/main.rs:38`
201. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyDefinition` should have a snake case name
   - Examples:
     - `src/main.rs:40`
202. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyEvaluator` should have a snake case name
   - Examples:
     - `src/main.rs:42`
203. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyResult` should have a snake case name
   - Examples:
     - `src/main.rs:44`
204. **warning** `non_snake_case` - 1 occurrence
   - Message: module `Mapped` should have a snake case name
   - Examples:
     - `src/main.rs:46`
205. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NTuple` should have a snake case name
   - Examples:
     - `src/main.rs:48`
206. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NarrowedTo` should have a snake case name
   - Examples:
     - `src/main.rs:50`
207. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NonEmptyArray` should have a snake case name
   - Examples:
     - `src/main.rs:52`
208. **warning** `non_snake_case` - 1 occurrence
   - Message: module `OptionalOptionsWithDefaults` should have a snake case name
   - Examples:
     - `src/main.rs:54`
209. **warning** `non_snake_case` - 1 occurrence
   - Message: module `PartialArray` should have a snake case name
   - Examples:
     - `src/main.rs:56`
210. **warning** `non_snake_case` - 1 occurrence
   - Message: module `PartitionByUnion` should have a snake case name
   - Examples:
     - `src/main.rs:58`
211. **warning** `non_snake_case` - 1 occurrence
   - Message: module `RemedaTypeError` should have a snake case name
   - Examples:
     - `src/main.rs:60`
212. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ReorderedArray` should have a snake case name
   - Examples:
     - `src/main.rs:62`
213. **warning** `non_snake_case` - 1 occurrence
   - Message: module `SimplifiedWritable` should have a snake case name
   - Examples:
     - `src/main.rs:64`
214. **warning** `non_snake_case` - 1 occurrence
   - Message: module `StrictFunction` should have a snake case name
   - Examples:
     - `src/main.rs:66`
215. **warning** `non_snake_case` - 1 occurrence
   - Message: module `StringLength` should have a snake case name
   - Examples:
     - `src/main.rs:68`
216. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ToString` should have a snake case name
   - Examples:
     - `src/main.rs:70`
217. **warning** `non_snake_case` - 1 occurrence
   - Message: module `TupleParts` should have a snake case name
   - Examples:
     - `src/main.rs:72`
218. **warning** `non_snake_case` - 1 occurrence
   - Message: module `TupleSplits` should have a snake case name
   - Examples:
     - `src/main.rs:74`
219. **warning** `non_snake_case` - 1 occurrence
   - Message: module `UpsertProp` should have a snake case name
   - Examples:
     - `src/main.rs:76`
220. **warning** `non_snake_case` - 1 occurrence
   - Message: module `addProp_test` should have a snake case name
   - Examples:
     - `src/main.rs:82`
221. **warning** `non_snake_case` - 1 occurrence
   - Message: module `addProp` should have a snake case name
   - Examples:
     - `src/main.rs:80`
222. **warning** `non_snake_case` - 1 occurrence
   - Message: module `allPass_test` should have a snake case name
   - Examples:
     - `src/main.rs:88`
223. **warning** `non_snake_case` - 1 occurrence
   - Message: module `allPass` should have a snake case name
   - Examples:
     - `src/main.rs:86`
224. **warning** `non_snake_case` - 1 occurrence
   - Message: module `anyPass_test` should have a snake case name
   - Examples:
     - `src/main.rs:92`
225. **warning** `non_snake_case` - 1 occurrence
   - Message: module `anyPass` should have a snake case name
   - Examples:
     - `src/main.rs:90`
226. **warning** `non_snake_case` - 1 occurrence
   - Message: module `binarySearchCutoffIndex_test` should have a snake case name
   - Examples:
     - `src/main.rs:96`
227. **warning** `non_snake_case` - 1 occurrence
   - Message: module `binarySearchCutoffIndex` should have a snake case name
   - Examples:
     - `src/main.rs:94`
228. **warning** `non_snake_case` - 1 occurrence
   - Message: module `countBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:132`
229. **warning** `non_snake_case` - 1 occurrence
   - Message: module `countBy` should have a snake case name
   - Examples:
     - `src/main.rs:130`
230. **warning** `non_snake_case` - 1 occurrence
   - Message: module `defaultTo_test` should have a snake case name
   - Examples:
     - `src/main.rs:140`
231. **warning** `non_snake_case` - 1 occurrence
   - Message: module `defaultTo` should have a snake case name
   - Examples:
     - `src/main.rs:138`
232. **warning** `non_snake_case` - 1 occurrence
   - Message: module `differenceWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:146`
233. **warning** `non_snake_case` - 1 occurrence
   - Message: module `differenceWith` should have a snake case name
   - Examples:
     - `src/main.rs:144`
234. **warning** `non_snake_case` - 1 occurrence
   - Message: module `doNothing_test` should have a snake case name
   - Examples:
     - `src/main.rs:156`
235. **warning** `non_snake_case` - 1 occurrence
   - Message: module `doNothing` should have a snake case name
   - Examples:
     - `src/main.rs:154`
236. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropFirstBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:162`
237. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropFirstBy` should have a snake case name
   - Examples:
     - `src/main.rs:160`
238. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLastWhile_test` should have a snake case name
   - Examples:
     - `src/main.rs:168`
239. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLastWhile` should have a snake case name
   - Examples:
     - `src/main.rs:166`
240. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLast_test` should have a snake case name
   - Examples:
     - `src/main.rs:170`
241. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLast` should have a snake case name
   - Examples:
     - `src/main.rs:164`
242. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropWhile_test` should have a snake case name
   - Examples:
     - `src/main.rs:174`
243. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropWhile` should have a snake case name
   - Examples:
     - `src/main.rs:172`
244. **warning** `non_snake_case` - 1 occurrence
   - Message: module `endsWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:180`
245. **warning** `non_snake_case` - 1 occurrence
   - Message: module `endsWith` should have a snake case name
   - Examples:
     - `src/main.rs:178`
246. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findIndex_test` should have a snake case name
   - Examples:
     - `src/main.rs:198`
247. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findIndex` should have a snake case name
   - Examples:
     - `src/main.rs:196`
248. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLastIndex_test` should have a snake case name
   - Examples:
     - `src/main.rs:204`
249. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLastIndex` should have a snake case name
   - Examples:
     - `src/main.rs:202`
250. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLast_test` should have a snake case name
   - Examples:
     - `src/main.rs:206`
251. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLast` should have a snake case name
   - Examples:
     - `src/main.rs:200`
252. **warning** `non_snake_case` - 1 occurrence
   - Message: module `firstBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:214`
253. **warning** `non_snake_case` - 1 occurrence
   - Message: module `firstBy` should have a snake case name
   - Examples:
     - `src/main.rs:212`
254. **warning** `non_snake_case` - 1 occurrence
   - Message: module `flatMap_test` should have a snake case name
   - Examples:
     - `src/main.rs:222`
255. **warning** `non_snake_case` - 1 occurrence
   - Message: module `flatMap` should have a snake case name
   - Examples:
     - `src/main.rs:220`
256. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEachObj_test` should have a snake case name
   - Examples:
     - `src/main.rs:234`
257. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEachObj` should have a snake case name
   - Examples:
     - `src/main.rs:232`
258. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEach_test` should have a snake case name
   - Examples:
     - `src/main.rs:236`
259. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEach` should have a snake case name
   - Examples:
     - `src/main.rs:230`
260. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromEntries_test` should have a snake case name
   - Examples:
     - `src/main.rs:240`
261. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromEntries` should have a snake case name
   - Examples:
     - `src/main.rs:238`
262. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromKeys_test` should have a snake case name
   - Examples:
     - `src/main.rs:244`
263. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromKeys` should have a snake case name
   - Examples:
     - `src/main.rs:242`
264. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupByProp_test` should have a snake case name
   - Examples:
     - `src/main.rs:266`
265. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupByProp` should have a snake case name
   - Examples:
     - `src/main.rs:264`
266. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:268`
267. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupBy` should have a snake case name
   - Examples:
     - `src/main.rs:262`
268. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasAtLeast_test` should have a snake case name
   - Examples:
     - `src/main.rs:272`
269. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasAtLeast` should have a snake case name
   - Examples:
     - `src/main.rs:270`
270. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasProp_test` should have a snake case name
   - Examples:
     - `src/main.rs:276`
271. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasProp` should have a snake case name
   - Examples:
     - `src/main.rs:274`
272. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasSubObject_test` should have a snake case name
   - Examples:
     - `src/main.rs:280`
273. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasSubObject` should have a snake case name
   - Examples:
     - `src/main.rs:278`
274. **warning** `non_snake_case` - 1 occurrence
   - Message: module `indexBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:292`
275. **warning** `non_snake_case` - 1 occurrence
   - Message: module `indexBy` should have a snake case name
   - Examples:
     - `src/main.rs:290`
276. **warning** `non_snake_case` - 1 occurrence
   - Message: module `intersectionWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:298`
277. **warning** `non_snake_case` - 1 occurrence
   - Message: module `intersectionWith` should have a snake case name
   - Examples:
     - `src/main.rs:296`
278. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isArray_test` should have a snake case name
   - Examples:
     - `src/main.rs:308`
279. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isArray` should have a snake case name
   - Examples:
     - `src/main.rs:306`
280. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBigInt_test` should have a snake case name
   - Examples:
     - `src/main.rs:312`
281. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBigInt` should have a snake case name
   - Examples:
     - `src/main.rs:310`
282. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBoolean_test` should have a snake case name
   - Examples:
     - `src/main.rs:316`
283. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBoolean` should have a snake case name
   - Examples:
     - `src/main.rs:314`
284. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDate_test` should have a snake case name
   - Examples:
     - `src/main.rs:320`
285. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDate` should have a snake case name
   - Examples:
     - `src/main.rs:318`
286. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDeepEqual_test` should have a snake case name
   - Examples:
     - `src/main.rs:324`
287. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDeepEqual` should have a snake case name
   - Examples:
     - `src/main.rs:322`
288. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDefined_test` should have a snake case name
   - Examples:
     - `src/main.rs:328`
289. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDefined` should have a snake case name
   - Examples:
     - `src/main.rs:326`
290. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmpty_test` should have a snake case name
   - Examples:
     - `src/main.rs:332`
291. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmpty` should have a snake case name
   - Examples:
     - `src/main.rs:330`
292. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmptyish_test` should have a snake case name
   - Examples:
     - `src/main.rs:336`
293. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmptyish` should have a snake case name
   - Examples:
     - `src/main.rs:334`
294. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isError_test` should have a snake case name
   - Examples:
     - `src/main.rs:340`
295. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isError` should have a snake case name
   - Examples:
     - `src/main.rs:338`
296. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isFunction_test` should have a snake case name
   - Examples:
     - `src/main.rs:344`
297. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isFunction` should have a snake case name
   - Examples:
     - `src/main.rs:342`
298. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isIncludedIn_test` should have a snake case name
   - Examples:
     - `src/main.rs:348`
299. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isIncludedIn` should have a snake case name
   - Examples:
     - `src/main.rs:346`
300. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNull_test` should have a snake case name
   - Examples:
     - `src/main.rs:352`
301. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNull` should have a snake case name
   - Examples:
     - `src/main.rs:350`
302. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNullish_test` should have a snake case name
   - Examples:
     - `src/main.rs:356`
303. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNullish` should have a snake case name
   - Examples:
     - `src/main.rs:354`
304. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNot_test` should have a snake case name
   - Examples:
     - `src/main.rs:360`
305. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNot` should have a snake case name
   - Examples:
     - `src/main.rs:358`
306. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNullish_test` should have a snake case name
   - Examples:
     - `src/main.rs:364`
307. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNullish` should have a snake case name
   - Examples:
     - `src/main.rs:362`
308. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNumber_test` should have a snake case name
   - Examples:
     - `src/main.rs:368`
309. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNumber` should have a snake case name
   - Examples:
     - `src/main.rs:366`
310. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isObjectType_test` should have a snake case name
   - Examples:
     - `src/main.rs:372`
311. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isObjectType` should have a snake case name
   - Examples:
     - `src/main.rs:370`
312. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPlainObject_test` should have a snake case name
   - Examples:
     - `src/main.rs:376`
313. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPlainObject` should have a snake case name
   - Examples:
     - `src/main.rs:374`
314. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPromise_test` should have a snake case name
   - Examples:
     - `src/main.rs:380`
315. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPromise` should have a snake case name
   - Examples:
     - `src/main.rs:378`
316. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isShallowEqual_test` should have a snake case name
   - Examples:
     - `src/main.rs:384`
317. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isShallowEqual` should have a snake case name
   - Examples:
     - `src/main.rs:382`
318. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isStrictEqual_test` should have a snake case name
   - Examples:
     - `src/main.rs:388`
319. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isStrictEqual` should have a snake case name
   - Examples:
     - `src/main.rs:386`
320. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isString_test` should have a snake case name
   - Examples:
     - `src/main.rs:392`
321. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isString` should have a snake case name
   - Examples:
     - `src/main.rs:390`
322. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isSymbol_test` should have a snake case name
   - Examples:
     - `src/main.rs:396`
323. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isSymbol` should have a snake case name
   - Examples:
     - `src/main.rs:394`
324. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isTruthy_test` should have a snake case name
   - Examples:
     - `src/main.rs:400`
325. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isTruthy` should have a snake case name
   - Examples:
     - `src/main.rs:398`
326. **warning** `non_snake_case` - 1 occurrence
   - Message: module `lazyDataLastImpl` should have a snake case name
   - Examples:
     - `src/main.rs:414`
327. **warning** `non_snake_case` - 1 occurrence
   - Message: module `lazyInvocationCounter` should have a snake case name
   - Examples:
     - `src/main.rs:416`
328. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapKeys_test` should have a snake case name
   - Examples:
     - `src/main.rs:426`
329. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapKeys` should have a snake case name
   - Examples:
     - `src/main.rs:424`
330. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapToObj_test` should have a snake case name
   - Examples:
     - `src/main.rs:430`
331. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapToObj` should have a snake case name
   - Examples:
     - `src/main.rs:428`
332. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapValues_test` should have a snake case name
   - Examples:
     - `src/main.rs:434`
333. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapValues` should have a snake case name
   - Examples:
     - `src/main.rs:432`
334. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapWithFeedback_test` should have a snake case name
   - Examples:
     - `src/main.rs:438`
335. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapWithFeedback` should have a snake case name
   - Examples:
     - `src/main.rs:436`
336. **warning** `non_snake_case` - 1 occurrence
   - Message: module `meanBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:446`
337. **warning** `non_snake_case` - 1 occurrence
   - Message: module `meanBy` should have a snake case name
   - Examples:
     - `src/main.rs:444`
338. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeAll_test` should have a snake case name
   - Examples:
     - `src/main.rs:458`
339. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeAll` should have a snake case name
   - Examples:
     - `src/main.rs:456`
340. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeDeep_test` should have a snake case name
   - Examples:
     - `src/main.rs:462`
341. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeDeep` should have a snake case name
   - Examples:
     - `src/main.rs:460`
342. **warning** `non_snake_case` - 1 occurrence
   - Message: module `nthBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:472`
343. **warning** `non_snake_case` - 1 occurrence
   - Message: module `nthBy` should have a snake case name
   - Examples:
     - `src/main.rs:470`
344. **warning** `non_snake_case` - 1 occurrence
   - Message: module `objOf_test` should have a snake case name
   - Examples:
     - `src/main.rs:476`
345. **warning** `non_snake_case` - 1 occurrence
   - Message: module `objOf` should have a snake case name
   - Examples:
     - `src/main.rs:474`
346. **warning** `non_snake_case` - 1 occurrence
   - Message: module `omitBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:482`
347. **warning** `non_snake_case` - 1 occurrence
   - Message: module `omitBy` should have a snake case name
   - Examples:
     - `src/main.rs:480`
348. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialBind_test` should have a snake case name
   - Examples:
     - `src/main.rs:496`
349. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialBind` should have a snake case name
   - Examples:
     - `src/main.rs:494`
350. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialLastBind_test` should have a snake case name
   - Examples:
     - `src/main.rs:500`
351. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialLastBind` should have a snake case name
   - Examples:
     - `src/main.rs:498`
352. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pathOr_test` should have a snake case name
   - Examples:
     - `src/main.rs:508`
353. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pathOr` should have a snake case name
   - Examples:
     - `src/main.rs:506`
354. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pickBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:514`
355. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pickBy` should have a snake case name
   - Examples:
     - `src/main.rs:512`
356. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pullObject_test` should have a snake case name
   - Examples:
     - `src/main.rs:536`
357. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pullObject` should have a snake case name
   - Examples:
     - `src/main.rs:534`
358. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryFromLazy_test` should have a snake case name
   - Examples:
     - `src/main.rs:542`
359. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryFromLazy` should have a snake case name
   - Examples:
     - `src/main.rs:540`
360. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryOn` should have a snake case name
   - Examples:
     - `src/main.rs:544`
361. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryOrderRules` should have a snake case name
   - Examples:
     - `src/main.rs:546`
362. **warning** `non_snake_case` - 1 occurrence
   - Message: module `quickSelect` should have a snake case name
   - Examples:
     - `src/main.rs:550`
363. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomBigInt_test` should have a snake case name
   - Examples:
     - `src/main.rs:554`
364. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomBigInt` should have a snake case name
   - Examples:
     - `src/main.rs:552`
365. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomInteger_test` should have a snake case name
   - Examples:
     - `src/main.rs:558`
366. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomInteger` should have a snake case name
   - Examples:
     - `src/main.rs:556`
367. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomString_test` should have a snake case name
   - Examples:
     - `src/main.rs:562`
368. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomString` should have a snake case name
   - Examples:
     - `src/main.rs:560`
369. **warning** `non_snake_case` - 1 occurrence
   - Message: module `rankBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:570`
370. **warning** `non_snake_case` - 1 occurrence
   - Message: module `rankBy` should have a snake case name
   - Examples:
     - `src/main.rs:568`
371. **warning** `non_snake_case` - 1 occurrence
   - Message: module `setPath_test` should have a snake case name
   - Examples:
     - `src/main.rs:592`
372. **warning** `non_snake_case` - 1 occurrence
   - Message: module `setPath` should have a snake case name
   - Examples:
     - `src/main.rs:590`
373. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sliceString_test` should have a snake case name
   - Examples:
     - `src/main.rs:604`
374. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sliceString` should have a snake case name
   - Examples:
     - `src/main.rs:602`
375. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:610`
376. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortBy` should have a snake case name
   - Examples:
     - `src/main.rs:608`
377. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:618`
378. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexBy` should have a snake case name
   - Examples:
     - `src/main.rs:616`
379. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:622`
380. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexWith` should have a snake case name
   - Examples:
     - `src/main.rs:620`
381. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndex_test` should have a snake case name
   - Examples:
     - `src/main.rs:624`
382. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndex` should have a snake case name
   - Examples:
     - `src/main.rs:614`
383. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndexBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:630`
384. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndexBy` should have a snake case name
   - Examples:
     - `src/main.rs:628`
385. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndex_test` should have a snake case name
   - Examples:
     - `src/main.rs:632`
386. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndex` should have a snake case name
   - Examples:
     - `src/main.rs:626`
387. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitAt_test` should have a snake case name
   - Examples:
     - `src/main.rs:642`
388. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitAt` should have a snake case name
   - Examples:
     - `src/main.rs:640`
389. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitWhen_test` should have a snake case name
   - Examples:
     - `src/main.rs:646`
390. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitWhen` should have a snake case name
   - Examples:
     - `src/main.rs:644`
391. **warning** `non_snake_case` - 1 occurrence
   - Message: module `startsWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:654`
392. **warning** `non_snake_case` - 1 occurrence
   - Message: module `startsWith` should have a snake case name
   - Examples:
     - `src/main.rs:652`
393. **warning** `non_snake_case` - 1 occurrence
   - Message: module `stringToPath_test` should have a snake case name
   - Examples:
     - `src/main.rs:658`
394. **warning** `non_snake_case` - 1 occurrence
   - Message: module `stringToPath` should have a snake case name
   - Examples:
     - `src/main.rs:656`
395. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sumBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:668`
396. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sumBy` should have a snake case name
   - Examples:
     - `src/main.rs:666`
397. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapInPlace` should have a snake case name
   - Examples:
     - `src/main.rs:672`
398. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapIndices_test` should have a snake case name
   - Examples:
     - `src/main.rs:676`
399. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapIndices` should have a snake case name
   - Examples:
     - `src/main.rs:674`
400. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapProps_test` should have a snake case name
   - Examples:
     - `src/main.rs:680`
401. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapProps` should have a snake case name
   - Examples:
     - `src/main.rs:678`
402. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeFirstBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:686`
403. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeFirstBy` should have a snake case name
   - Examples:
     - `src/main.rs:684`
404. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLastWhile_test` should have a snake case name
   - Examples:
     - `src/main.rs:692`
405. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLastWhile` should have a snake case name
   - Examples:
     - `src/main.rs:690`
406. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLast_test` should have a snake case name
   - Examples:
     - `src/main.rs:694`
407. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLast` should have a snake case name
   - Examples:
     - `src/main.rs:688`
408. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeWhile_test` should have a snake case name
   - Examples:
     - `src/main.rs:698`
409. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeWhile` should have a snake case name
   - Examples:
     - `src/main.rs:696`
410. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toCamelCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:712`
411. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toCamelCase` should have a snake case name
   - Examples:
     - `src/main.rs:710`
412. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toKebabCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:716`
413. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toKebabCase` should have a snake case name
   - Examples:
     - `src/main.rs:714`
414. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toLowerCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:720`
415. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toLowerCase` should have a snake case name
   - Examples:
     - `src/main.rs:718`
416. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toSingle` should have a snake case name
   - Examples:
     - `src/main.rs:722`
417. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toSnakeCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:726`
418. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toSnakeCase` should have a snake case name
   - Examples:
     - `src/main.rs:724`
419. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toTitleCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:730`
420. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toTitleCase` should have a snake case name
   - Examples:
     - `src/main.rs:728`
421. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toUpperCase_test` should have a snake case name
   - Examples:
     - `src/main.rs:734`
422. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toUpperCase` should have a snake case name
   - Examples:
     - `src/main.rs:732`
423. **warning** `non_snake_case` - 1 occurrence
   - Message: module `typesDataProvider` should have a snake case name
   - Examples:
     - `src/main.rs:740`
424. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueBy_test` should have a snake case name
   - Examples:
     - `src/main.rs:750`
425. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueBy` should have a snake case name
   - Examples:
     - `src/main.rs:748`
426. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:754`
427. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueWith` should have a snake case name
   - Examples:
     - `src/main.rs:752`
428. **warning** `non_snake_case` - 1 occurrence
   - Message: module `utilityEvaluators` should have a snake case name
   - Examples:
     - `src/main.rs:758`
429. **warning** `non_snake_case` - 1 occurrence
   - Message: module `withPrecision_test` should have a snake case name
   - Examples:
     - `src/main.rs:770`
430. **warning** `non_snake_case` - 1 occurrence
   - Message: module `withPrecision` should have a snake case name
   - Examples:
     - `src/main.rs:768`
431. **warning** `non_snake_case` - 1 occurrence
   - Message: module `zipWith_test` should have a snake case name
   - Examples:
     - `src/main.rs:780`
432. **warning** `non_snake_case` - 1 occurrence
   - Message: module `zipWith` should have a snake case name
   - Examples:
     - `src/main.rs:778`
433. **warning** `path_statements` - 1 occurrence
   - Message: path statement drops value
   - Examples:
     - `src/funnel_remeda_debounce_test.rs:96`
434. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
435. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
436. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
437. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
438. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
439. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
440. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
441. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
442. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/pipe.rs:8`
443. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:303`
444. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_cool_down_end` is never read
   - Examples:
     - `src/debounce.rs:8`
445. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_debounced_call` is never read
   - Examples:
     - `src/debounce.rs:9`
446. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_invoke` is never read
   - Examples:
     - `src/debounce.rs:7`
447. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
448. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
449. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_fn` is never read
   - Examples:
     - `src/pipe.rs:254`
450. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_sequence` is never read
   - Examples:
     - `src/pipe.rs:9`
451. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
452. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:47`
453. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
454. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
455. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
456. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
457. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
458. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
459. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
460. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
461. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
462. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
463. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
464. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
465. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
466. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
467. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
468. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
469. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
470. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
471. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
472. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
473. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
474. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
475. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
476. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
477. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
478. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
479. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
480. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
481. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
482. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
483. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
484. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
485. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
486. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
487. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
488. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
489. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
490. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
491. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
492. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
493. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
494. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
495. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
496. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp_test::*`
   - Examples:
     - `src/main.rs:83`
497. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add_test::*`
   - Examples:
     - `src/main.rs:85`
498. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:87`
499. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass_test::*`
   - Examples:
     - `src/main.rs:89`
500. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:91`
501. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass_test::*`
   - Examples:
     - `src/main.rs:93`
502. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `binarySearchCutoffIndex_test::*`
   - Examples:
     - `src/main.rs:97`
503. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:99`
504. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize_test::*`
   - Examples:
     - `src/main.rs:101`
505. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:103`
506. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil_test::*`
   - Examples:
     - `src/main.rs:105`
507. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:107`
508. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk_test::*`
   - Examples:
     - `src/main.rs:109`
509. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:111`
510. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp_test::*`
   - Examples:
     - `src/main.rs:113`
511. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:115`
512. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone_test::*`
   - Examples:
     - `src/main.rs:117`
513. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:119`
514. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat_test::*`
   - Examples:
     - `src/main.rs:121`
515. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:123`
516. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional_test::*`
   - Examples:
     - `src/main.rs:125`
517. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:127`
518. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant_test::*`
   - Examples:
     - `src/main.rs:129`
519. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:131`
520. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy_test::*`
   - Examples:
     - `src/main.rs:133`
521. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:135`
522. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce_test::*`
   - Examples:
     - `src/main.rs:137`
523. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:139`
524. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo_test::*`
   - Examples:
     - `src/main.rs:141`
525. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:143`
526. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:145`
527. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith_test::*`
   - Examples:
     - `src/main.rs:147`
528. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference_test::*`
   - Examples:
     - `src/main.rs:149`
529. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:151`
530. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide_test::*`
   - Examples:
     - `src/main.rs:153`
531. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:155`
532. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing_test::*`
   - Examples:
     - `src/main.rs:157`
533. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:159`
534. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:161`
535. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy_test::*`
   - Examples:
     - `src/main.rs:163`
536. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:165`
537. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:167`
538. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile_test::*`
   - Examples:
     - `src/main.rs:169`
539. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast_test::*`
   - Examples:
     - `src/main.rs:171`
540. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:173`
541. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile_test::*`
   - Examples:
     - `src/main.rs:175`
542. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop_test::*`
   - Examples:
     - `src/main.rs:177`
543. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:179`
544. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith_test::*`
   - Examples:
     - `src/main.rs:181`
545. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:183`
546. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries_test::*`
   - Examples:
     - `src/main.rs:185`
547. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:187`
548. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve_test::*`
   - Examples:
     - `src/main.rs:189`
549. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:191`
550. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter_test::*`
   - Examples:
     - `src/main.rs:193`
551. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:195`
552. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:197`
553. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex_test::*`
   - Examples:
     - `src/main.rs:199`
554. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:201`
555. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:203`
556. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex_test::*`
   - Examples:
     - `src/main.rs:205`
557. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast_test::*`
   - Examples:
     - `src/main.rs:207`
558. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find_test::*`
   - Examples:
     - `src/main.rs:209`
559. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:211`
560. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:213`
561. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy_test::*`
   - Examples:
     - `src/main.rs:215`
562. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first_test::*`
   - Examples:
     - `src/main.rs:217`
563. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:219`
564. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:221`
565. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap_test::*`
   - Examples:
     - `src/main.rs:223`
566. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat_test::*`
   - Examples:
     - `src/main.rs:225`
567. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:227`
568. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor_test::*`
   - Examples:
     - `src/main.rs:229`
569. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:231`
570. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:233`
571. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj_test::*`
   - Examples:
     - `src/main.rs:235`
572. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach_test::*`
   - Examples:
     - `src/main.rs:237`
573. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:239`
574. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries_test::*`
   - Examples:
     - `src/main.rs:241`
575. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:243`
576. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys_test::*`
   - Examples:
     - `src/main.rs:245`
577. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_test::*`
   - Examples:
     - `src/main.rs:249`
578. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:251`
579. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_test::*`
   - Examples:
     - `src/main.rs:253`
580. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:255`
581. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_reference_batch_test::*`
   - Examples:
     - `src/main.rs:257`
582. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_remeda_debounce_test::*`
   - Examples:
     - `src/main.rs:259`
583. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_test::*`
   - Examples:
     - `src/main.rs:261`
584. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:263`
585. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:265`
586. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp_test::*`
   - Examples:
     - `src/main.rs:267`
587. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy_test::*`
   - Examples:
     - `src/main.rs:269`
588. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasAtLeast_test::*`
   - Examples:
     - `src/main.rs:273`
589. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:275`
590. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp_test::*`
   - Examples:
     - `src/main.rs:277`
591. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:279`
592. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject_test::*`
   - Examples:
     - `src/main.rs:281`
593. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `heap_test::*`
   - Examples:
     - `src/main.rs:285`
594. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:287`
595. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity_test::*`
   - Examples:
     - `src/main.rs:289`
596. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:291`
597. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy_test::*`
   - Examples:
     - `src/main.rs:293`
598. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:295`
599. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:297`
600. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith_test::*`
   - Examples:
     - `src/main.rs:299`
601. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection_test::*`
   - Examples:
     - `src/main.rs:301`
602. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:303`
603. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert_test::*`
   - Examples:
     - `src/main.rs:305`
604. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:307`
605. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray_test::*`
   - Examples:
     - `src/main.rs:309`
606. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:311`
607. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt_test::*`
   - Examples:
     - `src/main.rs:313`
608. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:315`
609. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean_test::*`
   - Examples:
     - `src/main.rs:317`
610. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:319`
611. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate_test::*`
   - Examples:
     - `src/main.rs:321`
612. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDeepEqual_test::*`
   - Examples:
     - `src/main.rs:325`
613. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:327`
614. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined_test::*`
   - Examples:
     - `src/main.rs:329`
615. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:331`
616. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty_test::*`
   - Examples:
     - `src/main.rs:333`
617. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:335`
618. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish_test::*`
   - Examples:
     - `src/main.rs:337`
619. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:339`
620. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError_test::*`
   - Examples:
     - `src/main.rs:341`
621. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:343`
622. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction_test::*`
   - Examples:
     - `src/main.rs:345`
623. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:347`
624. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn_test::*`
   - Examples:
     - `src/main.rs:349`
625. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:351`
626. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull_test::*`
   - Examples:
     - `src/main.rs:353`
627. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:355`
628. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish_test::*`
   - Examples:
     - `src/main.rs:357`
629. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:359`
630. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot_test::*`
   - Examples:
     - `src/main.rs:361`
631. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:363`
632. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish_test::*`
   - Examples:
     - `src/main.rs:365`
633. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:367`
634. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber_test::*`
   - Examples:
     - `src/main.rs:369`
635. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:371`
636. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType_test::*`
   - Examples:
     - `src/main.rs:373`
637. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPlainObject_test::*`
   - Examples:
     - `src/main.rs:377`
638. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:379`
639. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise_test::*`
   - Examples:
     - `src/main.rs:381`
640. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:383`
641. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual_test::*`
   - Examples:
     - `src/main.rs:385`
642. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:387`
643. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual_test::*`
   - Examples:
     - `src/main.rs:389`
644. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:391`
645. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString_test::*`
   - Examples:
     - `src/main.rs:393`
646. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:395`
647. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol_test::*`
   - Examples:
     - `src/main.rs:397`
648. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:399`
649. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy_test::*`
   - Examples:
     - `src/main.rs:401`
650. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:403`
651. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join_test::*`
   - Examples:
     - `src/main.rs:405`
652. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:407`
653. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys_test::*`
   - Examples:
     - `src/main.rs:409`
654. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:411`
655. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last_test::*`
   - Examples:
     - `src/main.rs:413`
656. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `lazyInvocationCounter::*`
   - Examples:
     - `src/main.rs:417`
657. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:419`
658. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length_test::*`
   - Examples:
     - `src/main.rs:421`
659. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:425`
660. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys_test::*`
   - Examples:
     - `src/main.rs:427`
661. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:429`
662. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj_test::*`
   - Examples:
     - `src/main.rs:431`
663. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:433`
664. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues_test::*`
   - Examples:
     - `src/main.rs:435`
665. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:437`
666. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback_test::*`
   - Examples:
     - `src/main.rs:439`
667. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map_test::*`
   - Examples:
     - `src/main.rs:441`
668. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:443`
669. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:445`
670. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy_test::*`
   - Examples:
     - `src/main.rs:447`
671. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean_test::*`
   - Examples:
     - `src/main.rs:449`
672. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:451`
673. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median_test::*`
   - Examples:
     - `src/main.rs:453`
674. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:455`
675. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:457`
676. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll_test::*`
   - Examples:
     - `src/main.rs:459`
677. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:461`
678. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep_test::*`
   - Examples:
     - `src/main.rs:463`
679. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge_test::*`
   - Examples:
     - `src/main.rs:465`
680. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:467`
681. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply_test::*`
   - Examples:
     - `src/main.rs:469`
682. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:471`
683. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy_test::*`
   - Examples:
     - `src/main.rs:473`
684. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:475`
685. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf_test::*`
   - Examples:
     - `src/main.rs:477`
686. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:479`
687. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:481`
688. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy_test::*`
   - Examples:
     - `src/main.rs:483`
689. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit_test::*`
   - Examples:
     - `src/main.rs:485`
690. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:487`
691. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once_test::*`
   - Examples:
     - `src/main.rs:489`
692. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:491`
693. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only_test::*`
   - Examples:
     - `src/main.rs:493`
694. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:495`
695. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind_test::*`
   - Examples:
     - `src/main.rs:497`
696. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:499`
697. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind_test::*`
   - Examples:
     - `src/main.rs:501`
698. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:503`
699. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition_test::*`
   - Examples:
     - `src/main.rs:505`
700. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:507`
701. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr_test::*`
   - Examples:
     - `src/main.rs:509`
702. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:511`
703. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:513`
704. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy_test::*`
   - Examples:
     - `src/main.rs:515`
705. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick_test::*`
   - Examples:
     - `src/main.rs:517`
706. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pipe_test::*`
   - Examples:
     - `src/main.rs:521`
707. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:523`
708. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped_test::*`
   - Examples:
     - `src/main.rs:525`
709. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:527`
710. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product_test::*`
   - Examples:
     - `src/main.rs:529`
711. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:531`
712. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop_test::*`
   - Examples:
     - `src/main.rs:533`
713. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:535`
714. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject_test::*`
   - Examples:
     - `src/main.rs:537`
715. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry::*`
   - Examples:
     - `src/main.rs:539`
716. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purryFromLazy_test::*`
   - Examples:
     - `src/main.rs:543`
717. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry_test::*`
   - Examples:
     - `src/main.rs:549`
718. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt_test::*`
   - Examples:
     - `src/main.rs:555`
719. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger_test::*`
   - Examples:
     - `src/main.rs:559`
720. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:561`
721. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString_test::*`
   - Examples:
     - `src/main.rs:563`
722. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:565`
723. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range_test::*`
   - Examples:
     - `src/main.rs:567`
724. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:569`
725. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy_test::*`
   - Examples:
     - `src/main.rs:571`
726. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce_test::*`
   - Examples:
     - `src/main.rs:575`
727. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:577`
728. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse_test::*`
   - Examples:
     - `src/main.rs:579`
729. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:581`
730. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round_test::*`
   - Examples:
     - `src/main.rs:583`
731. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:585`
732. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample_test::*`
   - Examples:
     - `src/main.rs:587`
733. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:589`
734. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:591`
735. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath_test::*`
   - Examples:
     - `src/main.rs:593`
736. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set_test::*`
   - Examples:
     - `src/main.rs:595`
737. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:597`
738. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle_test::*`
   - Examples:
     - `src/main.rs:599`
739. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:603`
740. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString_test::*`
   - Examples:
     - `src/main.rs:605`
741. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:607`
742. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:609`
743. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy_test::*`
   - Examples:
     - `src/main.rs:611`
744. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort_test::*`
   - Examples:
     - `src/main.rs:613`
745. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:615`
746. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy_test::*`
   - Examples:
     - `src/main.rs:619`
747. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:621`
748. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith_test::*`
   - Examples:
     - `src/main.rs:623`
749. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex_test::*`
   - Examples:
     - `src/main.rs:625`
750. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:627`
751. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy_test::*`
   - Examples:
     - `src/main.rs:631`
752. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex_test::*`
   - Examples:
     - `src/main.rs:633`
753. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:635`
754. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice_test::*`
   - Examples:
     - `src/main.rs:637`
755. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:639`
756. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:641`
757. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt_test::*`
   - Examples:
     - `src/main.rs:643`
758. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:645`
759. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen_test::*`
   - Examples:
     - `src/main.rs:647`
760. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split_test::*`
   - Examples:
     - `src/main.rs:649`
761. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:651`
762. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:653`
763. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith_test::*`
   - Examples:
     - `src/main.rs:655`
764. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:657`
765. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath_test::*`
   - Examples:
     - `src/main.rs:659`
766. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:661`
767. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract_test::*`
   - Examples:
     - `src/main.rs:663`
768. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:667`
769. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy_test::*`
   - Examples:
     - `src/main.rs:669`
770. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sum_test::*`
   - Examples:
     - `src/main.rs:671`
771. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:675`
772. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices_test::*`
   - Examples:
     - `src/main.rs:677`
773. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:679`
774. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps_test::*`
   - Examples:
     - `src/main.rs:681`
775. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:683`
776. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:685`
777. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy_test::*`
   - Examples:
     - `src/main.rs:687`
778. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:689`
779. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:691`
780. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile_test::*`
   - Examples:
     - `src/main.rs:693`
781. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast_test::*`
   - Examples:
     - `src/main.rs:695`
782. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:697`
783. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile_test::*`
   - Examples:
     - `src/main.rs:699`
784. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take_test::*`
   - Examples:
     - `src/main.rs:701`
785. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:703`
786. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap_test::*`
   - Examples:
     - `src/main.rs:705`
787. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times_test::*`
   - Examples:
     - `src/main.rs:709`
788. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:711`
789. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase_test::*`
   - Examples:
     - `src/main.rs:713`
790. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:715`
791. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase_test::*`
   - Examples:
     - `src/main.rs:717`
792. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:719`
793. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase_test::*`
   - Examples:
     - `src/main.rs:721`
794. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:725`
795. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase_test::*`
   - Examples:
     - `src/main.rs:727`
796. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:729`
797. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase_test::*`
   - Examples:
     - `src/main.rs:731`
798. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:733`
799. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase_test::*`
   - Examples:
     - `src/main.rs:735`
800. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:737`
801. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate_test::*`
   - Examples:
     - `src/main.rs:739`
802. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `typesDataProvider::*`
   - Examples:
     - `src/main.rs:741`
803. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:743`
804. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize_test::*`
   - Examples:
     - `src/main.rs:745`
805. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:749`
806. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy_test::*`
   - Examples:
     - `src/main.rs:751`
807. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:753`
808. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith_test::*`
   - Examples:
     - `src/main.rs:755`
809. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique_test::*`
   - Examples:
     - `src/main.rs:757`
810. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:761`
811. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values_test::*`
   - Examples:
     - `src/main.rs:763`
812. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:765`
813. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when_test::*`
   - Examples:
     - `src/main.rs:767`
814. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `withPrecision_test::*`
   - Examples:
     - `src/main.rs:771`
815. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `words_test::*`
   - Examples:
     - `src/main.rs:775`
816. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:777`
817. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:779`
818. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith_test::*`
   - Examples:
     - `src/main.rs:781`
819. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip_test::*`
   - Examples:
     - `src/main.rs:783`
820. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:88`
821. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Updating crates.io index
     Locking 62 packages to latest Rust 1.93.0 compatible versions
      Adding rand v0.9.4 (available: v0.10.1)
   Compiling proc-macro2 v1.0.106
   Compiling quote v1.0.45
   Compiling unicode-ident v1.0.24
   Compiling libc v0.2.186
   Compiling zerocopy v0.8.48
   Compiling getrandom v0.3.4
   Compiling autocfg v1.5.0
    Checking cfg-if v1.0.4
   Compiling serde_core v1.0.228
    Checking memchr v2.8.0
   Compiling zmij v1.0.21
    Checking tinyvec_macros v0.1.1
   Compiling serde_json v1.0.149
    Checking aho-corasick v1.1.4
    Checking regex-syntax v0.8.10
   Compiling num-traits v0.2.19
   Compiling serde v1.0.228
   Compiling syn v2.0.117
    Checking rand_core v0.9.5
    Checking tinyvec v1.11.0
    Checking itoa v1.0.18
    Checking iana-time-zone v0.1.65
    Checking pin-project-lite v0.2.17
    Checking chrono v0.4.44
    Checking unicode-normalization v0.1.25
    Checking regex-automata v0.4.14
   Compiling serde_derive v1.0.228
   Compiling tokio-macros v2.7.0
    Checking ppv-lite86 v0.2.21
    Checking regex v1.12.3
    Checking rand_chacha v0.9.0
    Checking tokio v1.52.3
    Checking rand v0.9.4
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.17s
```
