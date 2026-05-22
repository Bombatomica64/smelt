# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `222`
- Warnings: `185`

## Summary By Code

1. **error** `E0618` - 208 diagnostics
2. **warning** `unused_mut` - 118 diagnostics
3. **warning** `unused_parens` - 37 diagnostics
4. **warning** `unused_assignments` - 29 diagnostics
5. **error** `E0277` - 10 diagnostics
6. **error** `E0308` - 3 diagnostics
7. **error** `E0271` - 1 diagnostic
8. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 118 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/clone.rs:16`
     - `src/clone.rs:17`
     - `src/conditional.rs:94`
     - `src/conditional.rs:95`
     - `src/conditional.rs:96`
2. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:46`
     - `src/dropWhile.rs:38`
     - `src/filter.rs:18`
     - `src/find.rs:19`
     - `src/findIndex.rs:17`
3. **warning** `unused_parens` - 11 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:83`
     - `src/allPass_test.rs:84`
     - `src/anyPass_test.rs:83`
     - `src/anyPass_test.rs:84`
     - `src/purryOrderRules.rs:148`
4. **error** `E0277` - 8 occurrences
   - Message: the `?` operator can only be used in a closure that returns `Result` or `Option` (or another type that implements `FromResidual`)
   - Examples:
     - `src/chunk.rs:8`
     - `src/conditional.rs:12`
     - `src/dropFirstBy.rs:8`
     - `src/hasSubObject.rs:8`
     - `src/lazyInvocationCounter.rs:16`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `burst_remaining_ms` is never read
   - Examples:
     - `src/funnel.rs:211`
     - `src/funnel.rs:194`
     - `src/funnel.rs:150`
     - `src/funnel.rs:133`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:92`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:77`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:78`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:63`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `cancel` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:94`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:79`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:80`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:65`
8. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:91`
     - `src/funnel_lodash_debounce_test.rs:78`
     - `src/funnel_lodash_throttle_test.rs:77`
     - `src/funnel_lodash_throttle_test.rs:64`
9. **error** `E0308` - 3 occurrences
   - Message: mismatched types
   - Examples:
     - `src/debounce.rs:85`
     - `src/debounce.rs:99`
     - `src/withPrecision.rs:74`
10. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:48`
     - `src/toCamelCase.rs:44`
     - `src/toCamelCase.rs:46`
11. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:127`
     - `src/funnel.rs:188`
     - `src/splitAt.rs:37`
12. **error** `E0277` - 2 occurrences
   - Message: the `?` operator can only be used on `Option`s, not `Result`s, in a closure that returns `Option`
   - Examples:
     - `src/firstBy.rs:8`
     - `src/mean.rs:8`
13. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/add.rs:11:151: 11:187}>>`
   - Examples:
     - `src/add.rs:11`
     - `src/add.rs:11`
14. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/addProp.rs:11:151: 11:187}>>`
   - Examples:
     - `src/addProp.rs:11`
     - `src/addProp.rs:11`
15. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/allPass.rs:11:151: 11:187}>>`
   - Examples:
     - `src/allPass.rs:11`
     - `src/allPass.rs:11`
16. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/anyPass.rs:11:151: 11:187}>>`
   - Examples:
     - `src/anyPass.rs:11`
     - `src/anyPass.rs:11`
17. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/capitalize.rs:11:151: 11:187}>>`
   - Examples:
     - `src/capitalize.rs:11`
     - `src/capitalize.rs:11`
18. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/ceil.rs:12:151: 12:187}>>`
   - Examples:
     - `src/ceil.rs:12`
     - `src/ceil.rs:12`
19. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/chunk.rs:11:151: 11:187}>>`
   - Examples:
     - `src/chunk.rs:11`
     - `src/chunk.rs:11`
20. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/clamp.rs:11:151: 11:187}>>`
   - Examples:
     - `src/clamp.rs:11`
     - `src/clamp.rs:11`
21. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/clone.rs:11:151: 11:187}>>`
   - Examples:
     - `src/clone.rs:11`
     - `src/clone.rs:11`
22. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/concat.rs:11:151: 11:187}>>`
   - Examples:
     - `src/concat.rs:11`
     - `src/concat.rs:11`
23. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/countBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/countBy.rs:11`
     - `src/countBy.rs:11`
24. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/defaultTo.rs:11:151: 11:187}>>`
   - Examples:
     - `src/defaultTo.rs:11`
     - `src/defaultTo.rs:11`
25. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/divide.rs:11:151: 11:187}>>`
   - Examples:
     - `src/divide.rs:11`
     - `src/divide.rs:11`
26. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/drop.rs:12:151: 12:187}>>`
   - Examples:
     - `src/drop.rs:12`
     - `src/drop.rs:12`
27. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/dropLast.rs:11:151: 11:187}>>`
   - Examples:
     - `src/dropLast.rs:11`
     - `src/dropLast.rs:11`
28. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/dropLastWhile.rs:11:151: 11:187}>>`
   - Examples:
     - `src/dropLastWhile.rs:11`
     - `src/dropLastWhile.rs:11`
29. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/dropWhile.rs:11:151: 11:187}>>`
   - Examples:
     - `src/dropWhile.rs:11`
     - `src/dropWhile.rs:11`
30. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/endsWith.rs:11:151: 11:187}>>`
   - Examples:
     - `src/endsWith.rs:11`
     - `src/endsWith.rs:11`
31. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/entries.rs:10:151: 10:187}>>`
   - Examples:
     - `src/entries.rs:10`
     - `src/entries.rs:10`
32. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/evolve.rs:11:151: 11:187}>>`
   - Examples:
     - `src/evolve.rs:11`
     - `src/evolve.rs:11`
33. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/filter.rs:12:151: 12:187}>>`
   - Examples:
     - `src/filter.rs:12`
     - `src/filter.rs:12`
34. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/find.rs:13:151: 13:187}>>`
   - Examples:
     - `src/find.rs:13`
     - `src/find.rs:13`
35. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/findIndex.rs:11:151: 11:187}>>`
   - Examples:
     - `src/findIndex.rs:11`
     - `src/findIndex.rs:11`
36. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/findLast.rs:11:151: 11:187}>>`
   - Examples:
     - `src/findLast.rs:11`
     - `src/findLast.rs:11`
37. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/findLastIndex.rs:11:151: 11:187}>>`
   - Examples:
     - `src/findLastIndex.rs:11`
     - `src/findLastIndex.rs:11`
38. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/first.rs:13:151: 13:187}>>`
   - Examples:
     - `src/first.rs:13`
     - `src/first.rs:13`
39. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/flatMap.rs:12:151: 12:187}>>`
   - Examples:
     - `src/flatMap.rs:12`
     - `src/flatMap.rs:12`
40. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/floor.rs:12:151: 12:187}>>`
   - Examples:
     - `src/floor.rs:12`
     - `src/floor.rs:12`
41. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/forEach.rs:12:151: 12:187}>>`
   - Examples:
     - `src/forEach.rs:12`
     - `src/forEach.rs:12`
42. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/forEachObj.rs:11:151: 11:187}>>`
   - Examples:
     - `src/forEachObj.rs:11`
     - `src/forEachObj.rs:11`
43. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/fromEntries.rs:10:151: 10:187}>>`
   - Examples:
     - `src/fromEntries.rs:10`
     - `src/fromEntries.rs:10`
44. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/fromKeys.rs:11:151: 11:187}>>`
   - Examples:
     - `src/fromKeys.rs:11`
     - `src/fromKeys.rs:11`
45. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/groupBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/groupBy.rs:11`
     - `src/groupBy.rs:11`
46. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/groupByProp.rs:11:151: 11:187}>>`
   - Examples:
     - `src/groupByProp.rs:11`
     - `src/groupByProp.rs:11`
47. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/hasAtLeast.rs:11:151: 11:187}>>`
   - Examples:
     - `src/hasAtLeast.rs:11`
     - `src/hasAtLeast.rs:11`
48. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/hasProp.rs:11:151: 11:187}>>`
   - Examples:
     - `src/hasProp.rs:11`
     - `src/hasProp.rs:11`
49. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/hasSubObject.rs:11:151: 11:187}>>`
   - Examples:
     - `src/hasSubObject.rs:11`
     - `src/hasSubObject.rs:11`
50. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/indexBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/indexBy.rs:11`
     - `src/indexBy.rs:11`
51. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/invert.rs:11:151: 11:187}>>`
   - Examples:
     - `src/invert.rs:11`
     - `src/invert.rs:11`
52. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/isDeepEqual.rs:11:151: 11:187}>>`
   - Examples:
     - `src/isDeepEqual.rs:11`
     - `src/isDeepEqual.rs:11`
53. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/isShallowEqual.rs:11:151: 11:187}>>`
   - Examples:
     - `src/isShallowEqual.rs:11`
     - `src/isShallowEqual.rs:11`
54. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/isStrictEqual.rs:11:151: 11:187}>>`
   - Examples:
     - `src/isStrictEqual.rs:11`
     - `src/isStrictEqual.rs:11`
55. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/join.rs:11:151: 11:187}>>`
   - Examples:
     - `src/join.rs:11`
     - `src/join.rs:11`
56. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/keys.rs:10:151: 10:187}>>`
   - Examples:
     - `src/keys.rs:10`
     - `src/keys.rs:10`
57. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/last.rs:11:151: 11:187}>>`
   - Examples:
     - `src/last.rs:11`
     - `src/last.rs:11`
58. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/length.rs:11:151: 11:187}>>`
   - Examples:
     - `src/length.rs:11`
     - `src/length.rs:11`
59. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/map.rs:12:151: 12:187}>>`
   - Examples:
     - `src/map.rs:12`
     - `src/map.rs:12`
60. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/mapKeys.rs:11:151: 11:187}>>`
   - Examples:
     - `src/mapKeys.rs:11`
     - `src/mapKeys.rs:11`
61. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/mapToObj.rs:11:151: 11:187}>>`
   - Examples:
     - `src/mapToObj.rs:11`
     - `src/mapToObj.rs:11`
62. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/mapValues.rs:11:151: 11:187}>>`
   - Examples:
     - `src/mapValues.rs:11`
     - `src/mapValues.rs:11`
63. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/mean.rs:11:151: 11:187}>>`
   - Examples:
     - `src/mean.rs:11`
     - `src/mean.rs:11`
64. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/meanBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/meanBy.rs:11`
     - `src/meanBy.rs:11`
65. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/median.rs:11:151: 11:187}>>`
   - Examples:
     - `src/median.rs:11`
     - `src/median.rs:11`
66. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/merge.rs:11:151: 11:187}>>`
   - Examples:
     - `src/merge.rs:11`
     - `src/merge.rs:11`
67. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/mergeDeep.rs:11:151: 11:187}>>`
   - Examples:
     - `src/mergeDeep.rs:11`
     - `src/mergeDeep.rs:11`
68. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/multiply.rs:11:151: 11:187}>>`
   - Examples:
     - `src/multiply.rs:11`
     - `src/multiply.rs:11`
69. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/objOf.rs:11:151: 11:187}>>`
   - Examples:
     - `src/objOf.rs:11`
     - `src/objOf.rs:11`
70. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/omit.rs:11:151: 11:187}>>`
   - Examples:
     - `src/omit.rs:11`
     - `src/omit.rs:11`
71. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/omitBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/omitBy.rs:11`
     - `src/omitBy.rs:11`
72. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/only.rs:11:151: 11:187}>>`
   - Examples:
     - `src/only.rs:11`
     - `src/only.rs:11`
73. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/partition.rs:11:151: 11:187}>>`
   - Examples:
     - `src/partition.rs:11`
     - `src/partition.rs:11`
74. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/pathOr.rs:11:151: 11:187}>>`
   - Examples:
     - `src/pathOr.rs:11`
     - `src/pathOr.rs:11`
75. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/pick.rs:11:151: 11:187}>>`
   - Examples:
     - `src/pick.rs:11`
     - `src/pick.rs:11`
76. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/pickBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/pickBy.rs:11`
     - `src/pickBy.rs:11`
77. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/product.rs:11:151: 11:187}>>`
   - Examples:
     - `src/product.rs:11`
     - `src/product.rs:11`
78. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/pullObject.rs:11:151: 11:187}>>`
   - Examples:
     - `src/pullObject.rs:11`
     - `src/pullObject.rs:11`
79. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/purry_test.rs:16:151: 16:187}>>`
   - Examples:
     - `src/purry_test.rs:16`
     - `src/purry_test.rs:16`
80. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/randomString.rs:11:151: 11:187}>>`
   - Examples:
     - `src/randomString.rs:11`
     - `src/randomString.rs:11`
81. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/range.rs:11:151: 11:187}>>`
   - Examples:
     - `src/range.rs:11`
     - `src/range.rs:11`
82. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/reduce.rs:11:151: 11:187}>>`
   - Examples:
     - `src/reduce.rs:11`
     - `src/reduce.rs:11`
83. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/reverse.rs:11:151: 11:187}>>`
   - Examples:
     - `src/reverse.rs:11`
     - `src/reverse.rs:11`
84. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/round.rs:12:151: 12:187}>>`
   - Examples:
     - `src/round.rs:12`
     - `src/round.rs:12`
85. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sample.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sample.rs:11`
     - `src/sample.rs:11`
86. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/set.rs:11:151: 11:187}>>`
   - Examples:
     - `src/set.rs:11`
     - `src/set.rs:11`
87. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/setPath.rs:11:151: 11:187}>>`
   - Examples:
     - `src/setPath.rs:11`
     - `src/setPath.rs:11`
88. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/shuffle.rs:11:151: 11:187}>>`
   - Examples:
     - `src/shuffle.rs:11`
     - `src/shuffle.rs:11`
89. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sort.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sort.rs:11`
     - `src/sort.rs:11`
90. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sortedIndex.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sortedIndex.rs:11`
     - `src/sortedIndex.rs:11`
91. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sortedIndexBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sortedIndexBy.rs:11`
     - `src/sortedIndexBy.rs:11`
92. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sortedIndexWith.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sortedIndexWith.rs:11`
     - `src/sortedIndexWith.rs:11`
93. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sortedLastIndex.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sortedLastIndex.rs:11`
     - `src/sortedLastIndex.rs:11`
94. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sortedLastIndexBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sortedLastIndexBy.rs:11`
     - `src/sortedLastIndexBy.rs:11`
95. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/splice.rs:11:151: 11:187}>>`
   - Examples:
     - `src/splice.rs:11`
     - `src/splice.rs:11`
96. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/splitAt.rs:11:151: 11:187}>>`
   - Examples:
     - `src/splitAt.rs:11`
     - `src/splitAt.rs:11`
97. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/splitWhen.rs:11:151: 11:187}>>`
   - Examples:
     - `src/splitWhen.rs:11`
     - `src/splitWhen.rs:11`
98. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/startsWith.rs:11:151: 11:187}>>`
   - Examples:
     - `src/startsWith.rs:11`
     - `src/startsWith.rs:11`
99. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/subtract.rs:11:151: 11:187}>>`
   - Examples:
     - `src/subtract.rs:11`
     - `src/subtract.rs:11`
100. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sum.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sum.rs:11`
     - `src/sum.rs:11`
101. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/sumBy.rs:11:151: 11:187}>>`
   - Examples:
     - `src/sumBy.rs:11`
     - `src/sumBy.rs:11`
102. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/swapIndices.rs:11:151: 11:187}>>`
   - Examples:
     - `src/swapIndices.rs:11`
     - `src/swapIndices.rs:11`
103. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/swapProps.rs:11:151: 11:187}>>`
   - Examples:
     - `src/swapProps.rs:11`
     - `src/swapProps.rs:11`
104. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/take.rs:12:151: 12:187}>>`
   - Examples:
     - `src/take.rs:12`
     - `src/take.rs:12`
105. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/takeLast.rs:11:151: 11:187}>>`
   - Examples:
     - `src/takeLast.rs:11`
     - `src/takeLast.rs:11`
106. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/takeLastWhile.rs:11:151: 11:187}>>`
   - Examples:
     - `src/takeLastWhile.rs:11`
     - `src/takeLastWhile.rs:11`
107. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/takeWhile.rs:11:151: 11:187}>>`
   - Examples:
     - `src/takeWhile.rs:11`
     - `src/takeWhile.rs:11`
108. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/tap.rs:11:151: 11:187}>>`
   - Examples:
     - `src/tap.rs:11`
     - `src/tap.rs:11`
109. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/times.rs:11:151: 11:187}>>`
   - Examples:
     - `src/times.rs:11`
     - `src/times.rs:11`
110. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/toKebabCase.rs:11:151: 11:187}>>`
   - Examples:
     - `src/toKebabCase.rs:11`
     - `src/toKebabCase.rs:11`
111. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/toLowerCase.rs:11:151: 11:187}>>`
   - Examples:
     - `src/toLowerCase.rs:11`
     - `src/toLowerCase.rs:11`
112. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/toSnakeCase.rs:11:151: 11:187}>>`
   - Examples:
     - `src/toSnakeCase.rs:11`
     - `src/toSnakeCase.rs:11`
113. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/toUpperCase.rs:11:151: 11:187}>>`
   - Examples:
     - `src/toUpperCase.rs:11`
     - `src/toUpperCase.rs:11`
114. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/uncapitalize.rs:11:151: 11:187}>>`
   - Examples:
     - `src/uncapitalize.rs:11`
     - `src/uncapitalize.rs:11`
115. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/values.rs:10:151: 10:187}>>`
   - Examples:
     - `src/values.rs:10`
     - `src/values.rs:10`
116. **error** `E0618` - 2 occurrences
   - Message: expected function, found `&mut Rc<RefCell<{closure@src/zip.rs:12:151: 12:187}>>`
   - Examples:
     - `src/zip.rs:12`
     - `src/zip.rs:12`
117. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/sample.rs:70`
     - `src/take.rs:31`
118. **error** `E0271` - 1 occurrence
   - Message: expected `{closure@debounce.rs:20:289}` to return `Result<(), Box<dyn Error>>`, but it returns `()`
   - Examples:
     - `src/debounce.rs:20`
119. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:85`
120. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:57`
121. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:56`
122. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:28`
123. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:102`
124. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `index` is never read
   - Examples:
     - `src/pipe.rs:182`
125. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:19`
126. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:129`
127. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `lazy_definition` is never read
   - Examples:
     - `src/purryFromLazy.rs:29`
128. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:30`
129. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:663`
130. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `then` is never read
   - Examples:
     - `src/conditional.rs:118`
131. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `when` is never read
   - Examples:
     - `src/conditional.rs:116`
132. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `word` is never read
   - Examples:
     - `src/words.rs:101`
133. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:87`
134. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:19`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 222 previous errors; 185 warnings emitted
```
