# Rust Diagnostics

- Cargo manifest: `/tmp/claude-0/-home-user-smelt/992beec0-8d53-5927-bcf1-98aff0398478/scratchpad/es-toolkit/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `268`
- Warnings: `489`

## Summary By Code

1. **warning** `unused_mut` - 291 diagnostics
2. **error** `E0308` - 170 diagnostics
3. **warning** `unused_assignments` - 108 diagnostics
4. **warning** `unreachable_code` - 48 diagnostics
5. **warning** `unused_parens` - 41 diagnostics
6. **error** `no-code` - 26 diagnostics
7. **error** `E0277` - 19 diagnostics
8. **error** `E0599` - 16 diagnostics
9. **error** `E0609` - 10 diagnostics
10. **error** `E0282` - 7 diagnostics
11. **error** `E0382` - 4 diagnostics
12. **error** `E0596` - 4 diagnostics
13. **error** `E0631` - 3 diagnostics
14. **error** `E0057` - 2 diagnostics
15. **error** `E0658` - 2 diagnostics
16. **error** `E0107` - 1 diagnostic
17. **error** `E0381` - 1 diagnostic
18. **error** `E0425` - 1 diagnostic
19. **error** `E0600` - 1 diagnostic
20. **error** `E0689` - 1 diagnostic
21. **warning** `non_camel_case_types` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 291 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/forEachAsync.rs:17`
     - `src/forEachAsync.rs:22`
     - `src/assignInWith.rs:7`
     - `src/assignValue.rs:7`
     - `src/assignWith.rs:7`
2. **error** `E0308` - 165 occurrences
   - Message: mismatched types
   - Examples:
     - `src/allKeyed.rs:81`
     - `src/attemptAsync.rs:14`
     - `src/attemptAsync.rs:15`
     - `src/attemptAsync.rs:20`
     - `src/attemptAsync.rs:21`
3. **warning** `unreachable_code` - 48 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/bind.rs:60`
     - `src/bindKey.rs:61`
     - `src/cloneDeepWith_1.rs:576`
     - `src/clone_1.rs:387`
     - `src/every.rs:122`
4. **warning** `unused_assignments` - 39 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/flatten.rs:519`
     - `src/flatten.rs:510`
     - `src/flatten.rs:503`
     - `src/flatten.rs:471`
     - `src/flatten.rs:462`
5. **warning** `unused_parens` - 17 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/chunk.rs:14`
     - `src/curryRight.rs:200`
     - `src/decimalAdjust.rs:62`
     - `src/dropRight_1.rs:11`
     - `src/drop_1.rs:8`
6. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/debounce.rs:96`
     - `src/debounce.rs:109`
     - `src/debounce.rs:133`
     - `src/dropWhile_1.rs:11`
     - `src/dropWhile_1.rs:16`
7. **error** `no-code` - 12 occurrences
   - Message: expected pattern, found `*`
   - Examples:
     - `src/filterAsync_spec.rs:316`
     - `src/filterAsync_spec.rs:381`
     - `src/flatMapAsync_spec.rs:387`
     - `src/flatMapAsync_spec.rs:452`
     - `src/forEachAsync_spec.rs:232`
8. **warning** `unused_assignments` - 11 occurrences
   - Message: value assigned to `keys` is never read
   - Examples:
     - `src/filter.rs:73`
     - `src/pick_1.rs:65`
     - `src/pick_1.rs:61`
     - `src/pick_1.rs:73`
     - `src/pick_1.rs:78`
9. **warning** `unused_assignments` - 11 occurrences
   - Message: value assigned to `resolved_path` is never read
   - Examples:
     - `src/has.rs:83`
     - `src/has.rs:80`
     - `src/has.rs:42`
     - `src/hasIn.rs:123`
     - `src/hasIn.rs:120`
10. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `i_2` is never read
   - Examples:
     - `src/mergeWith_1.rs:26504`
     - `src/mergeWith_1.rs:22758`
     - `src/mergeWith_1.rs:19007`
     - `src/mergeWith_1.rs:15261`
     - `src/mergeWith_1.rs:11504`
11. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:3461`
     - `src/main.rs:3515`
     - `src/main.rs:3527`
     - `src/main.rs:3538`
     - `src/main.rs:3694`
12. **error** `E0282` - 7 occurrences
   - Message: type annotations needed
   - Examples:
     - `src/indexOf.rs:39`
     - `src/omitBy_1.rs:59`
     - `src/omit_1.rs:144`
     - `src/omit_1.rs:200`
     - `src/pickBy_1.rs:72`
13. **warning** `unused_assignments` - 7 occurrences
   - Message: value assigned to `predicate` is never read
   - Examples:
     - `src/every.rs:77`
     - `src/every.rs:99`
     - `src/every.rs:96`
     - `src/every.rs:105`
     - `src/every.rs:110`
14. **error** `no-code` - 6 occurrences
   - Message: expected identifier, found `(`
   - Examples:
     - `src/cond.rs:25`
     - `src/cond.rs:26`
     - `src/cond.rs:42`
     - `src/cond.rs:42`
     - `src/cond.rs:44`
15. **error** `E0599` - 6 occurrences
   - Message: `SmeltUnknown` is not an iterator
   - Examples:
     - `src/isEqualWith.rs:47`
     - `src/isEqualWith.rs:56`
     - `src/isEqualWith.rs:67`
     - `src/isEqualWith.rs:80`
     - `src/isEqualWith.rs:89`
16. **error** `E0609` - 5 occurrences
   - Message: no field `apply` on type `DebouncedFunction<SmeltErasedFunction>`
   - Examples:
     - `src/debounce.rs:114`
     - `src/debounce.rs:138`
     - `src/debounce.rs:146`
     - `src/throttle.rs:73`
     - `src/throttle.rs:95`
17. **error** `no-code` - 4 occurrences
   - Message: cast cannot be followed by a method call
   - Examples:
     - `src/clone_1.rs:283`
     - `src/clone_1.rs:283`
     - `src/clone_spec.rs:347`
     - `src/clone_spec.rs:347`
18. **error** `E0308` - 4 occurrences
   - Message: arguments to this function are incorrect
   - Examples:
     - `src/isSubset.rs:7`
     - `src/without_1.rs:7`
     - `src/xor_1.rs:7`
     - `src/xor_1.rs:7`
19. **error** `no-code` - 3 occurrences
   - Message: `<` is interpreted as a start of generic arguments for `f64`, not a comparison
   - Examples:
     - `src/initial_spec.rs:81`
     - `src/initial_spec.rs:91`
     - `src/last_spec.rs:77`
20. **error** `E0631` - 3 occurrences
   - Message: type mismatch in closure arguments
   - Examples:
     - `src/matchesProperty.rs:27`
     - `src/matchesProperty.rs:65`
     - `src/matchesProperty.rs:100`
21. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/findIndex.rs:126`
     - `src/findLastIndex.rs:127`
     - `src/pullAllBy.rs:68`
22. **error** `E0277` - 2 occurrences
   - Message: can't compare `std::option::Option<SmeltUnknown>` with `{float}`
   - Examples:
     - `src/repeat.rs:31`
     - `src/repeat.rs:36`
23. **error** `E0277` - 2 occurrences
   - Message: the trait bound `SmeltUnknown: AsRef<str>` is not satisfied
   - Examples:
     - `src/escape_1.rs:8`
     - `src/unescape_1.rs:8`
24. **error** `E0277` - 2 occurrences
   - Message: the trait bound `dyn Future<Output = Result<SmeltUnknown, Box<dyn StdError>>>: Default` is not satisfied
   - Examples:
     - `src/allKeyed.rs:65`
     - `src/allKeyed.rs:65`
25. **error** `E0382` - 2 occurrences
   - Message: use of moved value: `predicate_1`
   - Examples:
     - `src/filter.rs:62`
     - `src/partition.rs:59`
26. **error** `E0596` - 2 occurrences
   - Message: cannot borrow `self.__data__` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/main.rs:5208`
     - `src/main.rs:5216`
27. **error** `E0596` - 2 occurrences
   - Message: cannot borrow data in an `Rc` as mutable
   - Examples:
     - `src/after.rs:33`
     - `src/before.rs:78`
28. **error** `E0599` - 2 occurrences
   - Message: no method named `same_js_key` found for enum `SmeltUnion359` in the current scope
   - Examples:
     - `src/decimalAdjust.rs:56`
     - `src/decimalAdjust.rs:135`
29. **error** `E0609` - 2 occurrences
   - Message: no field `length` on type `SmeltList<SmeltUnknown>`
   - Examples:
     - `src/unzipWith_1.rs:19`
     - `src/unzipWith_1.rs:22`
30. **error** `E0609` - 2 occurrences
   - Message: no field `result` on type `&SmeltMatch`
   - Examples:
     - `src/truncate.rs:170`
     - `src/truncate.rs:249`
31. **error** `E0658` - 2 occurrences
   - Message: use of unstable library feature `fn_traits`
   - Examples:
     - `src/attempt_1.rs:14`
     - `src/unzipWith_1.rs:45`
32. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/slice.rs:54`
     - `src/slice.rs:48`
33. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/findIndex.rs:109`
     - `src/findLastIndex.rs:124`
34. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `mapped` is never read
   - Examples:
     - `src/flatMap.rs:28`
     - `src/flatMap.rs:21`
35. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_error` is never read
   - Examples:
     - `src/clone.rs:195`
     - `src/clone.rs:111`
36. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_object` is never read
   - Examples:
     - `src/clone.rs:228`
     - `src/clone.rs:144`
37. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/filter.rs:69`
     - `src/sumBy_1.rs:49`
38. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `start_index` is never read
   - Examples:
     - `src/reduce.rs:54`
     - `src/reduceRight.rs:56`
39. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `timeout_id` is never read
   - Examples:
     - `src/delay_1.rs:58`
     - `src/timeout.rs:46`
40. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around `match` scrutinee expression
   - Examples:
     - `src/findIndex.rs:81`
     - `src/findLastIndex.rs:106`
41. **error** `no-code` - 1 occurrence
   - Message: lifetime may not live long enough
   - Examples:
     - `src/cloneDeepWith_1.rs:7`
42. **error** `E0057` - 1 occurrence
   - Message: this function takes 0 arguments but 1 argument was supplied
   - Examples:
     - `src/once_1.rs:21`
43. **error** `E0057` - 1 occurrence
   - Message: this function takes 1 argument but 0 arguments were supplied
   - Examples:
     - `src/once_1.rs:29`
44. **error** `E0107` - 1 occurrence
   - Message: missing generics for struct `ImmutableCache`
   - Examples:
     - `src/main.rs:5237`
45. **error** `E0277` - 1 occurrence
   - Message: `A` doesn't implement `std::fmt::Display`
   - Examples:
     - `src/main.rs:5315`
46. **error** `E0277` - 1 occurrence
   - Message: `SmeltJsMap<SmeltUnknown, std::string::String>` doesn't implement `Debug`
   - Examples:
     - `src/main.rs:4297`
47. **error** `E0277` - 1 occurrence
   - Message: `SmeltJsMap<T, std::string::String>` doesn't implement `Debug`
   - Examples:
     - `src/main.rs:4308`
48. **error** `E0277` - 1 occurrence
   - Message: a value of type `SmeltJsMap<T, std::string::String>` cannot be built from an iterator over elements of type `(SmeltUnknown, std::string::String)`
   - Examples:
     - `src/main.rs:5233`
49. **error** `E0277` - 1 occurrence
   - Message: can't compare `f64` with `std::option::Option<SmeltUnion359>`
   - Examples:
     - `src/rangeRight_1.rs:66`
50. **error** `E0277` - 1 occurrence
   - Message: can't compare `f64` with `std::option::Option<SmeltUnknown>`
   - Examples:
     - `src/range.rs:65`
51. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltRecord<std::string::String, SmeltUnknown>: serde::Deserialize<'de>` is not satisfied
   - Examples:
     - `src/isJSON.rs:16`
52. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltUnion359: Eq` is not satisfied
   - Examples:
     - `src/pullAt.rs:87`
53. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltUnion359: Hash` is not satisfied
   - Examples:
     - `src/pullAt.rs:87`
54. **error** `E0277` - 1 occurrence
   - Message: the trait bound `T: Eq` is not satisfied
   - Examples:
     - `src/uniq_1.rs:8`
55. **error** `E0277` - 1 occurrence
   - Message: the trait bound `T: Hash` is not satisfied
   - Examples:
     - `src/uniq_1.rs:8`
56. **error** `E0277` - 1 occurrence
   - Message: the trait bound `f64: Eq` is not satisfied
   - Examples:
     - `src/pullAt_1.rs:27`
57. **error** `E0277` - 1 occurrence
   - Message: the trait bound `f64: Hash` is not satisfied
   - Examples:
     - `src/pullAt_1.rs:27`
58. **error** `E0308` - 1 occurrence
   - Message: `match` arms have incompatible types
   - Examples:
     - `src/delay.rs:14`
59. **error** `E0381` - 1 occurrence
   - Message: used binding `i_1` isn't initialized
   - Examples:
     - `src/some.rs:138`
60. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `_smelt_tmp_11`
   - Examples:
     - `src/unionBy.rs:34`
61. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `iteratee_1`
   - Examples:
     - `src/uniqBy.rs:24`
62. **error** `E0425` - 1 occurrence
   - Message: cannot find value `smelt_callback` in this scope
   - Examples:
     - `src/template.rs:408`
63. **error** `E0599` - 1 occurrence
   - Message: no method named `clear` found for struct `SmeltJsMap<K, V>` in the current scope
   - Examples:
     - `src/main.rs:5220`
64. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for struct `Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>` in the current scope
   - Examples:
     - `src/isMatchWith.rs:61`
65. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for struct `Rc<{closure@src/toolkit.rs:12:43: 12:72}>` in the current scope
   - Examples:
     - `src/toolkit.rs:16`
66. **error** `E0599` - 1 occurrence
   - Message: the method `contains_key` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5245`
67. **error** `E0599` - 1 occurrence
   - Message: the method `get` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5241`
68. **error** `E0599` - 1 occurrence
   - Message: the method `insert` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5249`
69. **error** `E0599` - 1 occurrence
   - Message: the method `len` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5257`
70. **error** `E0599` - 1 occurrence
   - Message: the method `remove` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5253`
71. **error** `E0600` - 1 occurrence
   - Message: cannot apply unary operator `-` to type `std::option::Option<f64>`
   - Examples:
     - `src/takeRight_1.rs:41`
72. **error** `E0609` - 1 occurrence
   - Message: no field `name` on type `HttpError`
   - Examples:
     - `src/main.rs:5323`
73. **error** `E0689` - 1 occurrence
   - Message: can't call method `max` on ambiguous numeric type `{float}`
   - Examples:
     - `src/invoke.rs:19`
74. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_2461` should have an upper camel case name
   - Examples:
     - `src/main.rs:4487`
75. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cache_constructor` is never read
   - Examples:
     - `src/memoize_1.rs:140`
76. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `customizer_fn` is never read
   - Examples:
     - `src/setWith.rs:11`
77. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `debounced_1` is never read
   - Examples:
     - `src/debounce_1.rs:228`
78. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `i_1` is never read
   - Examples:
     - `src/filter.rs:81`
79. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key_1` is never read
   - Examples:
     - `src/some.rs:166`
80. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `length_1` is never read
   - Examples:
     - `src/filter.rs:80`
81. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `path` is never read
   - Examples:
     - `src/result.rs:38`
82. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `predicate_1` is never read
   - Examples:
     - `src/filter.rs:63`
83. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_1` is never read
   - Examples:
     - `src/filter.rs:77`
84. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_length` is never read
   - Examples:
     - `src/pullAllWith.rs:57`
85. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `set_low` is never read
   - Examples:
     - `src/sortedIndexBy.rs:99`
86. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/slice.rs:53`
87. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_1` is never read
   - Examples:
     - `src/some.rs:168`
88. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/now.rs:9`

## Cargo Stderr

```text
Updating crates.io index
     Locking 69 packages to latest Rust 1.96.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.18.0)
      Adding rand v0.9.4 (available: v0.10.2)
    Checking es_toolkit_probe v0.1.0 (/tmp/claude-0/-home-user-smelt/992beec0-8d53-5927-bcf1-98aff0398478/scratchpad/es-toolkit/dist-smelt)
error: could not compile `es_toolkit_probe` (bin "es_toolkit_probe") due to 268 previous errors; 489 warnings emitted
```
