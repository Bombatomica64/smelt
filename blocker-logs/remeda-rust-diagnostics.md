# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `1219`

## Summary By Code

1. **warning** `unused_assignments` - 442 diagnostics
2. **warning** `non_snake_case` - 292 diagnostics
3. **warning** `unused_imports` - 233 diagnostics
4. **warning** `unused_mut` - 130 diagnostics
5. **warning** `unused_parens` - 63 diagnostics
6. **warning** `unreachable_code` - 57 diagnostics
7. **warning** `unused_must_use` - 2 diagnostics

## Groups

1. **warning** `unused_assignments` - 151 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/evolve.rs:325`
     - `src/evolve.rs:316`
     - `src/evolve.rs:307`
     - `src/evolve.rs:298`
     - `src/evolve.rs:289`
2. **warning** `unused_mut` - 130 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/clone.rs:15`
     - `src/clone.rs:78`
     - `src/clone.rs:479`
     - `src/countBy.rs:7`
     - `src/countBy.rs:32`
3. **warning** `unreachable_code` - 57 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/binarySearchCutoffIndex.rs:33`
     - `src/clone.rs:476`
     - `src/clone.rs:909`
     - `src/conditional.rs:392`
     - `src/countBy.rs:63`
4. **warning** `unused_assignments` - 52 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/difference.rs:14`
     - `src/evolve.rs:326`
     - `src/evolve.rs:317`
     - `src/evolve.rs:308`
     - `src/evolve.rs:299`
5. **warning** `unused_parens` - 44 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:40`
     - `src/dropWhile.rs:36`
     - `src/dropWhile.rs:52`
     - `src/dropWhile.rs:68`
     - `src/dropWhile.rs:84`
6. **warning** `unused_imports` - 38 occurrences
   - Message: unused import: `super::*`
   - Examples:
     - `src/ArrayAt.rs:4`
     - `src/ArrayRequiredPrefix.rs:4`
     - `src/BoundedPartial.rs:4`
     - `src/ClampedIntegerSubtract.rs:4`
     - `src/CoercedArray.rs:4`
7. **warning** `unused_assignments` - 26 occurrences
   - Message: value assigned to `current` is never read
   - Examples:
     - `src/conditional.rs:384`
     - `src/conditional.rs:370`
     - `src/conditional.rs:356`
     - `src/conditional.rs:342`
     - `src/conditional.rs:328`
8. **warning** `unused_assignments` - 25 occurrences
   - Message: value assigned to `random_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:318`
     - `src/randomBigInt.rs:307`
     - `src/randomBigInt.rs:296`
     - `src/randomBigInt.rs:285`
     - `src/randomBigInt.rs:274`
9. **warning** `unused_assignments` - 18 occurrences
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/clone.rs:482`
     - `src/countBy.rs:18`
     - `src/dropFirstBy.rs:19`
     - `src/dropWhile.rs:18`
     - `src/findLast.rs:16`
10. **warning** `unused_assignments` - 15 occurrences
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/clone.rs:481`
     - `src/countBy.rs:17`
     - `src/dropWhile.rs:17`
     - `src/fromKeys.rs:17`
     - `src/indexBy.rs:17`
11. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/purryOrderRules.rs:225`
     - `src/purryOrderRules.rs:226`
     - `src/range.rs:48`
     - `src/range.rs:50`
     - `src/sortedIndex.rs:18`
12. **warning** `unused_assignments` - 6 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/conditional.rs:398`
     - `src/dropFirstBy.rs:18`
     - `src/firstBy.rs:17`
     - `src/purryFromLazy.rs:8`
     - `src/purryOrderRules.rs:174`
13. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `out` is never read
   - Examples:
     - `src/dropFirstBy.rs:17`
     - `src/evolve.rs:16`
     - `src/omit.rs:18`
     - `src/product.rs:16`
     - `src/sum.rs:16`
14. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `remaining` is never read
   - Examples:
     - `src/difference.rs:13`
     - `src/intersection.rs:13`
     - `src/omit.rs:17`
     - `src/setPath.rs:18`
     - `src/take.rs:17`
15. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/debounce.rs:200`
     - `src/debounce.rs:180`
     - `src/debounce.rs:175`
     - `src/debounce.rs:107`
     - `src/debounce.rs:65`
16. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:236`
     - `src/funnel.rs:219`
     - `src/funnel.rs:176`
     - `src/funnel.rs:159`
17. **warning** `unused_parens` - 4 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/range.rs:50`
     - `src/toCamelCase.rs:37`
     - `src/toCamelCase.rs:39`
18. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `cool_down_timeout_id` is never read
   - Examples:
     - `src/debounce.rs:177`
     - `src/debounce.rs:182`
     - `src/debounce.rs:10`
19. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `items` is never read
   - Examples:
     - `src/groupBy.rs:18`
     - `src/groupByProp.rs:18`
     - `src/pipe.rs:14`
20. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:22`
     - `src/truncate.rs:131`
     - `src/truncate.rs:101`
21. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/debounce.rs:13`
     - `src/randomBigInt.rs:13`
     - `src/main.rs:573`
22. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:22`
     - `src/conditional.rs:386`
     - `src/conditional.rs:418`
23. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:68`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
24. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:153`
     - `src/funnel.rs:213`
     - `src/splitAt.rs:37`
25. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:110`
     - `src/truncate.rs:80`
26. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `heap` is never read
   - Examples:
     - `src/dropFirstBy.rs:16`
     - `src/takeFirstBy.rs:16`
27. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `length` is never read
   - Examples:
     - `src/range.rs:18`
     - `src/times.rs:16`
28. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `pivot_index` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:7`
     - `src/quickSelect.rs:7`
29. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rounded` is never read
   - Examples:
     - `src/range.rs:57`
     - `src/withPrecision.rs:46`
30. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/meanBy.rs:16`
     - `src/sumBy.rs:17`
31. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:215`
     - `src/debounce.rs:204`
32. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `value_b` is never read
   - Examples:
     - `src/isShallowEqual.rs:19`
     - `src/isShallowEqual.rs:813`
33. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:21`
     - `src/conditional.rs:417`
34. **warning** `unused_must_use` - 2 occurrences
   - Message: unused return value of `clone` that must be used
   - Examples:
     - `src/doNothing.rs:9`
     - `src/take.rs:31`
35. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ArrayAt` should have a snake case name
   - Examples:
     - `src/ArrayAt.rs:6`
36. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ArrayRequiredPrefix` should have a snake case name
   - Examples:
     - `src/ArrayRequiredPrefix.rs:6`
37. **warning** `non_snake_case` - 1 occurrence
   - Message: function `BoundedPartial` should have a snake case name
   - Examples:
     - `src/BoundedPartial.rs:6`
38. **warning** `non_snake_case` - 1 occurrence
   - Message: function `BrandedReturn` should have a snake case name
   - Examples:
     - `src/BrandedReturn.rs:6`
39. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ClampedIntegerSubtract` should have a snake case name
   - Examples:
     - `src/ClampedIntegerSubtract.rs:6`
40. **warning** `non_snake_case` - 1 occurrence
   - Message: function `CoercedArray` should have a snake case name
   - Examples:
     - `src/CoercedArray.rs:6`
41. **warning** `non_snake_case` - 1 occurrence
   - Message: function `CompareFunction` should have a snake case name
   - Examples:
     - `src/CompareFunction.rs:6`
42. **warning** `non_snake_case` - 1 occurrence
   - Message: function `Deduped` should have a snake case name
   - Examples:
     - `src/Deduped.rs:6`
43. **warning** `non_snake_case` - 1 occurrence
   - Message: function `DisjointUnionFields` should have a snake case name
   - Examples:
     - `src/DisjointUnionFields.rs:6`
44. **warning** `non_snake_case` - 1 occurrence
   - Message: function `EnumerableStringKeyOf` should have a snake case name
   - Examples:
     - `src/EnumerableStringKeyOf.rs:6`
45. **warning** `non_snake_case` - 1 occurrence
   - Message: function `EnumerableStringKeyedValueOf` should have a snake case name
   - Examples:
     - `src/EnumerableStringKeyedValueOf.rs:6`
46. **warning** `non_snake_case` - 1 occurrence
   - Message: function `FilteredArray` should have a snake case name
   - Examples:
     - `src/FilteredArray.rs:6`
47. **warning** `non_snake_case` - 1 occurrence
   - Message: function `GuardType` should have a snake case name
   - Examples:
     - `src/GuardType.rs:6`
48. **warning** `non_snake_case` - 1 occurrence
   - Message: function `HasWritableKeys` should have a snake case name
   - Examples:
     - `src/HasWritableKeys.rs:6`
49. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IntRangeInclusive` should have a snake case name
   - Examples:
     - `src/IntRangeInclusive.rs:6`
50. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IsBoundedRecord` should have a snake case name
   - Examples:
     - `src/IsBoundedRecord.rs:6`
51. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IsBounded` should have a snake case name
   - Examples:
     - `src/IsBounded.rs:6`
52. **warning** `non_snake_case` - 1 occurrence
   - Message: function `IterableContainer` should have a snake case name
   - Examples:
     - `src/IterableContainer.rs:6`
53. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyDefinition` should have a snake case name
   - Examples:
     - `src/LazyDefinition.rs:6`
54. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyEvaluator` should have a snake case name
   - Examples:
     - `src/LazyEvaluator.rs:6`
55. **warning** `non_snake_case` - 1 occurrence
   - Message: function `LazyResult` should have a snake case name
   - Examples:
     - `src/LazyResult.rs:6`
56. **warning** `non_snake_case` - 1 occurrence
   - Message: function `Mapped` should have a snake case name
   - Examples:
     - `src/Mapped.rs:6`
57. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NTuple` should have a snake case name
   - Examples:
     - `src/NTuple.rs:6`
58. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NarrowedTo` should have a snake case name
   - Examples:
     - `src/NarrowedTo.rs:6`
59. **warning** `non_snake_case` - 1 occurrence
   - Message: function `NonEmptyArray` should have a snake case name
   - Examples:
     - `src/NonEmptyArray.rs:6`
60. **warning** `non_snake_case` - 1 occurrence
   - Message: function `OptionalOptionsWithDefaults` should have a snake case name
   - Examples:
     - `src/OptionalOptionsWithDefaults.rs:6`
61. **warning** `non_snake_case` - 1 occurrence
   - Message: function `PartialArray` should have a snake case name
   - Examples:
     - `src/PartialArray.rs:6`
62. **warning** `non_snake_case` - 1 occurrence
   - Message: function `PartitionByUnion` should have a snake case name
   - Examples:
     - `src/PartitionByUnion.rs:6`
63. **warning** `non_snake_case` - 1 occurrence
   - Message: function `RemedaTypeError` should have a snake case name
   - Examples:
     - `src/RemedaTypeError.rs:6`
64. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ReorderedArray` should have a snake case name
   - Examples:
     - `src/ReorderedArray.rs:6`
65. **warning** `non_snake_case` - 1 occurrence
   - Message: function `SimplifiedWritable` should have a snake case name
   - Examples:
     - `src/SimplifiedWritable.rs:6`
66. **warning** `non_snake_case` - 1 occurrence
   - Message: function `StrictFunction` should have a snake case name
   - Examples:
     - `src/StrictFunction.rs:6`
67. **warning** `non_snake_case` - 1 occurrence
   - Message: function `StringLength` should have a snake case name
   - Examples:
     - `src/StringLength.rs:6`
68. **warning** `non_snake_case` - 1 occurrence
   - Message: function `ToString` should have a snake case name
   - Examples:
     - `src/ToString.rs:6`
69. **warning** `non_snake_case` - 1 occurrence
   - Message: function `TupleParts` should have a snake case name
   - Examples:
     - `src/TupleParts.rs:6`
70. **warning** `non_snake_case` - 1 occurrence
   - Message: function `TupleSplits` should have a snake case name
   - Examples:
     - `src/TupleSplits.rs:6`
71. **warning** `non_snake_case` - 1 occurrence
   - Message: function `UpsertProp` should have a snake case name
   - Examples:
     - `src/UpsertProp.rs:6`
72. **warning** `non_snake_case` - 1 occurrence
   - Message: function `addProp` should have a snake case name
   - Examples:
     - `src/addProp.rs:22`
73. **warning** `non_snake_case` - 1 occurrence
   - Message: function `allPass` should have a snake case name
   - Examples:
     - `src/allPass.rs:24`
74. **warning** `non_snake_case` - 1 occurrence
   - Message: function `anyPass` should have a snake case name
   - Examples:
     - `src/anyPass.rs:24`
75. **warning** `non_snake_case` - 1 occurrence
   - Message: function `binarySearchCutoffIndex` should have a snake case name
   - Examples:
     - `src/binarySearchCutoffIndex.rs:36`
76. **warning** `non_snake_case` - 1 occurrence
   - Message: function `countBy` should have a snake case name
   - Examples:
     - `src/countBy.rs:66`
77. **warning** `non_snake_case` - 1 occurrence
   - Message: function `defaultTo` should have a snake case name
   - Examples:
     - `src/defaultTo.rs:21`
78. **warning** `non_snake_case` - 1 occurrence
   - Message: function `differenceWith` should have a snake case name
   - Examples:
     - `src/differenceWith.rs:12`
79. **warning** `non_snake_case` - 1 occurrence
   - Message: function `doNothing` should have a snake case name
   - Examples:
     - `src/doNothing.rs:18`
80. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropFirstBy` should have a snake case name
   - Examples:
     - `src/dropFirstBy.rs:399`
81. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLastWhile` should have a snake case name
   - Examples:
     - `src/dropLastWhile.rs:628`
82. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropLast` should have a snake case name
   - Examples:
     - `src/dropLast.rs:35`
83. **warning** `non_snake_case` - 1 occurrence
   - Message: function `dropWhile` should have a snake case name
   - Examples:
     - `src/dropWhile.rs:543`
84. **warning** `non_snake_case` - 1 occurrence
   - Message: function `endsWith` should have a snake case name
   - Examples:
     - `src/endsWith.rs:22`
85. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findIndex` should have a snake case name
   - Examples:
     - `src/findIndex.rs:20`
86. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findLastIndex` should have a snake case name
   - Examples:
     - `src/findLastIndex.rs:498`
87. **warning** `non_snake_case` - 1 occurrence
   - Message: function `findLast` should have a snake case name
   - Examples:
     - `src/findLast.rs:498`
88. **warning** `non_snake_case` - 1 occurrence
   - Message: function `firstBy` should have a snake case name
   - Examples:
     - `src/firstBy.rs:54`
89. **warning** `non_snake_case` - 1 occurrence
   - Message: function `flatMap` should have a snake case name
   - Examples:
     - `src/flatMap.rs:21`
90. **warning** `non_snake_case` - 1 occurrence
   - Message: function `forEachObj` should have a snake case name
   - Examples:
     - `src/forEachObj.rs:37`
91. **warning** `non_snake_case` - 1 occurrence
   - Message: function `forEach` should have a snake case name
   - Examples:
     - `src/forEach.rs:21`
92. **warning** `non_snake_case` - 1 occurrence
   - Message: function `fromEntries` should have a snake case name
   - Examples:
     - `src/fromEntries.rs:14`
93. **warning** `non_snake_case` - 1 occurrence
   - Message: function `fromKeys` should have a snake case name
   - Examples:
     - `src/fromKeys.rs:39`
94. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupByProp` should have a snake case name
   - Examples:
     - `src/groupByProp.rs:67`
95. **warning** `non_snake_case` - 1 occurrence
   - Message: function `groupBy` should have a snake case name
   - Examples:
     - `src/groupBy.rs:70`
96. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasAtLeast` should have a snake case name
   - Examples:
     - `src/hasAtLeast.rs:21`
97. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasProp` should have a snake case name
   - Examples:
     - `src/hasProp.rs:22`
98. **warning** `non_snake_case` - 1 occurrence
   - Message: function `hasSubObject` should have a snake case name
   - Examples:
     - `src/hasSubObject.rs:465`
99. **warning** `non_snake_case` - 1 occurrence
   - Message: function `indexBy` should have a snake case name
   - Examples:
     - `src/indexBy.rs:41`
100. **warning** `non_snake_case` - 1 occurrence
   - Message: function `intersectionWith` should have a snake case name
   - Examples:
     - `src/intersectionWith.rs:12`
101. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isArray` should have a snake case name
   - Examples:
     - `src/isArray.rs:10`
102. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isBigInt` should have a snake case name
   - Examples:
     - `src/isBigInt.rs:10`
103. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isBoolean` should have a snake case name
   - Examples:
     - `src/isBoolean.rs:10`
104. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDate` should have a snake case name
   - Examples:
     - `src/isDate.rs:10`
105. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDeepEqual` should have a snake case name
   - Examples:
     - `src/isDeepEqual.rs:1789`
106. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isDefined` should have a snake case name
   - Examples:
     - `src/isDefined.rs:11`
107. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isEmpty` should have a snake case name
   - Examples:
     - `src/isEmpty.rs:27`
108. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isEmptyish` should have a snake case name
   - Examples:
     - `src/isEmptyish.rs:66`
109. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isError` should have a snake case name
   - Examples:
     - `src/isError.rs:11`
110. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isFunction` should have a snake case name
   - Examples:
     - `src/isFunction.rs:10`
111. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isIncludedIn` should have a snake case name
   - Examples:
     - `src/isIncludedIn.rs:28`
112. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNonNull` should have a snake case name
   - Examples:
     - `src/isNonNull.rs:11`
113. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNonNullish` should have a snake case name
   - Examples:
     - `src/isNonNullish.rs:13`
114. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNot` should have a snake case name
   - Examples:
     - `src/isNot.rs:14`
115. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNullish` should have a snake case name
   - Examples:
     - `src/isNullish.rs:13`
116. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isNumber` should have a snake case name
   - Examples:
     - `src/isNumber.rs:13`
117. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isObjectType` should have a snake case name
   - Examples:
     - `src/isObjectType.rs:13`
118. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isPlainObject` should have a snake case name
   - Examples:
     - `src/isPlainObject.rs:28`
119. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isPromise` should have a snake case name
   - Examples:
     - `src/isPromise.rs:11`
120. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isShallowEqual` should have a snake case name
   - Examples:
     - `src/isShallowEqual.rs:1813`
121. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isStrictEqual` should have a snake case name
   - Examples:
     - `src/isStrictEqual.rs:22`
122. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isString` should have a snake case name
   - Examples:
     - `src/isString.rs:10`
123. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isSymbol` should have a snake case name
   - Examples:
     - `src/isSymbol.rs:10`
124. **warning** `non_snake_case` - 1 occurrence
   - Message: function `isTruthy` should have a snake case name
   - Examples:
     - `src/isTruthy.rs:11`
125. **warning** `non_snake_case` - 1 occurrence
   - Message: function `lazyDataLastImpl` should have a snake case name
   - Examples:
     - `src/lazyDataLastImpl.rs:30`
126. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapKeys` should have a snake case name
   - Examples:
     - `src/mapKeys.rs:42`
127. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapToObj` should have a snake case name
   - Examples:
     - `src/mapToObj.rs:43`
128. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapValues` should have a snake case name
   - Examples:
     - `src/mapValues.rs:42`
129. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mapWithFeedback` should have a snake case name
   - Examples:
     - `src/mapWithFeedback.rs:12`
130. **warning** `non_snake_case` - 1 occurrence
   - Message: function `meanBy` should have a snake case name
   - Examples:
     - `src/meanBy.rs:52`
131. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeAll` should have a snake case name
   - Examples:
     - `src/mergeAll.rs:27`
132. **warning** `non_snake_case` - 1 occurrence
   - Message: function `mergeDeep` should have a snake case name
   - Examples:
     - `src/mergeDeep.rs:15`
133. **warning** `non_snake_case` - 1 occurrence
   - Message: function `nthBy` should have a snake case name
   - Examples:
     - `src/nthBy.rs:32`
134. **warning** `non_snake_case` - 1 occurrence
   - Message: function `objOf` should have a snake case name
   - Examples:
     - `src/objOf.rs:20`
135. **warning** `non_snake_case` - 1 occurrence
   - Message: function `omitBy` should have a snake case name
   - Examples:
     - `src/omitBy.rs:47`
136. **warning** `non_snake_case` - 1 occurrence
   - Message: function `partialBind` should have a snake case name
   - Examples:
     - `src/partialBind.rs:15`
137. **warning** `non_snake_case` - 1 occurrence
   - Message: function `partialLastBind` should have a snake case name
   - Examples:
     - `src/partialLastBind.rs:15`
138. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pathOr` should have a snake case name
   - Examples:
     - `src/pathOr.rs:46`
139. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pickBy` should have a snake case name
   - Examples:
     - `src/pickBy.rs:46`
140. **warning** `non_snake_case` - 1 occurrence
   - Message: function `pullObject` should have a snake case name
   - Examples:
     - `src/pullObject.rs:45`
141. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryFromLazy` should have a snake case name
   - Examples:
     - `src/purryFromLazy.rs:51`
142. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryOn` should have a snake case name
   - Examples:
     - `src/purryOn.rs:27`
143. **warning** `non_snake_case` - 1 occurrence
   - Message: function `purryOrderRules` should have a snake case name
   - Examples:
     - `src/purryOrderRules.rs:224`
144. **warning** `non_snake_case` - 1 occurrence
   - Message: function `quickSelect` should have a snake case name
   - Examples:
     - `src/quickSelect.rs:94`
145. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomBigInt` should have a snake case name
   - Examples:
     - `src/randomBigInt.rs:377`
146. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomInteger` should have a snake case name
   - Examples:
     - `src/randomInteger.rs:45`
147. **warning** `non_snake_case` - 1 occurrence
   - Message: function `randomString` should have a snake case name
   - Examples:
     - `src/randomString.rs:45`
148. **warning** `non_snake_case` - 1 occurrence
   - Message: function `rankBy` should have a snake case name
   - Examples:
     - `src/rankBy.rs:43`
149. **warning** `non_snake_case` - 1 occurrence
   - Message: function `setPath` should have a snake case name
   - Examples:
     - `src/setPath.rs:62`
150. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sliceString` should have a snake case name
   - Examples:
     - `src/sliceString.rs:19`
151. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortBy` should have a snake case name
   - Examples:
     - `src/sortBy.rs:20`
152. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndexBy` should have a snake case name
   - Examples:
     - `src/sortedIndexBy.rs:27`
153. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndexWith` should have a snake case name
   - Examples:
     - `src/sortedIndexWith.rs:15`
154. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedIndex` should have a snake case name
   - Examples:
     - `src/sortedIndex.rs:24`
155. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedLastIndexBy` should have a snake case name
   - Examples:
     - `src/sortedLastIndexBy.rs:27`
156. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sortedLastIndex` should have a snake case name
   - Examples:
     - `src/sortedLastIndex.rs:24`
157. **warning** `non_snake_case` - 1 occurrence
   - Message: function `splitAt` should have a snake case name
   - Examples:
     - `src/splitAt.rs:45`
158. **warning** `non_snake_case` - 1 occurrence
   - Message: function `splitWhen` should have a snake case name
   - Examples:
     - `src/splitWhen.rs:40`
159. **warning** `non_snake_case` - 1 occurrence
   - Message: function `startsWith` should have a snake case name
   - Examples:
     - `src/startsWith.rs:22`
160. **warning** `non_snake_case` - 1 occurrence
   - Message: function `stringToPath` should have a snake case name
   - Examples:
     - `src/stringToPath.rs:49`
161. **warning** `non_snake_case` - 1 occurrence
   - Message: function `sumBy` should have a snake case name
   - Examples:
     - `src/sumBy.rs:57`
162. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapInPlace` should have a snake case name
   - Examples:
     - `src/swapInPlace.rs:11`
163. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapIndices` should have a snake case name
   - Examples:
     - `src/swapIndices.rs:92`
164. **warning** `non_snake_case` - 1 occurrence
   - Message: function `swapProps` should have a snake case name
   - Examples:
     - `src/swapProps.rs:24`
165. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeFirstBy` should have a snake case name
   - Examples:
     - `src/takeFirstBy.rs:300`
166. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLastWhile` should have a snake case name
   - Examples:
     - `src/takeLastWhile.rs:584`
167. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeLast` should have a snake case name
   - Examples:
     - `src/takeLast.rs:37`
168. **warning** `non_snake_case` - 1 occurrence
   - Message: function `takeWhile` should have a snake case name
   - Examples:
     - `src/takeWhile.rs:47`
169. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toCamelCase` should have a snake case name
   - Examples:
     - `src/toCamelCase.rs:44`
170. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toKebabCase` should have a snake case name
   - Examples:
     - `src/toKebabCase.rs:26`
171. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toLowerCase` should have a snake case name
   - Examples:
     - `src/toLowerCase.rs:20`
172. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toSingle` should have a snake case name
   - Examples:
     - `src/toSingle.rs:11`
173. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toSnakeCase` should have a snake case name
   - Examples:
     - `src/toSnakeCase.rs:26`
174. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toTitleCase` should have a snake case name
   - Examples:
     - `src/toTitleCase.rs:44`
175. **warning** `non_snake_case` - 1 occurrence
   - Message: function `toUpperCase` should have a snake case name
   - Examples:
     - `src/toUpperCase.rs:20`
176. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueBy` should have a snake case name
   - Examples:
     - `src/uniqueBy.rs:40`
177. **warning** `non_snake_case` - 1 occurrence
   - Message: function `uniqueWith` should have a snake case name
   - Examples:
     - `src/uniqueWith.rs:12`
178. **warning** `non_snake_case` - 1 occurrence
   - Message: function `utilityEvaluators` should have a snake case name
   - Examples:
     - `src/utilityEvaluators.rs:16`
179. **warning** `non_snake_case` - 1 occurrence
   - Message: function `withPrecision` should have a snake case name
   - Examples:
     - `src/withPrecision.rs:106`
180. **warning** `non_snake_case` - 1 occurrence
   - Message: function `zipWith` should have a snake case name
   - Examples:
     - `src/zipWith.rs:54`
181. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ArrayAt` should have a snake case name
   - Examples:
     - `src/main.rs:4`
182. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ArrayRequiredPrefix` should have a snake case name
   - Examples:
     - `src/main.rs:6`
183. **warning** `non_snake_case` - 1 occurrence
   - Message: module `BoundedPartial` should have a snake case name
   - Examples:
     - `src/main.rs:8`
184. **warning** `non_snake_case` - 1 occurrence
   - Message: module `BrandedReturn` should have a snake case name
   - Examples:
     - `src/main.rs:10`
185. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ClampedIntegerSubtract` should have a snake case name
   - Examples:
     - `src/main.rs:12`
186. **warning** `non_snake_case` - 1 occurrence
   - Message: module `CoercedArray` should have a snake case name
   - Examples:
     - `src/main.rs:14`
187. **warning** `non_snake_case` - 1 occurrence
   - Message: module `CompareFunction` should have a snake case name
   - Examples:
     - `src/main.rs:16`
188. **warning** `non_snake_case` - 1 occurrence
   - Message: module `Deduped` should have a snake case name
   - Examples:
     - `src/main.rs:18`
189. **warning** `non_snake_case` - 1 occurrence
   - Message: module `DisjointUnionFields` should have a snake case name
   - Examples:
     - `src/main.rs:20`
190. **warning** `non_snake_case` - 1 occurrence
   - Message: module `EnumerableStringKeyOf` should have a snake case name
   - Examples:
     - `src/main.rs:22`
191. **warning** `non_snake_case` - 1 occurrence
   - Message: module `EnumerableStringKeyedValueOf` should have a snake case name
   - Examples:
     - `src/main.rs:24`
192. **warning** `non_snake_case` - 1 occurrence
   - Message: module `FilteredArray` should have a snake case name
   - Examples:
     - `src/main.rs:26`
193. **warning** `non_snake_case` - 1 occurrence
   - Message: module `GuardType` should have a snake case name
   - Examples:
     - `src/main.rs:28`
194. **warning** `non_snake_case` - 1 occurrence
   - Message: module `HasWritableKeys` should have a snake case name
   - Examples:
     - `src/main.rs:30`
195. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IntRangeInclusive` should have a snake case name
   - Examples:
     - `src/main.rs:32`
196. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IsBoundedRecord` should have a snake case name
   - Examples:
     - `src/main.rs:36`
197. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IsBounded` should have a snake case name
   - Examples:
     - `src/main.rs:34`
198. **warning** `non_snake_case` - 1 occurrence
   - Message: module `IterableContainer` should have a snake case name
   - Examples:
     - `src/main.rs:38`
199. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyDefinition` should have a snake case name
   - Examples:
     - `src/main.rs:40`
200. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyEvaluator` should have a snake case name
   - Examples:
     - `src/main.rs:42`
201. **warning** `non_snake_case` - 1 occurrence
   - Message: module `LazyResult` should have a snake case name
   - Examples:
     - `src/main.rs:44`
202. **warning** `non_snake_case` - 1 occurrence
   - Message: module `Mapped` should have a snake case name
   - Examples:
     - `src/main.rs:46`
203. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NTuple` should have a snake case name
   - Examples:
     - `src/main.rs:48`
204. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NarrowedTo` should have a snake case name
   - Examples:
     - `src/main.rs:50`
205. **warning** `non_snake_case` - 1 occurrence
   - Message: module `NonEmptyArray` should have a snake case name
   - Examples:
     - `src/main.rs:52`
206. **warning** `non_snake_case` - 1 occurrence
   - Message: module `OptionalOptionsWithDefaults` should have a snake case name
   - Examples:
     - `src/main.rs:54`
207. **warning** `non_snake_case` - 1 occurrence
   - Message: module `PartialArray` should have a snake case name
   - Examples:
     - `src/main.rs:56`
208. **warning** `non_snake_case` - 1 occurrence
   - Message: module `PartitionByUnion` should have a snake case name
   - Examples:
     - `src/main.rs:58`
209. **warning** `non_snake_case` - 1 occurrence
   - Message: module `RemedaTypeError` should have a snake case name
   - Examples:
     - `src/main.rs:60`
210. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ReorderedArray` should have a snake case name
   - Examples:
     - `src/main.rs:62`
211. **warning** `non_snake_case` - 1 occurrence
   - Message: module `SimplifiedWritable` should have a snake case name
   - Examples:
     - `src/main.rs:64`
212. **warning** `non_snake_case` - 1 occurrence
   - Message: module `StrictFunction` should have a snake case name
   - Examples:
     - `src/main.rs:66`
213. **warning** `non_snake_case` - 1 occurrence
   - Message: module `StringLength` should have a snake case name
   - Examples:
     - `src/main.rs:68`
214. **warning** `non_snake_case` - 1 occurrence
   - Message: module `ToString` should have a snake case name
   - Examples:
     - `src/main.rs:70`
215. **warning** `non_snake_case` - 1 occurrence
   - Message: module `TupleParts` should have a snake case name
   - Examples:
     - `src/main.rs:72`
216. **warning** `non_snake_case` - 1 occurrence
   - Message: module `TupleSplits` should have a snake case name
   - Examples:
     - `src/main.rs:74`
217. **warning** `non_snake_case` - 1 occurrence
   - Message: module `UpsertProp` should have a snake case name
   - Examples:
     - `src/main.rs:76`
218. **warning** `non_snake_case` - 1 occurrence
   - Message: module `addProp` should have a snake case name
   - Examples:
     - `src/main.rs:80`
219. **warning** `non_snake_case` - 1 occurrence
   - Message: module `allPass` should have a snake case name
   - Examples:
     - `src/main.rs:82`
220. **warning** `non_snake_case` - 1 occurrence
   - Message: module `anyPass` should have a snake case name
   - Examples:
     - `src/main.rs:84`
221. **warning** `non_snake_case` - 1 occurrence
   - Message: module `binarySearchCutoffIndex` should have a snake case name
   - Examples:
     - `src/main.rs:86`
222. **warning** `non_snake_case` - 1 occurrence
   - Message: module `countBy` should have a snake case name
   - Examples:
     - `src/main.rs:104`
223. **warning** `non_snake_case` - 1 occurrence
   - Message: module `defaultTo` should have a snake case name
   - Examples:
     - `src/main.rs:108`
224. **warning** `non_snake_case` - 1 occurrence
   - Message: module `differenceWith` should have a snake case name
   - Examples:
     - `src/main.rs:112`
225. **warning** `non_snake_case` - 1 occurrence
   - Message: module `doNothing` should have a snake case name
   - Examples:
     - `src/main.rs:116`
226. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropFirstBy` should have a snake case name
   - Examples:
     - `src/main.rs:120`
227. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLastWhile` should have a snake case name
   - Examples:
     - `src/main.rs:124`
228. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropLast` should have a snake case name
   - Examples:
     - `src/main.rs:122`
229. **warning** `non_snake_case` - 1 occurrence
   - Message: module `dropWhile` should have a snake case name
   - Examples:
     - `src/main.rs:126`
230. **warning** `non_snake_case` - 1 occurrence
   - Message: module `endsWith` should have a snake case name
   - Examples:
     - `src/main.rs:128`
231. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findIndex` should have a snake case name
   - Examples:
     - `src/main.rs:138`
232. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLastIndex` should have a snake case name
   - Examples:
     - `src/main.rs:142`
233. **warning** `non_snake_case` - 1 occurrence
   - Message: module `findLast` should have a snake case name
   - Examples:
     - `src/main.rs:140`
234. **warning** `non_snake_case` - 1 occurrence
   - Message: module `firstBy` should have a snake case name
   - Examples:
     - `src/main.rs:146`
235. **warning** `non_snake_case` - 1 occurrence
   - Message: module `flatMap` should have a snake case name
   - Examples:
     - `src/main.rs:150`
236. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEachObj` should have a snake case name
   - Examples:
     - `src/main.rs:156`
237. **warning** `non_snake_case` - 1 occurrence
   - Message: module `forEach` should have a snake case name
   - Examples:
     - `src/main.rs:154`
238. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromEntries` should have a snake case name
   - Examples:
     - `src/main.rs:158`
239. **warning** `non_snake_case` - 1 occurrence
   - Message: module `fromKeys` should have a snake case name
   - Examples:
     - `src/main.rs:160`
240. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupByProp` should have a snake case name
   - Examples:
     - `src/main.rs:166`
241. **warning** `non_snake_case` - 1 occurrence
   - Message: module `groupBy` should have a snake case name
   - Examples:
     - `src/main.rs:164`
242. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasAtLeast` should have a snake case name
   - Examples:
     - `src/main.rs:168`
243. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasProp` should have a snake case name
   - Examples:
     - `src/main.rs:170`
244. **warning** `non_snake_case` - 1 occurrence
   - Message: module `hasSubObject` should have a snake case name
   - Examples:
     - `src/main.rs:172`
245. **warning** `non_snake_case` - 1 occurrence
   - Message: module `indexBy` should have a snake case name
   - Examples:
     - `src/main.rs:178`
246. **warning** `non_snake_case` - 1 occurrence
   - Message: module `intersectionWith` should have a snake case name
   - Examples:
     - `src/main.rs:182`
247. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isArray` should have a snake case name
   - Examples:
     - `src/main.rs:186`
248. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBigInt` should have a snake case name
   - Examples:
     - `src/main.rs:188`
249. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isBoolean` should have a snake case name
   - Examples:
     - `src/main.rs:190`
250. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDate` should have a snake case name
   - Examples:
     - `src/main.rs:192`
251. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDeepEqual` should have a snake case name
   - Examples:
     - `src/main.rs:194`
252. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isDefined` should have a snake case name
   - Examples:
     - `src/main.rs:196`
253. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmpty` should have a snake case name
   - Examples:
     - `src/main.rs:198`
254. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isEmptyish` should have a snake case name
   - Examples:
     - `src/main.rs:200`
255. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isError` should have a snake case name
   - Examples:
     - `src/main.rs:202`
256. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isFunction` should have a snake case name
   - Examples:
     - `src/main.rs:204`
257. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isIncludedIn` should have a snake case name
   - Examples:
     - `src/main.rs:206`
258. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNull` should have a snake case name
   - Examples:
     - `src/main.rs:208`
259. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNonNullish` should have a snake case name
   - Examples:
     - `src/main.rs:210`
260. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNot` should have a snake case name
   - Examples:
     - `src/main.rs:212`
261. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNullish` should have a snake case name
   - Examples:
     - `src/main.rs:214`
262. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isNumber` should have a snake case name
   - Examples:
     - `src/main.rs:216`
263. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isObjectType` should have a snake case name
   - Examples:
     - `src/main.rs:218`
264. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPlainObject` should have a snake case name
   - Examples:
     - `src/main.rs:220`
265. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isPromise` should have a snake case name
   - Examples:
     - `src/main.rs:222`
266. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isShallowEqual` should have a snake case name
   - Examples:
     - `src/main.rs:224`
267. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isStrictEqual` should have a snake case name
   - Examples:
     - `src/main.rs:226`
268. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isString` should have a snake case name
   - Examples:
     - `src/main.rs:228`
269. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isSymbol` should have a snake case name
   - Examples:
     - `src/main.rs:230`
270. **warning** `non_snake_case` - 1 occurrence
   - Message: module `isTruthy` should have a snake case name
   - Examples:
     - `src/main.rs:232`
271. **warning** `non_snake_case` - 1 occurrence
   - Message: module `lazyDataLastImpl` should have a snake case name
   - Examples:
     - `src/main.rs:240`
272. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapKeys` should have a snake case name
   - Examples:
     - `src/main.rs:246`
273. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapToObj` should have a snake case name
   - Examples:
     - `src/main.rs:248`
274. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapValues` should have a snake case name
   - Examples:
     - `src/main.rs:250`
275. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mapWithFeedback` should have a snake case name
   - Examples:
     - `src/main.rs:252`
276. **warning** `non_snake_case` - 1 occurrence
   - Message: module `meanBy` should have a snake case name
   - Examples:
     - `src/main.rs:256`
277. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeAll` should have a snake case name
   - Examples:
     - `src/main.rs:262`
278. **warning** `non_snake_case` - 1 occurrence
   - Message: module `mergeDeep` should have a snake case name
   - Examples:
     - `src/main.rs:264`
279. **warning** `non_snake_case` - 1 occurrence
   - Message: module `nthBy` should have a snake case name
   - Examples:
     - `src/main.rs:268`
280. **warning** `non_snake_case` - 1 occurrence
   - Message: module `objOf` should have a snake case name
   - Examples:
     - `src/main.rs:270`
281. **warning** `non_snake_case` - 1 occurrence
   - Message: module `omitBy` should have a snake case name
   - Examples:
     - `src/main.rs:274`
282. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialBind` should have a snake case name
   - Examples:
     - `src/main.rs:280`
283. **warning** `non_snake_case` - 1 occurrence
   - Message: module `partialLastBind` should have a snake case name
   - Examples:
     - `src/main.rs:282`
284. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pathOr` should have a snake case name
   - Examples:
     - `src/main.rs:286`
285. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pickBy` should have a snake case name
   - Examples:
     - `src/main.rs:290`
286. **warning** `non_snake_case` - 1 occurrence
   - Message: module `pullObject` should have a snake case name
   - Examples:
     - `src/main.rs:300`
287. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryFromLazy` should have a snake case name
   - Examples:
     - `src/main.rs:304`
288. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryOn` should have a snake case name
   - Examples:
     - `src/main.rs:306`
289. **warning** `non_snake_case` - 1 occurrence
   - Message: module `purryOrderRules` should have a snake case name
   - Examples:
     - `src/main.rs:308`
290. **warning** `non_snake_case` - 1 occurrence
   - Message: module `quickSelect` should have a snake case name
   - Examples:
     - `src/main.rs:310`
291. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomBigInt` should have a snake case name
   - Examples:
     - `src/main.rs:312`
292. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomInteger` should have a snake case name
   - Examples:
     - `src/main.rs:314`
293. **warning** `non_snake_case` - 1 occurrence
   - Message: module `randomString` should have a snake case name
   - Examples:
     - `src/main.rs:316`
294. **warning** `non_snake_case` - 1 occurrence
   - Message: module `rankBy` should have a snake case name
   - Examples:
     - `src/main.rs:320`
295. **warning** `non_snake_case` - 1 occurrence
   - Message: module `setPath` should have a snake case name
   - Examples:
     - `src/main.rs:332`
296. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sliceString` should have a snake case name
   - Examples:
     - `src/main.rs:336`
297. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortBy` should have a snake case name
   - Examples:
     - `src/main.rs:340`
298. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexBy` should have a snake case name
   - Examples:
     - `src/main.rs:344`
299. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndexWith` should have a snake case name
   - Examples:
     - `src/main.rs:346`
300. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedIndex` should have a snake case name
   - Examples:
     - `src/main.rs:342`
301. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndexBy` should have a snake case name
   - Examples:
     - `src/main.rs:350`
302. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sortedLastIndex` should have a snake case name
   - Examples:
     - `src/main.rs:348`
303. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitAt` should have a snake case name
   - Examples:
     - `src/main.rs:356`
304. **warning** `non_snake_case` - 1 occurrence
   - Message: module `splitWhen` should have a snake case name
   - Examples:
     - `src/main.rs:358`
305. **warning** `non_snake_case` - 1 occurrence
   - Message: module `startsWith` should have a snake case name
   - Examples:
     - `src/main.rs:362`
306. **warning** `non_snake_case` - 1 occurrence
   - Message: module `stringToPath` should have a snake case name
   - Examples:
     - `src/main.rs:364`
307. **warning** `non_snake_case` - 1 occurrence
   - Message: module `sumBy` should have a snake case name
   - Examples:
     - `src/main.rs:370`
308. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapInPlace` should have a snake case name
   - Examples:
     - `src/main.rs:372`
309. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapIndices` should have a snake case name
   - Examples:
     - `src/main.rs:374`
310. **warning** `non_snake_case` - 1 occurrence
   - Message: module `swapProps` should have a snake case name
   - Examples:
     - `src/main.rs:376`
311. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeFirstBy` should have a snake case name
   - Examples:
     - `src/main.rs:380`
312. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLastWhile` should have a snake case name
   - Examples:
     - `src/main.rs:384`
313. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeLast` should have a snake case name
   - Examples:
     - `src/main.rs:382`
314. **warning** `non_snake_case` - 1 occurrence
   - Message: module `takeWhile` should have a snake case name
   - Examples:
     - `src/main.rs:386`
315. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toCamelCase` should have a snake case name
   - Examples:
     - `src/main.rs:392`
316. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toKebabCase` should have a snake case name
   - Examples:
     - `src/main.rs:394`
317. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toLowerCase` should have a snake case name
   - Examples:
     - `src/main.rs:396`
318. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toSingle` should have a snake case name
   - Examples:
     - `src/main.rs:398`
319. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toSnakeCase` should have a snake case name
   - Examples:
     - `src/main.rs:400`
320. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toTitleCase` should have a snake case name
   - Examples:
     - `src/main.rs:402`
321. **warning** `non_snake_case` - 1 occurrence
   - Message: module `toUpperCase` should have a snake case name
   - Examples:
     - `src/main.rs:404`
322. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueBy` should have a snake case name
   - Examples:
     - `src/main.rs:412`
323. **warning** `non_snake_case` - 1 occurrence
   - Message: module `uniqueWith` should have a snake case name
   - Examples:
     - `src/main.rs:414`
324. **warning** `non_snake_case` - 1 occurrence
   - Message: module `utilityEvaluators` should have a snake case name
   - Examples:
     - `src/main.rs:416`
325. **warning** `non_snake_case` - 1 occurrence
   - Message: module `withPrecision` should have a snake case name
   - Examples:
     - `src/main.rs:422`
326. **warning** `non_snake_case` - 1 occurrence
   - Message: module `zipWith` should have a snake case name
   - Examples:
     - `src/main.rs:428`
327. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `accumulator` is never read
   - Examples:
     - `src/main.rs:751`
328. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `actual_sample_size` is never read
   - Examples:
     - `src/sample.rs:16`
329. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
330. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
331. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `args` is never read
   - Examples:
     - `src/debounce.rs:59`
332. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `as_set` is never read
   - Examples:
     - `src/isIncludedIn.rs:7`
333. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `byte` is never read
   - Examples:
     - `src/randomBigInt.rs:334`
334. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `category` is never read
   - Examples:
     - `src/countBy.rs:19`
335. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `character` is never read
   - Examples:
     - `src/words.rs:7`
336. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `chunks` is never read
   - Examples:
     - `src/main.rs:572`
337. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `comparator` is never read
   - Examples:
     - `src/purryOrderRules.rs:76`
338. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn_1` is never read
   - Examples:
     - `src/purryOrderRules.rs:8`
339. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `compare_fn` is never read
   - Examples:
     - `src/purryOrderRules.rs:7`
340. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `copy` is never read
   - Examples:
     - `src/setPath.rs:16`
341. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `count` is never read
   - Examples:
     - `src/countBy.rs:20`
342. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_first` is never read
   - Examples:
     - `src/firstBy.rs:16`
343. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_item` is never read
   - Examples:
     - `src/pipe.rs:7`
344. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_value` is never read
   - Examples:
     - `src/setPath.rs:17`
345. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:19`
346. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_item` is never read
   - Examples:
     - `src/main.rs:878`
347. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data_last` is never read
   - Examples:
     - `src/purryFromLazy.rs:11`
348. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `data` is never read
   - Examples:
     - `src/purryFromLazy.rs:7`
349. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `destination_value` is never read
   - Examples:
     - `src/main.rs:958`
350. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:75`
351. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `double_quoted` is never read
   - Examples:
     - `src/stringToPath.rs:38`
352. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `effective_index` is never read
   - Examples:
     - `src/splitAt.rs:16`
353. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `element` is never read
   - Examples:
     - `src/mapToObj.rs:18`
354. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/range.rs:17`
355. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `excess_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:10`
356. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `first_child_index` is never read
   - Examples:
     - `src/heap.rs:298`
357. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `first_value` is never read
   - Examples:
     - `src/sumBy.rs:16`
358. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/main.rs:672`
359. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `function_index` is never read
   - Examples:
     - `src/main.rs:726`
360. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:58`
361. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_cool_down_end` is never read
   - Examples:
     - `src/debounce.rs:8`
362. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handle_invoke` is never read
   - Examples:
     - `src/debounce.rs:7`
363. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `head` is never read
   - Examples:
     - `src/heap.rs:274`
364. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/times.rs:18`
365. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `idx` is never read
   - Examples:
     - `src/clone.rs:17`
366. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_done` is never read
   - Examples:
     - `src/pipe.rs:9`
367. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_found` is never read
   - Examples:
     - `src/main.rs:879`
368. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `is_single` is never read
   - Examples:
     - `src/main.rs:761`
369. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `j` is never read
   - Examples:
     - `src/quickSelect.rs:69`
370. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `k` is never read
   - Examples:
     - `src/clone.rs:80`
371. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `keys` is never read
   - Examples:
     - `src/isShallowEqual.rs:16`
372. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `last_character` is never read
   - Examples:
     - `src/words.rs:8`
373. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `latest_call_args` is never read
   - Examples:
     - `src/debounce.rs:12`
374. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition_1` is never read
   - Examples:
     - `src/purryFromLazy.rs:10`
375. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
376. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_fn` is never read
   - Examples:
     - `src/pipe.rs:12`
377. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_function` is never read
   - Examples:
     - `src/main.rs:671`
378. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_op` is never read
   - Examples:
     - `src/main.rs:675`
379. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_result` is never read
   - Examples:
     - `src/pipe.rs:8`
380. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `left` is never read
   - Examples:
     - `src/drop.rs:17`
381. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_key` is never read
   - Examples:
     - `src/mapKeys.rs:19`
382. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `mapped_value` is never read
   - Examples:
     - `src/mapValues.rs:19`
383. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bits` is never read
   - Examples:
     - `src/randomBigInt.rs:8`
384. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_bytes` is never read
   - Examples:
     - `src/randomBigInt.rs:9`
385. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `max_wait_timeout_id` is never read
   - Examples:
     - `src/debounce.rs:11`
386. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_direction` is never read
   - Examples:
     - `src/purryOrderRules.rs:173`
387. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `maybe_projection` is never read
   - Examples:
     - `src/purryOrderRules.rs:172`
388. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `middle_index` is never read
   - Examples:
     - `src/median.rs:17`
389. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `next_comparer` is never read
   - Examples:
     - `src/purryOrderRules.rs:77`
390. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `now` is never read
   - Examples:
     - `src/funnel.rs:104`
391. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `other_copy` is never read
   - Examples:
     - `src/main.rs:847`
392. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:481`
393. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `pivot` is never read
   - Examples:
     - `src/binarySearchCutoffIndex.rs:8`
394. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_a` is never read
   - Examples:
     - `src/swapIndices.rs:16`
395. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `positive_index_b` is never read
   - Examples:
     - `src/swapIndices.rs:17`
396. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `previous_head` is never read
   - Examples:
     - `src/dropFirstBy.rs:20`
397. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `projector` is never read
   - Examples:
     - `src/purryOrderRules.rs:74`
398. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prop_name` is never read
   - Examples:
     - `src/stringToPath.rs:7`
399. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prop` is never read
   - Examples:
     - `src/pathOr.rs:16`
400. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `proto` is never read
   - Examples:
     - `src/isPlainObject.rs:7`
401. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `prototype` is never read
   - Examples:
     - `src/clone.rs:16`
402. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `quoted` is never read
   - Examples:
     - `src/stringToPath.rs:37`
403. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `rand` is never read
   - Examples:
     - `src/shuffle.rs:16`
404. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_char` is never read
   - Examples:
     - `src/randomString.rs:16`
405. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `random_index` is never read
   - Examples:
     - `src/sample.rs:62`
406. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `range` is never read
   - Examples:
     - `src/randomBigInt.rs:7`
407. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `raw` is never read
   - Examples:
     - `src/randomBigInt.rs:12`
408. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `res` is never read
   - Examples:
     - `src/times.rs:17`
409. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sample_indices` is never read
   - Examples:
     - `src/sample.rs:17`
410. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `second_child_index` is never read
   - Examples:
     - `src/heap.rs:300`
411. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_exponent` is never read
   - Examples:
     - `src/withPrecision.rs:7`
412. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value_as_string` is never read
   - Examples:
     - `src/withPrecision.rs:8`
413. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `shifted_value` is never read
   - Examples:
     - `src/withPrecision.rs:45`
414. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sorted_data` is never read
   - Examples:
     - `src/median.rs:16`
415. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `source_value` is never read
   - Examples:
     - `src/main.rs:959`
416. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/main.rs:578`
417. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `step` is never read
   - Examples:
     - `src/range.rs:16`
418. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `summand` is never read
   - Examples:
     - `src/sumBy.rs:21`
419. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `swap_index` is never read
   - Examples:
     - `src/heap.rs:299`
420. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `unquoted` is never read
   - Examples:
     - `src/stringToPath.rs:10`
421. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `v` is never read
   - Examples:
     - `src/clone.rs:81`
422. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_a` is never read
   - Examples:
     - `src/isShallowEqual.rs:18`
423. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:82`
424. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
425. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
426. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
427. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
428. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
429. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
430. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
431. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
432. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
433. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
434. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
435. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
436. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
437. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
438. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
439. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
440. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
441. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
442. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
443. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
444. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
445. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
446. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
447. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
448. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
449. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
450. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
451. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
452. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
453. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
454. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
455. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
456. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
457. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
458. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
459. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
460. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
461. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add::*`
   - Examples:
     - `src/main.rs:79`
462. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
463. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:83`
464. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:85`
465. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:89`
466. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:91`
467. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:93`
468. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:95`
469. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:97`
470. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:99`
471. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:101`
472. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:103`
473. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:105`
474. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:107`
475. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:109`
476. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:111`
477. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:113`
478. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:115`
479. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:117`
480. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:119`
481. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:121`
482. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:123`
483. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:125`
484. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:127`
485. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:129`
486. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:131`
487. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:133`
488. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:135`
489. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:137`
490. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:139`
491. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:141`
492. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:143`
493. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:145`
494. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:147`
495. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:149`
496. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:151`
497. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:153`
498. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:155`
499. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:157`
500. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:159`
501. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:161`
502. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel::*`
   - Examples:
     - `src/main.rs:163`
503. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:165`
504. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:167`
505. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:171`
506. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:173`
507. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:177`
508. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:179`
509. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:181`
510. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:183`
511. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:185`
512. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:187`
513. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:189`
514. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:191`
515. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:193`
516. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:197`
517. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:199`
518. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:201`
519. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:203`
520. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:205`
521. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:207`
522. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:209`
523. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:211`
524. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:213`
525. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:215`
526. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:217`
527. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:219`
528. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:223`
529. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:225`
530. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:227`
531. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:229`
532. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:231`
533. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:233`
534. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:235`
535. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:237`
536. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:239`
537. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:243`
538. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map::*`
   - Examples:
     - `src/main.rs:245`
539. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:247`
540. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:249`
541. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:251`
542. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:253`
543. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:255`
544. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:257`
545. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:259`
546. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:261`
547. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:263`
548. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:265`
549. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:267`
550. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:269`
551. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:271`
552. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:273`
553. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:275`
554. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:277`
555. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:279`
556. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:281`
557. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:283`
558. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:285`
559. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:287`
560. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:289`
561. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:291`
562. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:295`
563. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:297`
564. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:299`
565. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:301`
566. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt::*`
   - Examples:
     - `src/main.rs:313`
567. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger::*`
   - Examples:
     - `src/main.rs:315`
568. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:317`
569. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:319`
570. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:321`
571. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce::*`
   - Examples:
     - `src/main.rs:323`
572. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:325`
573. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:327`
574. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:329`
575. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:331`
576. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:333`
577. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:335`
578. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:337`
579. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:339`
580. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:341`
581. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:343`
582. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy::*`
   - Examples:
     - `src/main.rs:345`
583. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:347`
584. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:349`
585. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy::*`
   - Examples:
     - `src/main.rs:351`
586. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:353`
587. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:355`
588. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:357`
589. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:359`
590. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:361`
591. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:363`
592. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:365`
593. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:367`
594. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:371`
595. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:375`
596. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:377`
597. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:379`
598. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:381`
599. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:383`
600. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:385`
601. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:387`
602. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:389`
603. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times::*`
   - Examples:
     - `src/main.rs:391`
604. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:393`
605. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:395`
606. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:397`
607. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:401`
608. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:403`
609. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:405`
610. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:407`
611. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:409`
612. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique::*`
   - Examples:
     - `src/main.rs:411`
613. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:413`
614. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:415`
615. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:419`
616. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:421`
617. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:427`
618. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:429`
619. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:351`

## Cargo Stderr

```text
Blocking waiting for file lock on package cache
    Updating crates.io index
     Locking 51 packages to latest Rust 1.93.0 compatible versions
      Adding rand v0.9.4 (available: v0.10.1)
    Blocking waiting for file lock on package cache
   Compiling libc v0.2.186
   Compiling zerocopy v0.8.48
   Compiling getrandom v0.3.4
    Checking cfg-if v1.0.4
   Compiling autocfg v1.5.0
    Checking memchr v2.8.0
    Checking regex-syntax v0.8.10
    Checking iana-time-zone v0.1.65
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
    Checking chrono v0.4.44
    Checking regex-automata v0.4.14
    Checking rand_core v0.9.5
    Checking regex v1.12.3
    Checking ppv-lite86 v0.2.21
    Checking rand_chacha v0.9.0
    Checking rand v0.9.4
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.32s
```
