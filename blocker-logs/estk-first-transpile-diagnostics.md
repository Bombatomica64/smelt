# Rust Diagnostics

- Cargo manifest: `/tmp/claude-0/-home-user-smelt/992beec0-8d53-5927-bcf1-98aff0398478/scratchpad/es-toolkit/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `525`
- Warnings: `387`

## Summary By Code

1. **error** `E0308` - 394 diagnostics
2. **warning** `unused_mut` - 244 diagnostics
3. **warning** `unused_assignments` - 57 diagnostics
4. **warning** `unreachable_code` - 44 diagnostics
5. **warning** `unused_parens` - 41 diagnostics
6. **error** `no-code` - 26 diagnostics
7. **error** `E0599` - 20 diagnostics
8. **error** `E0277` - 19 diagnostics
9. **error** `E0004` - 16 diagnostics
10. **error** `E0596` - 12 diagnostics
11. **error** `E0609` - 10 diagnostics
12. **error** `E0425` - 7 diagnostics
13. **error** `E0631` - 5 diagnostics
14. **error** `E0282` - 4 diagnostics
15. **error** `E0382` - 3 diagnostics
16. **error** `E0057` - 2 diagnostics
17. **error** `E0658` - 2 diagnostics
18. **error** `E0107` - 1 diagnostic
19. **error** `E0271` - 1 diagnostic
20. **error** `E0381` - 1 diagnostic
21. **error** `E0600` - 1 diagnostic
22. **error** `E0689` - 1 diagnostic
23. **warning** `non_camel_case_types` - 1 diagnostic

## Groups

1. **error** `E0308` - 389 occurrences
   - Message: mismatched types
   - Examples:
     - `src/allKeyed.rs:65`
     - `src/allKeyed.rs:65`
     - `src/allKeyed.rs:65`
     - `src/allKeyed.rs:65`
     - `src/allKeyed.rs:65`
2. **warning** `unused_mut` - 244 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/assignInWith.rs:7`
     - `src/assignValue.rs:7`
     - `src/assignWith.rs:7`
     - `src/cartesianProduct.rs:10`
     - `src/clone.rs:91`
3. **warning** `unreachable_code` - 44 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/bind.rs:60`
     - `src/bindKey.rs:61`
     - `src/cloneDeepWith_1.rs:576`
     - `src/clone_1.rs:387`
     - `src/every.rs:122`
4. **warning** `unused_parens` - 17 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/chunk.rs:14`
     - `src/curryRight.rs:200`
     - `src/decimalAdjust.rs:62`
     - `src/dropRight_1.rs:11`
     - `src/drop_1.rs:8`
5. **error** `E0004` - 16 occurrences
   - Message: non-exhaustive patterns: `Some(SmeltUnknown::Undefined)` not covered
   - Examples:
     - `src/ary.rs:15`
     - `src/cloneWith.rs:13`
     - `src/curry.rs:22`
     - `src/curryRight.rs:22`
     - `src/defaults.rs:43`
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
12. **error** `no-code` - 6 occurrences
   - Message: expected identifier, found `(`
   - Examples:
     - `src/cond.rs:25`
     - `src/cond.rs:26`
     - `src/cond.rs:42`
     - `src/cond.rs:42`
     - `src/cond.rs:44`
13. **error** `E0599` - 6 occurrences
   - Message: `SmeltUnknown` is not an iterator
   - Examples:
     - `src/isEqualWith.rs:47`
     - `src/isEqualWith.rs:56`
     - `src/isEqualWith.rs:67`
     - `src/isEqualWith.rs:80`
     - `src/isEqualWith.rs:89`
14. **error** `E0609` - 5 occurrences
   - Message: no field `apply` on type `DebouncedFunction<SmeltErasedFunction>`
   - Examples:
     - `src/debounce.rs:114`
     - `src/debounce.rs:138`
     - `src/debounce.rs:146`
     - `src/throttle.rs:73`
     - `src/throttle.rs:95`
15. **error** `no-code` - 4 occurrences
   - Message: cast cannot be followed by a method call
   - Examples:
     - `src/clone_1.rs:283`
     - `src/clone_1.rs:283`
     - `src/clone_spec.rs:347`
     - `src/clone_spec.rs:347`
16. **error** `E0282` - 4 occurrences
   - Message: type annotations needed
   - Examples:
     - `src/omitBy_1.rs:59`
     - `src/omit_1.rs:144`
     - `src/omit_1.rs:200`
     - `src/pickBy_1.rs:72`
17. **error** `E0308` - 4 occurrences
   - Message: arguments to this function are incorrect
   - Examples:
     - `src/isSubset.rs:7`
     - `src/without_1.rs:7`
     - `src/xor_1.rs:7`
     - `src/xor_1.rs:7`
18. **error** `E0425` - 4 occurrences
   - Message: cannot find value `smelt_capture_self` in this scope
   - Examples:
     - `src/main.rs:5082`
     - `src/main.rs:5084`
     - `src/main.rs:5085`
     - `src/main.rs:5089`
19. **error** `no-code` - 3 occurrences
   - Message: `<` is interpreted as a start of generic arguments for `f64`, not a comparison
   - Examples:
     - `src/initial_spec.rs:81`
     - `src/initial_spec.rs:91`
     - `src/last_spec.rs:77`
20. **error** `E0596` - 3 occurrences
   - Message: cannot borrow `*mapper` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/unionBy_1.rs:9`
     - `src/xorBy_1.rs:12`
     - `src/xorBy_1.rs:14`
21. **error** `E0631` - 3 occurrences
   - Message: type mismatch in closure arguments
   - Examples:
     - `src/matchesProperty.rs:27`
     - `src/matchesProperty.rs:65`
     - `src/matchesProperty.rs:100`
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
26. **error** `E0425` - 2 occurrences
   - Message: cannot find function `smelt_capture_` in this scope
   - Examples:
     - `src/debounce_1.rs:117`
     - `src/debounce_1.rs:129`
27. **error** `E0596` - 2 occurrences
   - Message: cannot borrow `*are_elements_equal` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/xorWith_1.rs:12`
     - `src/xorWith_1.rs:14`
28. **error** `E0596` - 2 occurrences
   - Message: cannot borrow `self.__data__` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/main.rs:5169`
     - `src/main.rs:5177`
29. **error** `E0596` - 2 occurrences
   - Message: cannot borrow `self.semaphore` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/main.rs:5146`
     - `src/main.rs:5152`
30. **error** `E0596` - 2 occurrences
   - Message: cannot borrow data in an `Rc` as mutable
   - Examples:
     - `src/after.rs:33`
     - `src/before.rs:78`
31. **error** `E0599` - 2 occurrences
   - Message: no method named `len` found for tuple `(SmeltUnknown, SmeltUnknown)` in the current scope
   - Examples:
     - `src/dropWhile.rs:61`
     - `src/findIndex.rs:101`
32. **error** `E0599` - 2 occurrences
   - Message: no method named `len` found for tuple `(std::string::String, SmeltUnknown)` in the current scope
   - Examples:
     - `src/dropRightWhile.rs:63`
     - `src/findLastIndex.rs:115`
33. **error** `E0599` - 2 occurrences
   - Message: no method named `same_js_key` found for enum `SmeltUnion359` in the current scope
   - Examples:
     - `src/decimalAdjust.rs:56`
     - `src/decimalAdjust.rs:135`
34. **error** `E0609` - 2 occurrences
   - Message: no field `length` on type `SmeltList<SmeltUnknown>`
   - Examples:
     - `src/unzipWith_1.rs:19`
     - `src/unzipWith_1.rs:22`
35. **error** `E0609` - 2 occurrences
   - Message: no field `result` on type `&SmeltMatch`
   - Examples:
     - `src/truncate.rs:170`
     - `src/truncate.rs:249`
36. **error** `E0631` - 2 occurrences
   - Message: type mismatch in function arguments
   - Examples:
     - `src/trimEnd_1.rs:33`
     - `src/trimStart_1.rs:31`
37. **error** `E0658` - 2 occurrences
   - Message: use of unstable library feature `fn_traits`
   - Examples:
     - `src/attempt_1.rs:14`
     - `src/unzipWith_1.rs:45`
38. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `end` is never read
   - Examples:
     - `src/slice.rs:54`
     - `src/slice.rs:48`
39. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `i` is never read
   - Examples:
     - `src/some.rs:130`
     - `src/sumBy_1.rs:50`
40. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `mapped` is never read
   - Examples:
     - `src/flatMap.rs:28`
     - `src/flatMap.rs:21`
41. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_error` is never read
   - Examples:
     - `src/clone.rs:195`
     - `src/clone.rs:111`
42. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `new_object` is never read
   - Examples:
     - `src/clone.rs:228`
     - `src/clone.rs:144`
43. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/filter.rs:69`
     - `src/sumBy_1.rs:49`
44. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `start_index` is never read
   - Examples:
     - `src/reduce.rs:54`
     - `src/reduceRight.rs:56`
45. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around `match` scrutinee expression
   - Examples:
     - `src/findIndex.rs:81`
     - `src/findLastIndex.rs:106`
46. **error** `no-code` - 1 occurrence
   - Message: lifetime may not live long enough
   - Examples:
     - `src/cloneDeepWith_1.rs:7`
47. **error** `E0057` - 1 occurrence
   - Message: this function takes 0 arguments but 1 argument was supplied
   - Examples:
     - `src/once_1.rs:21`
48. **error** `E0057` - 1 occurrence
   - Message: this function takes 1 argument but 0 arguments were supplied
   - Examples:
     - `src/once_1.rs:29`
49. **error** `E0107` - 1 occurrence
   - Message: missing generics for struct `ImmutableCache`
   - Examples:
     - `src/main.rs:5194`
50. **error** `E0271` - 1 occurrence
   - Message: expected `{closure@flatten_1.rs:20:5}` to return `SmeltUnknown`, but it returns `()`
   - Examples:
     - `src/flatten_1.rs:20`
51. **error** `E0277` - 1 occurrence
   - Message: `A` doesn't implement `std::fmt::Display`
   - Examples:
     - `src/main.rs:5268`
52. **error** `E0277` - 1 occurrence
   - Message: `SmeltJsMap<SmeltUnknown, std::string::String>` doesn't implement `Debug`
   - Examples:
     - `src/main.rs:4281`
53. **error** `E0277` - 1 occurrence
   - Message: `SmeltJsMap<T, std::string::String>` doesn't implement `Debug`
   - Examples:
     - `src/main.rs:4293`
54. **error** `E0277` - 1 occurrence
   - Message: a value of type `SmeltJsMap<T, std::string::String>` cannot be built from an iterator over elements of type `(SmeltUnknown, std::string::String)`
   - Examples:
     - `src/main.rs:5190`
55. **error** `E0277` - 1 occurrence
   - Message: can't compare `f64` with `std::option::Option<SmeltUnion359>`
   - Examples:
     - `src/rangeRight_1.rs:66`
56. **error** `E0277` - 1 occurrence
   - Message: can't compare `f64` with `std::option::Option<SmeltUnknown>`
   - Examples:
     - `src/range.rs:65`
57. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltRecord<std::string::String, SmeltUnknown>: serde::Deserialize<'de>` is not satisfied
   - Examples:
     - `src/isJSON.rs:16`
58. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltUnion359: Eq` is not satisfied
   - Examples:
     - `src/pullAt.rs:87`
59. **error** `E0277` - 1 occurrence
   - Message: the trait bound `SmeltUnion359: Hash` is not satisfied
   - Examples:
     - `src/pullAt.rs:87`
60. **error** `E0277` - 1 occurrence
   - Message: the trait bound `T: Eq` is not satisfied
   - Examples:
     - `src/uniq_1.rs:8`
61. **error** `E0277` - 1 occurrence
   - Message: the trait bound `T: Hash` is not satisfied
   - Examples:
     - `src/uniq_1.rs:8`
62. **error** `E0277` - 1 occurrence
   - Message: the trait bound `f64: Eq` is not satisfied
   - Examples:
     - `src/pullAt_1.rs:27`
63. **error** `E0277` - 1 occurrence
   - Message: the trait bound `f64: Hash` is not satisfied
   - Examples:
     - `src/pullAt_1.rs:27`
64. **error** `E0308` - 1 occurrence
   - Message: `match` arms have incompatible types
   - Examples:
     - `src/delay.rs:14`
65. **error** `E0381` - 1 occurrence
   - Message: used binding `i_1` isn't initialized
   - Examples:
     - `src/some.rs:138`
66. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `_smelt_tmp_11`
   - Examples:
     - `src/unionBy.rs:34`
67. **error** `E0425` - 1 occurrence
   - Message: cannot find value `smelt_callback` in this scope
   - Examples:
     - `src/template.rs:408`
68. **error** `E0596` - 1 occurrence
   - Message: cannot borrow `*are_items_equal` as mutable, as it is behind a `&` reference
   - Examples:
     - `src/isSubsetWith.rs:10`
69. **error** `E0599` - 1 occurrence
   - Message: no method named `clear` found for struct `SmeltJsMap<K, V>` in the current scope
   - Examples:
     - `src/main.rs:5181`
70. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for struct `Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>` in the current scope
   - Examples:
     - `src/isMatchWith.rs:61`
71. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for struct `Rc<{closure@src/toolkit.rs:12:43: 12:72}>` in the current scope
   - Examples:
     - `src/toolkit.rs:16`
72. **error** `E0599` - 1 occurrence
   - Message: no method named `unwrap_or_else` found for unit type `()` in the current scope
   - Examples:
     - `src/flow_1.rs:75`
73. **error** `E0599` - 1 occurrence
   - Message: the method `contains_key` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5202`
74. **error** `E0599` - 1 occurrence
   - Message: the method `get` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5198`
75. **error** `E0599` - 1 occurrence
   - Message: the method `insert` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5206`
76. **error** `E0599` - 1 occurrence
   - Message: the method `remove` exists for struct `SmeltJsMap<T, std::string::String>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:5210`
77. **error** `E0600` - 1 occurrence
   - Message: cannot apply unary operator `-` to type `std::option::Option<f64>`
   - Examples:
     - `src/takeRight_1.rs:41`
78. **error** `E0609` - 1 occurrence
   - Message: no field `name` on type `HttpError`
   - Examples:
     - `src/main.rs:5276`
79. **error** `E0689` - 1 occurrence
   - Message: can't call method `max` on ambiguous numeric type `{float}`
   - Examples:
     - `src/invoke.rs:19`
80. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_2461` should have an upper camel case name
   - Examples:
     - `src/main.rs:4454`
81. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `cache_constructor` is never read
   - Examples:
     - `src/memoize_1.rs:140`
82. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `customizer_fn` is never read
   - Examples:
     - `src/setWith.rs:11`
83. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `i_1` is never read
   - Examples:
     - `src/filter.rs:81`
84. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key_1` is never read
   - Examples:
     - `src/some.rs:166`
85. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `length_1` is never read
   - Examples:
     - `src/filter.rs:80`
86. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `path` is never read
   - Examples:
     - `src/result.rs:38`
87. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `predicate_1` is never read
   - Examples:
     - `src/filter.rs:63`
88. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_1` is never read
   - Examples:
     - `src/filter.rs:77`
89. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `result_length` is never read
   - Examples:
     - `src/pullAllWith.rs:57`
90. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `set_low` is never read
   - Examples:
     - `src/sortedIndexBy.rs:99`
91. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `start` is never read
   - Examples:
     - `src/slice.rs:53`
92. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value_1` is never read
   - Examples:
     - `src/some.rs:168`
93. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/pullAllBy.rs:68`
94. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/now.rs:9`

## Cargo Stderr

```text
Updating crates.io index
     Locking 69 packages to latest Rust 1.96.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.18.0)
      Adding rand v0.9.4 (available: v0.10.2)
 Downloading crates ...
  Downloaded zerocopy v0.8.53
   Compiling proc-macro2 v1.0.106
   Compiling quote v1.0.46
   Compiling unicode-ident v1.0.24
   Compiling libc v0.2.186
   Compiling zerocopy v0.8.53
   Compiling getrandom v0.3.4
    Checking memchr v2.8.3
   Compiling autocfg v1.5.1
   Compiling num-traits v0.2.19
    Checking cfg-if v1.0.4
   Compiling serde_core v1.0.228
   Compiling syn v2.0.118
    Checking aho-corasick v1.1.4
    Checking regex-syntax v0.8.11
   Compiling zmij v1.0.21
    Checking siphasher v1.0.3
    Checking phf_shared v0.12.1
    Checking rand_core v0.9.5
    Checking regex-automata v0.4.14
    Checking tinyvec_macros v0.1.1
   Compiling serde_json v1.0.150
    Checking iana-time-zone v0.1.65
    Checking ppv-lite86 v0.2.21
   Compiling chrono-tz v0.10.4
   Compiling serde v1.0.228
    Checking bit-vec v0.8.0
    Checking bit-set v0.8.0
    Checking rand_chacha v0.9.0
    Checking chrono v0.4.45
   Compiling serde_derive v1.0.228
   Compiling tokio-macros v2.7.0
    Checking tinyvec v1.11.0
    Checking phf v0.12.1
    Checking itoa v1.0.18
    Checking pin-project-lite v0.2.17
    Checking tokio v1.52.3
    Checking unicode-normalization v0.1.25
    Checking regex v1.12.4
    Checking fancy-regex v0.14.0
    Checking rand v0.9.4
    Checking es_toolkit_probe v0.1.0 (/tmp/claude-0/-home-user-smelt/992beec0-8d53-5927-bcf1-98aff0398478/scratchpad/es-toolkit/dist-smelt)
error: could not compile `es_toolkit_probe` (bin "es_toolkit_probe") due to 525 previous errors; 387 warnings emitted
```
