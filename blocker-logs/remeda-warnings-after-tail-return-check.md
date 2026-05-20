# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `1160`

## Summary By Code

1. **warning** `non_snake_case` - 411 diagnostics
2. **warning** `unused_imports` - 362 diagnostics
3. **warning** `unused_mut` - 283 diagnostics
4. **warning** `unused_assignments` - 64 diagnostics
5. **warning** `unused_parens` - 36 diagnostics
6. **warning** `unreachable_code` - 2 diagnostics
7. **warning** `unused_must_use` - 2 diagnostics

## Groups

1. **warning** `unused_mut` - 283 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/add.rs:11`
     - `src/addProp.rs:11`
     - `src/allPass.rs:11`
     - `src/anyPass.rs:11`
     - `src/capitalize.rs:11`
2. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:40`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
3. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:85`
     - `src/allPass_test.rs:86`
     - `src/anyPass_test.rs:85`
     - `src/anyPass_test.rs:86`
     - `src/purryOrderRules.rs:206`
4. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/pipe.rs:10`
     - `src/pipe.rs:182`
     - `src/pipe.rs:254`
     - `src/randomBigInt.rs:91`
     - `src/truncate.rs:31`
5. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:218`
     - `src/debounce.rs:195`
     - `src/debounce.rs:190`
     - `src/debounce.rs:116`
     - `src/debounce.rs:68`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:7`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_test.rs:7`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
8. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:160`
     - `src/truncate.rs:145`
     - `src/truncate.rs:119`
     - `src/truncate.rs:104`
9. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
10. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
11. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
12. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
13. **warning** `unreachable_code` - 2 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/isDeepEqual.rs:305`
     - `src/sample.rs:85`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `flush` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:8`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:8`
16. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:130`
     - `src/truncate.rs:89`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/fromKeys.rs:36`
     - `src/omit.rs:129`
18. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:151`
     - `src/truncate.rs:110`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
20. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
21. **warning** `unused_must_use` - 2 occurrences
   - Message: unused return value of `clone` that must be used
   - Examples:
     - `src/doNothing.rs:9`
     - `src/funnel_lodash_debounce_test.rs:92`
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
     - `src/binarySearchCutoffIndex.rs:35`
65. **warning** `non_snake_case` - 1 occurrence
   - Message: function `countBy` should have a snake case name
   - Examples:
     - `src/countBy.rs:72`
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
     - `src/dropFirstBy.rs:63`
71. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLastWhile` should have a snake case name
   - Examples:
     - `src/dropLastWhile.rs:60`
72. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLast` should have a snake case name
   - Examples:
     - `src/dropLast.rs:35`
73. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropWhile` should have a snake case name
   - Examples:
     - `src/dropWhile.rs:49`
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
     - `src/findLastIndex.rs:37`
77. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findLast` should have a snake case name
   - Examples:
     - `src/findLast.rs:37`
78. **warning** `non_snake_case` - 1 occurrence
   - Message: function `firstBy` should have a snake case name
   - Examples:
     - `src/firstBy.rs:55`
79. **warning** `non_snake_case` - 1 occurrence
   - Message: function `flatMap` should have a snake case name
   - Examples:
     - `src/flatMap.rs:21`
80. **warning** `non_snake_case` - 1 occurrence
   - Message: function `forEachObj` should have a snake case name
   - Examples:
     - `src/forEachObj.rs:42`
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
     - `src/fromKeys.rs:44`
84. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupByProp_test` should have a snake case name
   - Examples:
     - `src/groupByProp_test.rs:276`
85. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupByProp` should have a snake case name
   - Examples:
     - `src/groupByProp.rs:66`
86. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupBy` should have a snake case name
   - Examples:
     - `src/groupBy.rs:69`
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
     - `src/hasSubObject.rs:57`
90. **warning** `non_snake_case` - 1 occurrence
   - Message: function `indexBy` should have a snake case name
   - Examples:
     - `src/indexBy.rs:46`
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
     - `src/isDeepEqual.rs:308`
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
     - `src/isShallowEqual.rs:198`
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
     - `src/mapKeys.rs:47`
120. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapToObj` should have a snake case name
   - Examples:
     - `src/mapToObj.rs:52`
121. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapValues` should have a snake case name
   - Examples:
     - `src/mapValues.rs:47`
122. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapWithFeedback` should have a snake case name
   - Examples:
     - `src/mapWithFeedback.rs:12`
123. **warning** `non_snake_case` - 1 occurrence
   - Message: function `meanBy` should have a snake case name
   - Examples:
     - `src/meanBy.rs:57`
124. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeAll` should have a snake case name
   - Examples:
     - `src/mergeAll.rs:28`
125. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeDeep` should have a snake case name
   - Examples:
     - `src/mergeDeep.rs:71`
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
     - `src/omitBy.rs:52`
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
     - `src/pathOr.rs:47`
133. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pickBy` should have a snake case name
   - Examples:
     - `src/pickBy.rs:51`
134. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pullObject` should have a snake case name
   - Examples:
     - `src/pullObject.rs:50`
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
     - `src/quickSelect.rs:99`
139. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomBigInt_test` should have a snake case name
   - Examples:
     - `src/randomBigInt_test.rs:552`
140. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomBigInt` should have a snake case name
   - Examples:
     - `src/randomBigInt.rs:111`
141. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomInteger_test` should have a snake case name
   - Examples:
     - `src/randomInteger_test.rs:231`
142. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomInteger` should have a snake case name
   - Examples:
     - `src/randomInteger.rs:45`
143. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomString` should have a snake case name
   - Examples:
     - `src/randomString.rs:44`
144. **warning** `non_snake_case` - 1 occurrence
   - Message: function `rankBy` should have a snake case name
   - Examples:
     - `src/rankBy.rs:44`
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
     - `src/stringToPath.rs:82`
160. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sumBy` should have a snake case name
   - Examples:
     - `src/sumBy.rs:79`
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
     - `src/takeFirstBy.rs:54`
165. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLastWhile` should have a snake case name
   - Examples:
     - `src/takeLastWhile.rs:58`
166. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLast` should have a snake case name
   - Examples:
     - `src/takeLast.rs:37`
167. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeWhile` should have a snake case name
   - Examples:
     - `src/takeWhile.rs:52`
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
433. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
434. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
435. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
436. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
437. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
438. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
439. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
440. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
441. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/pipe.rs:8`
442. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:302`
443. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_cool_down_end` is never read
   - Examples:
     - `src/debounce.rs:8`
444. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_debounced_call` is never read
   - Examples:
     - `src/debounce.rs:9`
445. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_invoke` is never read
   - Examples:
     - `src/debounce.rs:7`
446. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
447. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
448. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_fn` is never read
   - Examples:
     - `src/pipe.rs:253`
449. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_sequence` is never read
   - Examples:
     - `src/pipe.rs:9`
450. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
451. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:47`
452. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
453. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
454. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
455. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
456. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
457. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
458. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
459. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
460. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
461. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
462. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
463. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
464. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
465. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
466. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
467. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
468. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
469. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
470. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
471. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
472. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
473. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
474. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
475. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
476. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
477. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
478. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
479. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
480. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
481. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
482. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
483. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
484. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
485. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
486. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
487. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
488. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
489. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
490. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
491. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
492. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
493. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
494. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
495. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp_test::*`
   - Examples:
     - `src/main.rs:83`
496. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add_test::*`
   - Examples:
     - `src/main.rs:85`
497. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:87`
498. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass_test::*`
   - Examples:
     - `src/main.rs:89`
499. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:91`
500. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass_test::*`
   - Examples:
     - `src/main.rs:93`
501. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `binarySearchCutoffIndex_test::*`
   - Examples:
     - `src/main.rs:97`
502. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:99`
503. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize_test::*`
   - Examples:
     - `src/main.rs:101`
504. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:103`
505. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil_test::*`
   - Examples:
     - `src/main.rs:105`
506. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:107`
507. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk_test::*`
   - Examples:
     - `src/main.rs:109`
508. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:111`
509. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp_test::*`
   - Examples:
     - `src/main.rs:113`
510. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:115`
511. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone_test::*`
   - Examples:
     - `src/main.rs:117`
512. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:119`
513. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat_test::*`
   - Examples:
     - `src/main.rs:121`
514. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:123`
515. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional_test::*`
   - Examples:
     - `src/main.rs:125`
516. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:127`
517. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant_test::*`
   - Examples:
     - `src/main.rs:129`
518. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:131`
519. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy_test::*`
   - Examples:
     - `src/main.rs:133`
520. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:135`
521. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce_test::*`
   - Examples:
     - `src/main.rs:137`
522. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:139`
523. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo_test::*`
   - Examples:
     - `src/main.rs:141`
524. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:143`
525. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:145`
526. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith_test::*`
   - Examples:
     - `src/main.rs:147`
527. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference_test::*`
   - Examples:
     - `src/main.rs:149`
528. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:151`
529. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide_test::*`
   - Examples:
     - `src/main.rs:153`
530. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:155`
531. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing_test::*`
   - Examples:
     - `src/main.rs:157`
532. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:159`
533. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:161`
534. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy_test::*`
   - Examples:
     - `src/main.rs:163`
535. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:165`
536. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:167`
537. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile_test::*`
   - Examples:
     - `src/main.rs:169`
538. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast_test::*`
   - Examples:
     - `src/main.rs:171`
539. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:173`
540. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile_test::*`
   - Examples:
     - `src/main.rs:175`
541. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop_test::*`
   - Examples:
     - `src/main.rs:177`
542. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:179`
543. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith_test::*`
   - Examples:
     - `src/main.rs:181`
544. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:183`
545. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries_test::*`
   - Examples:
     - `src/main.rs:185`
546. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:187`
547. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve_test::*`
   - Examples:
     - `src/main.rs:189`
548. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:191`
549. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter_test::*`
   - Examples:
     - `src/main.rs:193`
550. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:195`
551. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:197`
552. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex_test::*`
   - Examples:
     - `src/main.rs:199`
553. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:201`
554. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:203`
555. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex_test::*`
   - Examples:
     - `src/main.rs:205`
556. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast_test::*`
   - Examples:
     - `src/main.rs:207`
557. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find_test::*`
   - Examples:
     - `src/main.rs:209`
558. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:211`
559. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:213`
560. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy_test::*`
   - Examples:
     - `src/main.rs:215`
561. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first_test::*`
   - Examples:
     - `src/main.rs:217`
562. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:219`
563. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:221`
564. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap_test::*`
   - Examples:
     - `src/main.rs:223`
565. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat_test::*`
   - Examples:
     - `src/main.rs:225`
566. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:227`
567. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor_test::*`
   - Examples:
     - `src/main.rs:229`
568. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:231`
569. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:233`
570. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj_test::*`
   - Examples:
     - `src/main.rs:235`
571. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach_test::*`
   - Examples:
     - `src/main.rs:237`
572. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:239`
573. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries_test::*`
   - Examples:
     - `src/main.rs:241`
574. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:243`
575. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys_test::*`
   - Examples:
     - `src/main.rs:245`
576. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_test::*`
   - Examples:
     - `src/main.rs:249`
577. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:251`
578. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_test::*`
   - Examples:
     - `src/main.rs:253`
579. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:255`
580. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_reference_batch_test::*`
   - Examples:
     - `src/main.rs:257`
581. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_remeda_debounce_test::*`
   - Examples:
     - `src/main.rs:259`
582. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_test::*`
   - Examples:
     - `src/main.rs:261`
583. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:263`
584. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:265`
585. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp_test::*`
   - Examples:
     - `src/main.rs:267`
586. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy_test::*`
   - Examples:
     - `src/main.rs:269`
587. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasAtLeast_test::*`
   - Examples:
     - `src/main.rs:273`
588. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:275`
589. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp_test::*`
   - Examples:
     - `src/main.rs:277`
590. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:279`
591. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject_test::*`
   - Examples:
     - `src/main.rs:281`
592. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `heap_test::*`
   - Examples:
     - `src/main.rs:285`
593. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:287`
594. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity_test::*`
   - Examples:
     - `src/main.rs:289`
595. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:291`
596. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy_test::*`
   - Examples:
     - `src/main.rs:293`
597. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:295`
598. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:297`
599. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith_test::*`
   - Examples:
     - `src/main.rs:299`
600. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection_test::*`
   - Examples:
     - `src/main.rs:301`
601. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:303`
602. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert_test::*`
   - Examples:
     - `src/main.rs:305`
603. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:307`
604. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray_test::*`
   - Examples:
     - `src/main.rs:309`
605. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:311`
606. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt_test::*`
   - Examples:
     - `src/main.rs:313`
607. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:315`
608. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean_test::*`
   - Examples:
     - `src/main.rs:317`
609. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:319`
610. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate_test::*`
   - Examples:
     - `src/main.rs:321`
611. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDeepEqual_test::*`
   - Examples:
     - `src/main.rs:325`
612. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:327`
613. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined_test::*`
   - Examples:
     - `src/main.rs:329`
614. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:331`
615. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty_test::*`
   - Examples:
     - `src/main.rs:333`
616. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:335`
617. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish_test::*`
   - Examples:
     - `src/main.rs:337`
618. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:339`
619. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError_test::*`
   - Examples:
     - `src/main.rs:341`
620. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:343`
621. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction_test::*`
   - Examples:
     - `src/main.rs:345`
622. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:347`
623. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn_test::*`
   - Examples:
     - `src/main.rs:349`
624. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:351`
625. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull_test::*`
   - Examples:
     - `src/main.rs:353`
626. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:355`
627. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish_test::*`
   - Examples:
     - `src/main.rs:357`
628. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:359`
629. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot_test::*`
   - Examples:
     - `src/main.rs:361`
630. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:363`
631. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish_test::*`
   - Examples:
     - `src/main.rs:365`
632. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:367`
633. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber_test::*`
   - Examples:
     - `src/main.rs:369`
634. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:371`
635. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType_test::*`
   - Examples:
     - `src/main.rs:373`
636. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPlainObject_test::*`
   - Examples:
     - `src/main.rs:377`
637. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:379`
638. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise_test::*`
   - Examples:
     - `src/main.rs:381`
639. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:383`
640. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual_test::*`
   - Examples:
     - `src/main.rs:385`
641. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:387`
642. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual_test::*`
   - Examples:
     - `src/main.rs:389`
643. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:391`
644. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString_test::*`
   - Examples:
     - `src/main.rs:393`
645. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:395`
646. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol_test::*`
   - Examples:
     - `src/main.rs:397`
647. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:399`
648. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy_test::*`
   - Examples:
     - `src/main.rs:401`
649. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:403`
650. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join_test::*`
   - Examples:
     - `src/main.rs:405`
651. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:407`
652. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys_test::*`
   - Examples:
     - `src/main.rs:409`
653. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:411`
654. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last_test::*`
   - Examples:
     - `src/main.rs:413`
655. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `lazyInvocationCounter::*`
   - Examples:
     - `src/main.rs:417`
656. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:419`
657. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length_test::*`
   - Examples:
     - `src/main.rs:421`
658. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:425`
659. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys_test::*`
   - Examples:
     - `src/main.rs:427`
660. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:429`
661. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj_test::*`
   - Examples:
     - `src/main.rs:431`
662. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:433`
663. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues_test::*`
   - Examples:
     - `src/main.rs:435`
664. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:437`
665. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback_test::*`
   - Examples:
     - `src/main.rs:439`
666. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map_test::*`
   - Examples:
     - `src/main.rs:441`
667. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:443`
668. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:445`
669. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy_test::*`
   - Examples:
     - `src/main.rs:447`
670. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean_test::*`
   - Examples:
     - `src/main.rs:449`
671. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:451`
672. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median_test::*`
   - Examples:
     - `src/main.rs:453`
673. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:455`
674. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:457`
675. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll_test::*`
   - Examples:
     - `src/main.rs:459`
676. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:461`
677. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep_test::*`
   - Examples:
     - `src/main.rs:463`
678. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge_test::*`
   - Examples:
     - `src/main.rs:465`
679. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:467`
680. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply_test::*`
   - Examples:
     - `src/main.rs:469`
681. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:471`
682. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy_test::*`
   - Examples:
     - `src/main.rs:473`
683. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:475`
684. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf_test::*`
   - Examples:
     - `src/main.rs:477`
685. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:479`
686. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:481`
687. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy_test::*`
   - Examples:
     - `src/main.rs:483`
688. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit_test::*`
   - Examples:
     - `src/main.rs:485`
689. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:487`
690. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once_test::*`
   - Examples:
     - `src/main.rs:489`
691. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:491`
692. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only_test::*`
   - Examples:
     - `src/main.rs:493`
693. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:495`
694. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind_test::*`
   - Examples:
     - `src/main.rs:497`
695. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:499`
696. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind_test::*`
   - Examples:
     - `src/main.rs:501`
697. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:503`
698. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition_test::*`
   - Examples:
     - `src/main.rs:505`
699. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:507`
700. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr_test::*`
   - Examples:
     - `src/main.rs:509`
701. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:511`
702. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:513`
703. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy_test::*`
   - Examples:
     - `src/main.rs:515`
704. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick_test::*`
   - Examples:
     - `src/main.rs:517`
705. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pipe_test::*`
   - Examples:
     - `src/main.rs:521`
706. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:523`
707. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped_test::*`
   - Examples:
     - `src/main.rs:525`
708. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:527`
709. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product_test::*`
   - Examples:
     - `src/main.rs:529`
710. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:531`
711. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop_test::*`
   - Examples:
     - `src/main.rs:533`
712. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:535`
713. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject_test::*`
   - Examples:
     - `src/main.rs:537`
714. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry::*`
   - Examples:
     - `src/main.rs:539`
715. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purryFromLazy_test::*`
   - Examples:
     - `src/main.rs:543`
716. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry_test::*`
   - Examples:
     - `src/main.rs:549`
717. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt_test::*`
   - Examples:
     - `src/main.rs:555`
718. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger_test::*`
   - Examples:
     - `src/main.rs:559`
719. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:561`
720. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString_test::*`
   - Examples:
     - `src/main.rs:563`
721. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:565`
722. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range_test::*`
   - Examples:
     - `src/main.rs:567`
723. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:569`
724. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy_test::*`
   - Examples:
     - `src/main.rs:571`
725. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce_test::*`
   - Examples:
     - `src/main.rs:575`
726. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:577`
727. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse_test::*`
   - Examples:
     - `src/main.rs:579`
728. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:581`
729. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round_test::*`
   - Examples:
     - `src/main.rs:583`
730. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:585`
731. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample_test::*`
   - Examples:
     - `src/main.rs:587`
732. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:589`
733. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:591`
734. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath_test::*`
   - Examples:
     - `src/main.rs:593`
735. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set_test::*`
   - Examples:
     - `src/main.rs:595`
736. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:597`
737. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle_test::*`
   - Examples:
     - `src/main.rs:599`
738. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:603`
739. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString_test::*`
   - Examples:
     - `src/main.rs:605`
740. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:607`
741. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:609`
742. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy_test::*`
   - Examples:
     - `src/main.rs:611`
743. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort_test::*`
   - Examples:
     - `src/main.rs:613`
744. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:615`
745. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy_test::*`
   - Examples:
     - `src/main.rs:619`
746. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:621`
747. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith_test::*`
   - Examples:
     - `src/main.rs:623`
748. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex_test::*`
   - Examples:
     - `src/main.rs:625`
749. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:627`
750. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy_test::*`
   - Examples:
     - `src/main.rs:631`
751. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex_test::*`
   - Examples:
     - `src/main.rs:633`
752. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:635`
753. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice_test::*`
   - Examples:
     - `src/main.rs:637`
754. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:639`
755. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:641`
756. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt_test::*`
   - Examples:
     - `src/main.rs:643`
757. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:645`
758. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen_test::*`
   - Examples:
     - `src/main.rs:647`
759. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split_test::*`
   - Examples:
     - `src/main.rs:649`
760. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:651`
761. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:653`
762. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith_test::*`
   - Examples:
     - `src/main.rs:655`
763. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:657`
764. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath_test::*`
   - Examples:
     - `src/main.rs:659`
765. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:661`
766. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract_test::*`
   - Examples:
     - `src/main.rs:663`
767. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:667`
768. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy_test::*`
   - Examples:
     - `src/main.rs:669`
769. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sum_test::*`
   - Examples:
     - `src/main.rs:671`
770. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:675`
771. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices_test::*`
   - Examples:
     - `src/main.rs:677`
772. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:679`
773. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps_test::*`
   - Examples:
     - `src/main.rs:681`
774. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:683`
775. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:685`
776. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy_test::*`
   - Examples:
     - `src/main.rs:687`
777. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:689`
778. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:691`
779. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile_test::*`
   - Examples:
     - `src/main.rs:693`
780. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast_test::*`
   - Examples:
     - `src/main.rs:695`
781. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:697`
782. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile_test::*`
   - Examples:
     - `src/main.rs:699`
783. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take_test::*`
   - Examples:
     - `src/main.rs:701`
784. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:703`
785. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap_test::*`
   - Examples:
     - `src/main.rs:705`
786. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times_test::*`
   - Examples:
     - `src/main.rs:709`
787. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:711`
788. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase_test::*`
   - Examples:
     - `src/main.rs:713`
789. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:715`
790. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase_test::*`
   - Examples:
     - `src/main.rs:717`
791. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:719`
792. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase_test::*`
   - Examples:
     - `src/main.rs:721`
793. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:725`
794. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase_test::*`
   - Examples:
     - `src/main.rs:727`
795. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:729`
796. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase_test::*`
   - Examples:
     - `src/main.rs:731`
797. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:733`
798. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase_test::*`
   - Examples:
     - `src/main.rs:735`
799. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:737`
800. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate_test::*`
   - Examples:
     - `src/main.rs:739`
801. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `typesDataProvider::*`
   - Examples:
     - `src/main.rs:741`
802. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:743`
803. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize_test::*`
   - Examples:
     - `src/main.rs:745`
804. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:749`
805. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy_test::*`
   - Examples:
     - `src/main.rs:751`
806. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:753`
807. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith_test::*`
   - Examples:
     - `src/main.rs:755`
808. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique_test::*`
   - Examples:
     - `src/main.rs:757`
809. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:761`
810. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values_test::*`
   - Examples:
     - `src/main.rs:763`
811. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:765`
812. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when_test::*`
   - Examples:
     - `src/main.rs:767`
813. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `withPrecision_test::*`
   - Examples:
     - `src/main.rs:771`
814. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `words_test::*`
   - Examples:
     - `src/main.rs:775`
815. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:777`
816. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:779`
817. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith_test::*`
   - Examples:
     - `src/main.rs:781`
818. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip_test::*`
   - Examples:
     - `src/main.rs:783`
819. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:87`
820. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Updating crates.io index
     Locking 62 packages to latest Rust 1.93.0 compatible versions
      Adding rand v0.9.4 (available: v0.10.1)
   Compiling proc-macro2 v1.0.106
   Compiling unicode-ident v1.0.24
   Compiling quote v1.0.45
   Compiling libc v0.2.186
   Compiling zerocopy v0.8.48
   Compiling getrandom v0.3.4
    Checking memchr v2.8.0
   Compiling serde_core v1.0.228
   Compiling autocfg v1.5.0
    Checking cfg-if v1.0.4
   Compiling zmij v1.0.21
    Checking regex-syntax v0.8.10
   Compiling num-traits v0.2.19
   Compiling serde_json v1.0.149
    Checking aho-corasick v1.1.4
    Checking tinyvec_macros v0.1.1
   Compiling serde v1.0.228
    Checking tinyvec v1.11.0
   Compiling syn v2.0.117
    Checking pin-project-lite v0.2.17
    Checking iana-time-zone v0.1.65
    Checking itoa v1.0.18
    Checking chrono v0.4.44
    Checking unicode-normalization v0.1.25
    Checking rand_core v0.9.5
    Checking regex-automata v0.4.14
    Checking regex v1.12.3
    Checking ppv-lite86 v0.2.21
   Compiling serde_derive v1.0.228
   Compiling tokio-macros v2.7.0
    Checking rand_chacha v0.9.0
    Checking rand v0.9.4
    Checking tokio v1.52.3
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.08s
```
