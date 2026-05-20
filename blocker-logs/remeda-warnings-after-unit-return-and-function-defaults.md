# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `2`
- Warnings: `750`

## Summary By Code

1. **warning** `unused_imports` - 362 diagnostics
2. **warning** `unused_mut` - 303 diagnostics
3. **warning** `unused_assignments` - 47 diagnostics
4. **warning** `unused_parens` - 36 diagnostics
5. **error** `E0381` - 2 diagnostics
6. **warning** `unreachable_code` - 2 diagnostics

## Groups

1. **warning** `unused_mut` - 303 occurrences
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
   - Message: value assigned to `cutoff` is never read
   - Examples:
     - `src/truncate.rs:160`
     - `src/truncate.rs:145`
     - `src/truncate.rs:119`
     - `src/truncate.rs:104`
8. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/dropLastWhile.rs:56`
     - `src/findLast.rs:33`
     - `src/findLastIndex.rs:33`
     - `src/takeLastWhile.rs:54`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
10. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
     - `src/withPrecision.rs:27`
11. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
12. **warning** `unreachable_code` - 2 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/isDeepEqual.rs:305`
     - `src/sample.rs:85`
13. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:81`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:67`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:83`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:69`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:130`
     - `src/truncate.rs:89`
16. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/fromKeys.rs:36`
     - `src/omit.rs:129`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `last_separator_1` is never read
   - Examples:
     - `src/truncate.rs:151`
     - `src/truncate.rs:110`
18. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:82`
     - `src/funnel_lodash_throttle_test.rs:68`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id_1` is never read
   - Examples:
     - `src/debounce.rs:235`
     - `src/debounce.rs:223`
20. **error** `E0381` - 1 occurrence
   - Message: used binding `_smelt_tmp_22` isn't initialized
   - Examples:
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:75`
21. **error** `E0381` - 1 occurrence
   - Message: used binding `_smelt_tmp_27` isn't initialized
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:89`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:51`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:50`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `current_index` is never read
   - Examples:
     - `src/heap.rs:95`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `functions_index` is never read
   - Examples:
     - `src/pipe.rs:302`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:27`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
30. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `output` is never read
   - Examples:
     - `src/prop.rs:47`
31. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:529`
32. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
33. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayAt::*`
   - Examples:
     - `src/main.rs:5`
34. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ArrayRequiredPrefix::*`
   - Examples:
     - `src/main.rs:7`
35. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BoundedPartial::*`
   - Examples:
     - `src/main.rs:9`
36. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `BrandedReturn::*`
   - Examples:
     - `src/main.rs:11`
37. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ClampedIntegerSubtract::*`
   - Examples:
     - `src/main.rs:13`
38. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CoercedArray::*`
   - Examples:
     - `src/main.rs:15`
39. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `CompareFunction::*`
   - Examples:
     - `src/main.rs:17`
40. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Deduped::*`
   - Examples:
     - `src/main.rs:19`
41. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `DisjointUnionFields::*`
   - Examples:
     - `src/main.rs:21`
42. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyOf::*`
   - Examples:
     - `src/main.rs:23`
43. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `EnumerableStringKeyedValueOf::*`
   - Examples:
     - `src/main.rs:25`
44. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `FilteredArray::*`
   - Examples:
     - `src/main.rs:27`
45. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `GuardType::*`
   - Examples:
     - `src/main.rs:29`
46. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `HasWritableKeys::*`
   - Examples:
     - `src/main.rs:31`
47. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IntRangeInclusive::*`
   - Examples:
     - `src/main.rs:33`
48. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBounded::*`
   - Examples:
     - `src/main.rs:35`
49. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IsBoundedRecord::*`
   - Examples:
     - `src/main.rs:37`
50. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `IterableContainer::*`
   - Examples:
     - `src/main.rs:39`
51. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyDefinition::*`
   - Examples:
     - `src/main.rs:41`
52. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyEvaluator::*`
   - Examples:
     - `src/main.rs:43`
53. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `LazyResult::*`
   - Examples:
     - `src/main.rs:45`
54. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `Mapped::*`
   - Examples:
     - `src/main.rs:47`
55. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NTuple::*`
   - Examples:
     - `src/main.rs:49`
56. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NarrowedTo::*`
   - Examples:
     - `src/main.rs:51`
57. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `NonEmptyArray::*`
   - Examples:
     - `src/main.rs:53`
58. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `OptionalOptionsWithDefaults::*`
   - Examples:
     - `src/main.rs:55`
59. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartialArray::*`
   - Examples:
     - `src/main.rs:57`
60. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `PartitionByUnion::*`
   - Examples:
     - `src/main.rs:59`
61. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `RemedaTypeError::*`
   - Examples:
     - `src/main.rs:61`
62. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ReorderedArray::*`
   - Examples:
     - `src/main.rs:63`
63. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `SimplifiedWritable::*`
   - Examples:
     - `src/main.rs:65`
64. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StrictFunction::*`
   - Examples:
     - `src/main.rs:67`
65. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `StringLength::*`
   - Examples:
     - `src/main.rs:69`
66. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ToString::*`
   - Examples:
     - `src/main.rs:71`
67. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleParts::*`
   - Examples:
     - `src/main.rs:73`
68. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `TupleSplits::*`
   - Examples:
     - `src/main.rs:75`
69. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `UpsertProp::*`
   - Examples:
     - `src/main.rs:77`
70. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp::*`
   - Examples:
     - `src/main.rs:81`
71. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `addProp_test::*`
   - Examples:
     - `src/main.rs:83`
72. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `add_test::*`
   - Examples:
     - `src/main.rs:85`
73. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass::*`
   - Examples:
     - `src/main.rs:87`
74. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `allPass_test::*`
   - Examples:
     - `src/main.rs:89`
75. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass::*`
   - Examples:
     - `src/main.rs:91`
76. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `anyPass_test::*`
   - Examples:
     - `src/main.rs:93`
77. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `binarySearchCutoffIndex_test::*`
   - Examples:
     - `src/main.rs:97`
78. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize::*`
   - Examples:
     - `src/main.rs:99`
79. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `capitalize_test::*`
   - Examples:
     - `src/main.rs:101`
80. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil::*`
   - Examples:
     - `src/main.rs:103`
81. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `ceil_test::*`
   - Examples:
     - `src/main.rs:105`
82. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk::*`
   - Examples:
     - `src/main.rs:107`
83. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `chunk_test::*`
   - Examples:
     - `src/main.rs:109`
84. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp::*`
   - Examples:
     - `src/main.rs:111`
85. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clamp_test::*`
   - Examples:
     - `src/main.rs:113`
86. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone::*`
   - Examples:
     - `src/main.rs:115`
87. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `clone_test::*`
   - Examples:
     - `src/main.rs:117`
88. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat::*`
   - Examples:
     - `src/main.rs:119`
89. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `concat_test::*`
   - Examples:
     - `src/main.rs:121`
90. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional::*`
   - Examples:
     - `src/main.rs:123`
91. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `conditional_test::*`
   - Examples:
     - `src/main.rs:125`
92. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant::*`
   - Examples:
     - `src/main.rs:127`
93. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `constant_test::*`
   - Examples:
     - `src/main.rs:129`
94. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy::*`
   - Examples:
     - `src/main.rs:131`
95. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `countBy_test::*`
   - Examples:
     - `src/main.rs:133`
96. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce::*`
   - Examples:
     - `src/main.rs:135`
97. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `debounce_test::*`
   - Examples:
     - `src/main.rs:137`
98. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo::*`
   - Examples:
     - `src/main.rs:139`
99. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `defaultTo_test::*`
   - Examples:
     - `src/main.rs:141`
100. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference::*`
   - Examples:
     - `src/main.rs:143`
101. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith::*`
   - Examples:
     - `src/main.rs:145`
102. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `differenceWith_test::*`
   - Examples:
     - `src/main.rs:147`
103. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `difference_test::*`
   - Examples:
     - `src/main.rs:149`
104. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide::*`
   - Examples:
     - `src/main.rs:151`
105. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `divide_test::*`
   - Examples:
     - `src/main.rs:153`
106. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing::*`
   - Examples:
     - `src/main.rs:155`
107. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `doNothing_test::*`
   - Examples:
     - `src/main.rs:157`
108. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop::*`
   - Examples:
     - `src/main.rs:159`
109. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy::*`
   - Examples:
     - `src/main.rs:161`
110. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropFirstBy_test::*`
   - Examples:
     - `src/main.rs:163`
111. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast::*`
   - Examples:
     - `src/main.rs:165`
112. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile::*`
   - Examples:
     - `src/main.rs:167`
113. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLastWhile_test::*`
   - Examples:
     - `src/main.rs:169`
114. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropLast_test::*`
   - Examples:
     - `src/main.rs:171`
115. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile::*`
   - Examples:
     - `src/main.rs:173`
116. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `dropWhile_test::*`
   - Examples:
     - `src/main.rs:175`
117. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `drop_test::*`
   - Examples:
     - `src/main.rs:177`
118. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith::*`
   - Examples:
     - `src/main.rs:179`
119. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `endsWith_test::*`
   - Examples:
     - `src/main.rs:181`
120. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries::*`
   - Examples:
     - `src/main.rs:183`
121. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `entries_test::*`
   - Examples:
     - `src/main.rs:185`
122. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve::*`
   - Examples:
     - `src/main.rs:187`
123. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `evolve_test::*`
   - Examples:
     - `src/main.rs:189`
124. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter::*`
   - Examples:
     - `src/main.rs:191`
125. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `filter_test::*`
   - Examples:
     - `src/main.rs:193`
126. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find::*`
   - Examples:
     - `src/main.rs:195`
127. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex::*`
   - Examples:
     - `src/main.rs:197`
128. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findIndex_test::*`
   - Examples:
     - `src/main.rs:199`
129. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast::*`
   - Examples:
     - `src/main.rs:201`
130. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex::*`
   - Examples:
     - `src/main.rs:203`
131. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLastIndex_test::*`
   - Examples:
     - `src/main.rs:205`
132. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `findLast_test::*`
   - Examples:
     - `src/main.rs:207`
133. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `find_test::*`
   - Examples:
     - `src/main.rs:209`
134. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first::*`
   - Examples:
     - `src/main.rs:211`
135. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy::*`
   - Examples:
     - `src/main.rs:213`
136. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `firstBy_test::*`
   - Examples:
     - `src/main.rs:215`
137. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `first_test::*`
   - Examples:
     - `src/main.rs:217`
138. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat::*`
   - Examples:
     - `src/main.rs:219`
139. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap::*`
   - Examples:
     - `src/main.rs:221`
140. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flatMap_test::*`
   - Examples:
     - `src/main.rs:223`
141. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `flat_test::*`
   - Examples:
     - `src/main.rs:225`
142. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor::*`
   - Examples:
     - `src/main.rs:227`
143. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `floor_test::*`
   - Examples:
     - `src/main.rs:229`
144. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach::*`
   - Examples:
     - `src/main.rs:231`
145. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj::*`
   - Examples:
     - `src/main.rs:233`
146. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEachObj_test::*`
   - Examples:
     - `src/main.rs:235`
147. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `forEach_test::*`
   - Examples:
     - `src/main.rs:237`
148. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries::*`
   - Examples:
     - `src/main.rs:239`
149. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromEntries_test::*`
   - Examples:
     - `src/main.rs:241`
150. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys::*`
   - Examples:
     - `src/main.rs:243`
151. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `fromKeys_test::*`
   - Examples:
     - `src/main.rs:245`
152. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_test::*`
   - Examples:
     - `src/main.rs:249`
153. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_debounce_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:251`
154. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_test::*`
   - Examples:
     - `src/main.rs:253`
155. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_lodash_throttle_with_cached_value_test::*`
   - Examples:
     - `src/main.rs:255`
156. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_reference_batch_test::*`
   - Examples:
     - `src/main.rs:257`
157. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_remeda_debounce_test::*`
   - Examples:
     - `src/main.rs:259`
158. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `funnel_test::*`
   - Examples:
     - `src/main.rs:261`
159. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy::*`
   - Examples:
     - `src/main.rs:263`
160. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp::*`
   - Examples:
     - `src/main.rs:265`
161. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupByProp_test::*`
   - Examples:
     - `src/main.rs:267`
162. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `groupBy_test::*`
   - Examples:
     - `src/main.rs:269`
163. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasAtLeast_test::*`
   - Examples:
     - `src/main.rs:273`
164. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp::*`
   - Examples:
     - `src/main.rs:275`
165. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasProp_test::*`
   - Examples:
     - `src/main.rs:277`
166. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject::*`
   - Examples:
     - `src/main.rs:279`
167. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `hasSubObject_test::*`
   - Examples:
     - `src/main.rs:281`
168. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `heap_test::*`
   - Examples:
     - `src/main.rs:285`
169. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity::*`
   - Examples:
     - `src/main.rs:287`
170. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `identity_test::*`
   - Examples:
     - `src/main.rs:289`
171. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy::*`
   - Examples:
     - `src/main.rs:291`
172. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `indexBy_test::*`
   - Examples:
     - `src/main.rs:293`
173. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection::*`
   - Examples:
     - `src/main.rs:295`
174. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith::*`
   - Examples:
     - `src/main.rs:297`
175. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersectionWith_test::*`
   - Examples:
     - `src/main.rs:299`
176. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `intersection_test::*`
   - Examples:
     - `src/main.rs:301`
177. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert::*`
   - Examples:
     - `src/main.rs:303`
178. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `invert_test::*`
   - Examples:
     - `src/main.rs:305`
179. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray::*`
   - Examples:
     - `src/main.rs:307`
180. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isArray_test::*`
   - Examples:
     - `src/main.rs:309`
181. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt::*`
   - Examples:
     - `src/main.rs:311`
182. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBigInt_test::*`
   - Examples:
     - `src/main.rs:313`
183. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean::*`
   - Examples:
     - `src/main.rs:315`
184. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isBoolean_test::*`
   - Examples:
     - `src/main.rs:317`
185. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate::*`
   - Examples:
     - `src/main.rs:319`
186. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDate_test::*`
   - Examples:
     - `src/main.rs:321`
187. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDeepEqual_test::*`
   - Examples:
     - `src/main.rs:325`
188. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined::*`
   - Examples:
     - `src/main.rs:327`
189. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isDefined_test::*`
   - Examples:
     - `src/main.rs:329`
190. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty::*`
   - Examples:
     - `src/main.rs:331`
191. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmpty_test::*`
   - Examples:
     - `src/main.rs:333`
192. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish::*`
   - Examples:
     - `src/main.rs:335`
193. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isEmptyish_test::*`
   - Examples:
     - `src/main.rs:337`
194. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError::*`
   - Examples:
     - `src/main.rs:339`
195. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isError_test::*`
   - Examples:
     - `src/main.rs:341`
196. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction::*`
   - Examples:
     - `src/main.rs:343`
197. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isFunction_test::*`
   - Examples:
     - `src/main.rs:345`
198. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn::*`
   - Examples:
     - `src/main.rs:347`
199. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isIncludedIn_test::*`
   - Examples:
     - `src/main.rs:349`
200. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull::*`
   - Examples:
     - `src/main.rs:351`
201. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNull_test::*`
   - Examples:
     - `src/main.rs:353`
202. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish::*`
   - Examples:
     - `src/main.rs:355`
203. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNonNullish_test::*`
   - Examples:
     - `src/main.rs:357`
204. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot::*`
   - Examples:
     - `src/main.rs:359`
205. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNot_test::*`
   - Examples:
     - `src/main.rs:361`
206. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish::*`
   - Examples:
     - `src/main.rs:363`
207. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNullish_test::*`
   - Examples:
     - `src/main.rs:365`
208. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber::*`
   - Examples:
     - `src/main.rs:367`
209. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isNumber_test::*`
   - Examples:
     - `src/main.rs:369`
210. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType::*`
   - Examples:
     - `src/main.rs:371`
211. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isObjectType_test::*`
   - Examples:
     - `src/main.rs:373`
212. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPlainObject_test::*`
   - Examples:
     - `src/main.rs:377`
213. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise::*`
   - Examples:
     - `src/main.rs:379`
214. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isPromise_test::*`
   - Examples:
     - `src/main.rs:381`
215. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual::*`
   - Examples:
     - `src/main.rs:383`
216. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isShallowEqual_test::*`
   - Examples:
     - `src/main.rs:385`
217. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual::*`
   - Examples:
     - `src/main.rs:387`
218. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isStrictEqual_test::*`
   - Examples:
     - `src/main.rs:389`
219. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString::*`
   - Examples:
     - `src/main.rs:391`
220. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isString_test::*`
   - Examples:
     - `src/main.rs:393`
221. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol::*`
   - Examples:
     - `src/main.rs:395`
222. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isSymbol_test::*`
   - Examples:
     - `src/main.rs:397`
223. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy::*`
   - Examples:
     - `src/main.rs:399`
224. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `isTruthy_test::*`
   - Examples:
     - `src/main.rs:401`
225. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join::*`
   - Examples:
     - `src/main.rs:403`
226. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `join_test::*`
   - Examples:
     - `src/main.rs:405`
227. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys::*`
   - Examples:
     - `src/main.rs:407`
228. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `keys_test::*`
   - Examples:
     - `src/main.rs:409`
229. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last::*`
   - Examples:
     - `src/main.rs:411`
230. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `last_test::*`
   - Examples:
     - `src/main.rs:413`
231. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `lazyInvocationCounter::*`
   - Examples:
     - `src/main.rs:417`
232. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length::*`
   - Examples:
     - `src/main.rs:419`
233. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `length_test::*`
   - Examples:
     - `src/main.rs:421`
234. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys::*`
   - Examples:
     - `src/main.rs:425`
235. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapKeys_test::*`
   - Examples:
     - `src/main.rs:427`
236. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj::*`
   - Examples:
     - `src/main.rs:429`
237. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapToObj_test::*`
   - Examples:
     - `src/main.rs:431`
238. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues::*`
   - Examples:
     - `src/main.rs:433`
239. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapValues_test::*`
   - Examples:
     - `src/main.rs:435`
240. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback::*`
   - Examples:
     - `src/main.rs:437`
241. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mapWithFeedback_test::*`
   - Examples:
     - `src/main.rs:439`
242. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `map_test::*`
   - Examples:
     - `src/main.rs:441`
243. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean::*`
   - Examples:
     - `src/main.rs:443`
244. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy::*`
   - Examples:
     - `src/main.rs:445`
245. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `meanBy_test::*`
   - Examples:
     - `src/main.rs:447`
246. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mean_test::*`
   - Examples:
     - `src/main.rs:449`
247. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median::*`
   - Examples:
     - `src/main.rs:451`
248. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `median_test::*`
   - Examples:
     - `src/main.rs:453`
249. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge::*`
   - Examples:
     - `src/main.rs:455`
250. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll::*`
   - Examples:
     - `src/main.rs:457`
251. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeAll_test::*`
   - Examples:
     - `src/main.rs:459`
252. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep::*`
   - Examples:
     - `src/main.rs:461`
253. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `mergeDeep_test::*`
   - Examples:
     - `src/main.rs:463`
254. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `merge_test::*`
   - Examples:
     - `src/main.rs:465`
255. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply::*`
   - Examples:
     - `src/main.rs:467`
256. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `multiply_test::*`
   - Examples:
     - `src/main.rs:469`
257. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy::*`
   - Examples:
     - `src/main.rs:471`
258. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `nthBy_test::*`
   - Examples:
     - `src/main.rs:473`
259. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf::*`
   - Examples:
     - `src/main.rs:475`
260. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `objOf_test::*`
   - Examples:
     - `src/main.rs:477`
261. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit::*`
   - Examples:
     - `src/main.rs:479`
262. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy::*`
   - Examples:
     - `src/main.rs:481`
263. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omitBy_test::*`
   - Examples:
     - `src/main.rs:483`
264. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `omit_test::*`
   - Examples:
     - `src/main.rs:485`
265. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once::*`
   - Examples:
     - `src/main.rs:487`
266. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `once_test::*`
   - Examples:
     - `src/main.rs:489`
267. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only::*`
   - Examples:
     - `src/main.rs:491`
268. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `only_test::*`
   - Examples:
     - `src/main.rs:493`
269. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind::*`
   - Examples:
     - `src/main.rs:495`
270. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialBind_test::*`
   - Examples:
     - `src/main.rs:497`
271. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind::*`
   - Examples:
     - `src/main.rs:499`
272. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partialLastBind_test::*`
   - Examples:
     - `src/main.rs:501`
273. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition::*`
   - Examples:
     - `src/main.rs:503`
274. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `partition_test::*`
   - Examples:
     - `src/main.rs:505`
275. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr::*`
   - Examples:
     - `src/main.rs:507`
276. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pathOr_test::*`
   - Examples:
     - `src/main.rs:509`
277. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick::*`
   - Examples:
     - `src/main.rs:511`
278. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy::*`
   - Examples:
     - `src/main.rs:513`
279. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pickBy_test::*`
   - Examples:
     - `src/main.rs:515`
280. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pick_test::*`
   - Examples:
     - `src/main.rs:517`
281. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pipe_test::*`
   - Examples:
     - `src/main.rs:521`
282. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped::*`
   - Examples:
     - `src/main.rs:523`
283. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `piped_test::*`
   - Examples:
     - `src/main.rs:525`
284. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product::*`
   - Examples:
     - `src/main.rs:527`
285. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `product_test::*`
   - Examples:
     - `src/main.rs:529`
286. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop::*`
   - Examples:
     - `src/main.rs:531`
287. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `prop_test::*`
   - Examples:
     - `src/main.rs:533`
288. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject::*`
   - Examples:
     - `src/main.rs:535`
289. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `pullObject_test::*`
   - Examples:
     - `src/main.rs:537`
290. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry::*`
   - Examples:
     - `src/main.rs:539`
291. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purryFromLazy_test::*`
   - Examples:
     - `src/main.rs:543`
292. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `purry_test::*`
   - Examples:
     - `src/main.rs:549`
293. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomBigInt_test::*`
   - Examples:
     - `src/main.rs:555`
294. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomInteger_test::*`
   - Examples:
     - `src/main.rs:559`
295. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString::*`
   - Examples:
     - `src/main.rs:561`
296. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `randomString_test::*`
   - Examples:
     - `src/main.rs:563`
297. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range::*`
   - Examples:
     - `src/main.rs:565`
298. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `range_test::*`
   - Examples:
     - `src/main.rs:567`
299. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy::*`
   - Examples:
     - `src/main.rs:569`
300. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `rankBy_test::*`
   - Examples:
     - `src/main.rs:571`
301. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reduce_test::*`
   - Examples:
     - `src/main.rs:575`
302. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse::*`
   - Examples:
     - `src/main.rs:577`
303. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `reverse_test::*`
   - Examples:
     - `src/main.rs:579`
304. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round::*`
   - Examples:
     - `src/main.rs:581`
305. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `round_test::*`
   - Examples:
     - `src/main.rs:583`
306. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample::*`
   - Examples:
     - `src/main.rs:585`
307. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sample_test::*`
   - Examples:
     - `src/main.rs:587`
308. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set::*`
   - Examples:
     - `src/main.rs:589`
309. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath::*`
   - Examples:
     - `src/main.rs:591`
310. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `setPath_test::*`
   - Examples:
     - `src/main.rs:593`
311. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `set_test::*`
   - Examples:
     - `src/main.rs:595`
312. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle::*`
   - Examples:
     - `src/main.rs:597`
313. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `shuffle_test::*`
   - Examples:
     - `src/main.rs:599`
314. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString::*`
   - Examples:
     - `src/main.rs:603`
315. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sliceString_test::*`
   - Examples:
     - `src/main.rs:605`
316. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort::*`
   - Examples:
     - `src/main.rs:607`
317. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy::*`
   - Examples:
     - `src/main.rs:609`
318. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortBy_test::*`
   - Examples:
     - `src/main.rs:611`
319. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sort_test::*`
   - Examples:
     - `src/main.rs:613`
320. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex::*`
   - Examples:
     - `src/main.rs:615`
321. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexBy_test::*`
   - Examples:
     - `src/main.rs:619`
322. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith::*`
   - Examples:
     - `src/main.rs:621`
323. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndexWith_test::*`
   - Examples:
     - `src/main.rs:623`
324. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedIndex_test::*`
   - Examples:
     - `src/main.rs:625`
325. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex::*`
   - Examples:
     - `src/main.rs:627`
326. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndexBy_test::*`
   - Examples:
     - `src/main.rs:631`
327. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sortedLastIndex_test::*`
   - Examples:
     - `src/main.rs:633`
328. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice::*`
   - Examples:
     - `src/main.rs:635`
329. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splice_test::*`
   - Examples:
     - `src/main.rs:637`
330. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split::*`
   - Examples:
     - `src/main.rs:639`
331. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt::*`
   - Examples:
     - `src/main.rs:641`
332. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitAt_test::*`
   - Examples:
     - `src/main.rs:643`
333. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen::*`
   - Examples:
     - `src/main.rs:645`
334. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `splitWhen_test::*`
   - Examples:
     - `src/main.rs:647`
335. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `split_test::*`
   - Examples:
     - `src/main.rs:649`
336. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `src_index::*`
   - Examples:
     - `src/main.rs:651`
337. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith::*`
   - Examples:
     - `src/main.rs:653`
338. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `startsWith_test::*`
   - Examples:
     - `src/main.rs:655`
339. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath::*`
   - Examples:
     - `src/main.rs:657`
340. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `stringToPath_test::*`
   - Examples:
     - `src/main.rs:659`
341. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract::*`
   - Examples:
     - `src/main.rs:661`
342. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `subtract_test::*`
   - Examples:
     - `src/main.rs:663`
343. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy::*`
   - Examples:
     - `src/main.rs:667`
344. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sumBy_test::*`
   - Examples:
     - `src/main.rs:669`
345. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `sum_test::*`
   - Examples:
     - `src/main.rs:671`
346. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices::*`
   - Examples:
     - `src/main.rs:675`
347. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapIndices_test::*`
   - Examples:
     - `src/main.rs:677`
348. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps::*`
   - Examples:
     - `src/main.rs:679`
349. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `swapProps_test::*`
   - Examples:
     - `src/main.rs:681`
350. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take::*`
   - Examples:
     - `src/main.rs:683`
351. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy::*`
   - Examples:
     - `src/main.rs:685`
352. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeFirstBy_test::*`
   - Examples:
     - `src/main.rs:687`
353. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast::*`
   - Examples:
     - `src/main.rs:689`
354. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile::*`
   - Examples:
     - `src/main.rs:691`
355. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLastWhile_test::*`
   - Examples:
     - `src/main.rs:693`
356. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeLast_test::*`
   - Examples:
     - `src/main.rs:695`
357. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile::*`
   - Examples:
     - `src/main.rs:697`
358. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `takeWhile_test::*`
   - Examples:
     - `src/main.rs:699`
359. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `take_test::*`
   - Examples:
     - `src/main.rs:701`
360. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap::*`
   - Examples:
     - `src/main.rs:703`
361. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `tap_test::*`
   - Examples:
     - `src/main.rs:705`
362. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `times_test::*`
   - Examples:
     - `src/main.rs:709`
363. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase::*`
   - Examples:
     - `src/main.rs:711`
364. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toCamelCase_test::*`
   - Examples:
     - `src/main.rs:713`
365. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase::*`
   - Examples:
     - `src/main.rs:715`
366. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toKebabCase_test::*`
   - Examples:
     - `src/main.rs:717`
367. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase::*`
   - Examples:
     - `src/main.rs:719`
368. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toLowerCase_test::*`
   - Examples:
     - `src/main.rs:721`
369. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase::*`
   - Examples:
     - `src/main.rs:725`
370. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toSnakeCase_test::*`
   - Examples:
     - `src/main.rs:727`
371. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase::*`
   - Examples:
     - `src/main.rs:729`
372. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toTitleCase_test::*`
   - Examples:
     - `src/main.rs:731`
373. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase::*`
   - Examples:
     - `src/main.rs:733`
374. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `toUpperCase_test::*`
   - Examples:
     - `src/main.rs:735`
375. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate::*`
   - Examples:
     - `src/main.rs:737`
376. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `truncate_test::*`
   - Examples:
     - `src/main.rs:739`
377. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `typesDataProvider::*`
   - Examples:
     - `src/main.rs:741`
378. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize::*`
   - Examples:
     - `src/main.rs:743`
379. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uncapitalize_test::*`
   - Examples:
     - `src/main.rs:745`
380. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy::*`
   - Examples:
     - `src/main.rs:749`
381. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueBy_test::*`
   - Examples:
     - `src/main.rs:751`
382. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith::*`
   - Examples:
     - `src/main.rs:753`
383. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `uniqueWith_test::*`
   - Examples:
     - `src/main.rs:755`
384. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `unique_test::*`
   - Examples:
     - `src/main.rs:757`
385. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values::*`
   - Examples:
     - `src/main.rs:761`
386. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `values_test::*`
   - Examples:
     - `src/main.rs:763`
387. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when::*`
   - Examples:
     - `src/main.rs:765`
388. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `when_test::*`
   - Examples:
     - `src/main.rs:767`
389. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `withPrecision_test::*`
   - Examples:
     - `src/main.rs:771`
390. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `words_test::*`
   - Examples:
     - `src/main.rs:775`
391. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip::*`
   - Examples:
     - `src/main.rs:777`
392. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith::*`
   - Examples:
     - `src/main.rs:779`
393. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zipWith_test::*`
   - Examples:
     - `src/main.rs:781`
394. **warning** `unused_imports` - 1 occurrence
   - Message: unused import: `zip_test::*`
   - Examples:
     - `src/main.rs:783`
395. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:87`
396. **warning** `unused_parens` - 1 occurrence
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
    Checking cfg-if v1.0.4
    Checking memchr v2.8.0
   Compiling serde_core v1.0.228
   Compiling autocfg v1.5.0
   Compiling zmij v1.0.21
   Compiling serde_json v1.0.149
    Checking tinyvec_macros v0.1.1
    Checking aho-corasick v1.1.4
    Checking regex-syntax v0.8.10
   Compiling serde v1.0.228
   Compiling num-traits v0.2.19
    Checking tinyvec v1.11.0
    Checking pin-project-lite v0.2.17
   Compiling syn v2.0.117
    Checking rand_core v0.9.5
    Checking itoa v1.0.18
    Checking iana-time-zone v0.1.65
    Checking chrono v0.4.44
    Checking unicode-normalization v0.1.25
    Checking regex-automata v0.4.14
    Checking regex v1.12.3
    Checking ppv-lite86 v0.2.21
   Compiling serde_derive v1.0.228
   Compiling tokio-macros v2.7.0
    Checking rand_chacha v0.9.0
    Checking rand v0.9.4
    Checking tokio v1.52.3
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 2 previous errors; 750 warnings emitted
```
