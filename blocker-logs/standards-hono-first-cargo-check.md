# Rust Diagnostics

- Cargo manifest: `/tmp/claude-0/hono26c/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `924`
- Warnings: `0`

## Summary By Code

1. **error** `E0609` - 428 diagnostics
2. **error** `E0107` - 215 diagnostics
3. **error** `E0308` - 173 diagnostics
4. **error** `E0599` - 33 diagnostics
5. **error** `E0277` - 29 diagnostics
6. **error** `E0424` - 17 diagnostics
7. **error** `E0560` - 12 diagnostics
8. **error** `E0382` - 8 diagnostics
9. **error** `E0121` - 4 diagnostics
10. **error** `E0425` - 2 diagnostics
11. **error** `E0063` - 1 diagnostic
12. **error** `E0283` - 1 diagnostic
13. **error** `E0615` - 1 diagnostic

## Groups

1. **error** `E0609` - 180 occurrences
   - Message: no field `var_index` on type `Context`
   - Examples:
     - `src/main.rs:11683`
     - `src/main.rs:11684`
     - `src/main.rs:11685`
     - `src/main.rs:11935`
     - `src/main.rs:11936`
2. **error** `E0308` - 173 occurrences
   - Message: mismatched types
   - Examples:
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/buffer.rs:333`
3. **error** `E0107` - 162 occurrences
   - Message: struct takes 0 generic arguments but 3 generic arguments were supplied
   - Examples:
     - `src/main.rs:6086`
     - `src/compose.rs:7`
     - `src/compose.rs:7`
     - `src/main.rs:8627`
     - `src/main.rs:11314`
4. **error** `E0107` - 53 occurrences
   - Message: struct takes 0 generic arguments but 1 generic argument was supplied
   - Examples:
     - `src/main.rs:5945`
     - `src/main.rs:5946`
     - `src/main.rs:5947`
     - `src/main.rs:5948`
     - `src/compose.rs:7`
5. **error** `E0599` - 20 occurrences
   - Message: no method named `_new_response` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:8874`
     - `src/main.rs:8883`
     - `src/main.rs:8930`
     - `src/main.rs:8948`
     - `src/main.rs:8970`
6. **error** `E0609` - 20 occurrences
   - Message: no field `finalized` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8701`
     - `src/main.rs:8917`
     - `src/main.rs:8957`
     - `src/main.rs:9001`
     - `src/main.rs:9041`
7. **error** `E0424` - 17 occurrences
   - Message: expected unit struct, unit variant or constant, found local variable `self`
   - Examples:
     - `src/main.rs:10587`
     - `src/main.rs:10604`
     - `src/main.rs:10640`
     - `src/main.rs:10767`
     - `src/main.rs:10784`
8. **error** `E0560` - 12 occurrences
   - Message: struct `ContextInner` has no field named `_smelt_phantom`
   - Examples:
     - `src/main.rs:8634`
     - `src/main.rs:10215`
     - `src/main.rs:10231`
     - `src/main.rs:10297`
     - `src/main.rs:10325`
9. **error** `E0609` - 12 occurrences
   - Message: no field `env` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
10. **error** `E0609` - 11 occurrences
   - Message: no field `new_response` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9328`
     - `src/main.rs:9337`
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
11. **error** `E0609` - 10 occurrences
   - Message: no field `header` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9322`
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
12. **error** `E0609` - 10 occurrences
   - Message: no field `html` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8644`
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
13. **error** `E0609` - 10 occurrences
   - Message: no field `text` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/hono_base.rs:9`
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
14. **error** `E0609` - 9 occurrences
   - Message: no field `body` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
15. **error** `E0609` - 9 occurrences
   - Message: no field `error` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
16. **error** `E0609` - 9 occurrences
   - Message: no field `get_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
17. **error** `E0609` - 9 occurrences
   - Message: no field `get` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
18. **error** `E0609` - 9 occurrences
   - Message: no field `json` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
19. **error** `E0609` - 9 occurrences
   - Message: no field `not_found` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
20. **error** `E0609` - 9 occurrences
   - Message: no field `redirect` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
21. **error** `E0609` - 9 occurrences
   - Message: no field `render` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
22. **error** `E0609` - 9 occurrences
   - Message: no field `set_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
23. **error** `E0609` - 9 occurrences
   - Message: no field `set_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
24. **error** `E0609` - 9 occurrences
   - Message: no field `set` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
25. **error** `E0609` - 9 occurrences
   - Message: no field `status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9369`
     - `src/main.rs:10334`
     - `src/main.rs:10427`
     - `src/main.rs:10440`
     - `src/main.rs:10481`
26. **error** `E0609` - 6 occurrences
   - Message: no field `_res` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8707`
     - `src/main.rs:8709`
     - `src/main.rs:8711`
     - `src/main.rs:8713`
     - `src/main.rs:8758`
27. **error** `E0609` - 6 occurrences
   - Message: no field `_var` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8823`
     - `src/main.rs:8825`
     - `src/main.rs:8828`
     - `src/main.rs:8848`
     - `src/main.rs:8850`
28. **error** `E0599` - 4 occurrences
   - Message: no method named `__smelt_get_res` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10434`
     - `src/main.rs:10488`
     - `src/main.rs:11203`
     - `src/main.rs:11275`
29. **error** `E0277` - 3 occurrences
   - Message: the trait bound `(SmeltUnknown, RouterRoute): SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5967`
     - `src/main.rs:10067`
     - `src/main.rs:46880`
30. **error** `E0382` - 3 occurrences
   - Message: use of moved value: `str`
   - Examples:
     - `src/html.rs:359`
     - `src/html.rs:370`
     - `src/html.rs:351`
31. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_execution_ctx` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10573`
     - `src/main.rs:10753`
     - `src/main.rs:10924`
32. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_req` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10685`
     - `src/main.rs:10865`
     - `src/main.rs:11036`
33. **error** `E0609` - 3 occurrences
   - Message: no field `_prepared_headers` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8736`
     - `src/main.rs:8783`
     - `src/main.rs:8906`
34. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for functions
   - Examples:
     - `src/request.rs:7`
     - `src/request.rs:7`
35. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for variants
   - Examples:
     - `src/main.rs:7127`
     - `src/main.rs:7127`
36. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:27914`
     - `src/main.rs:27917`
37. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
38. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
39. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
40. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
41. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
42. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
43. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: Clone` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
44. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
45. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
46. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: Clone` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
47. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
48. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5974`
     - `src/main.rs:5975`
49. **error** `E0382` - 2 occurrences
   - Message: borrow of moved value
   - Examples:
     - `src/html.rs:380`
     - `src/html.rs:482`
50. **error** `E0382` - 2 occurrences
   - Message: use of moved value
   - Examples:
     - `src/html.rs:97`
     - `src/html.rs:127`
51. **error** `E0609` - 2 occurrences
   - Message: no field `_not_found_handler` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9354`
     - `src/main.rs:9369`
52. **error** `E0609` - 2 occurrences
   - Message: no field `_not_found_handler` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9353`
     - `src/main.rs:9355`
53. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8650`
     - `src/main.rs:8674`
54. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8649`
     - `src/main.rs:8651`
55. **error** `E0609` - 2 occurrences
   - Message: no field `cache` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
56. **error** `E0609` - 2 occurrences
   - Message: no field `credentials` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
57. **error** `E0609` - 2 occurrences
   - Message: no field `env` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8636`
     - `src/main.rs:9367`
58. **error** `E0609` - 2 occurrences
   - Message: no field `integrity` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
59. **error** `E0609` - 2 occurrences
   - Message: no field `keepalive` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
60. **error** `E0609` - 2 occurrences
   - Message: no field `mode` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
61. **error** `E0609` - 2 occurrences
   - Message: no field `redirect` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
62. **error** `E0609` - 2 occurrences
   - Message: no field `referrer_policy` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
63. **error** `E0609` - 2 occurrences
   - Message: no field `referrer` on type `SmeltRequest`
   - Examples:
     - `src/request.rs:72`
     - `src/request.rs:94`
64. **error** `E0063` - 1 occurrence
   - Message: missing field `_smelt_phantom` in initializer of `HonoRequestInner<_, _>`
   - Examples:
     - `src/main.rs:7139`
65. **error** `E0283` - 1 occurrence
   - Message: type annotations needed for `__smelt_anon_class_3050<_>`
   - Examples:
     - `src/prepared_router.rs:120`
66. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `r`
   - Examples:
     - `src/html.rs:65`
67. **error** `E0425` - 1 occurrence
   - Message: cannot find function `__smelt_fn_value_627` in this scope
   - Examples:
     - `src/main.rs:7898`
68. **error** `E0425` - 1 occurrence
   - Message: cannot find value `dispatch` in this scope
   - Examples:
     - `src/compose.rs:12`
69. **error** `E0599` - 1 occurrence
   - Message: no associated function or constant named `new` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11168`
70. **error** `E0599` - 1 occurrence
   - Message: the associated function or constant `default` exists for struct `Hono_1Inner<_, _, _, _>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6000`
71. **error** `E0599` - 1 occurrence
   - Message: the method `into_smelt_unknown` exists for struct `Router<(SmeltUnknown, RouterRoute)>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6024`
72. **error** `E0609` - 1 occurrence
   - Message: no field `_execution_ctx` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9365`
73. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8667`
74. **error** `E0609` - 1 occurrence
   - Message: no field `_match_result` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9373`
75. **error** `E0609` - 1 occurrence
   - Message: no field `_path` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9371`
76. **error** `E0609` - 1 occurrence
   - Message: no field `_raw_request` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9361`
77. **error** `E0609` - 1 occurrence
   - Message: no field `_res` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8710`
78. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8811`
79. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8908`
80. **error** `E0609` - 1 occurrence
   - Message: no field `_var` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8824`
81. **error** `E0609` - 1 occurrence
   - Message: no field `body` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8888`
82. **error** `E0609` - 1 occurrence
   - Message: no field `finalized` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8637`
83. **error** `E0609` - 1 occurrence
   - Message: no field `get_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8670`
84. **error** `E0609` - 1 occurrence
   - Message: no field `get` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8869`
85. **error** `E0609` - 1 occurrence
   - Message: no field `header` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8807`
86. **error** `E0609` - 1 occurrence
   - Message: no field `html` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9309`
87. **error** `E0609` - 1 occurrence
   - Message: no field `json` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9271`
88. **error** `E0609` - 1 occurrence
   - Message: no field `new_response` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8878`
89. **error** `E0609` - 1 occurrence
   - Message: no field `not_found` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9360`
90. **error** `E0609` - 1 occurrence
   - Message: no field `redirect` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9345`
91. **error** `E0609` - 1 occurrence
   - Message: no field `render` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8656`
92. **error** `E0609` - 1 occurrence
   - Message: no field `router` on type `Hono<E, S, BasePath>`
   - Examples:
     - `src/main.rs:46881`
93. **error** `E0609` - 1 occurrence
   - Message: no field `set_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8663`
94. **error** `E0609` - 1 occurrence
   - Message: no field `set_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8678`
95. **error** `E0609` - 1 occurrence
   - Message: no field `set` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8838`
96. **error** `E0609` - 1 occurrence
   - Message: no field `status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8815`
97. **error** `E0609` - 1 occurrence
   - Message: no field `text` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9255`
98. **error** `E0615` - 1 occurrence
   - Message: attempted to take value of method `build_all_matchers` on type `&__smelt_anon_class_3050<T>`
   - Examples:
     - `src/main.rs:29865`

## Cargo Stderr

```text
Updating crates.io index
     Locking 98 packages to latest Rust 1.96.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.19.1)
      Adding generic-array v0.14.7 (available: v0.14.9)
      Adding sha1 v0.10.7 (available: v0.11.0)
      Adding sha2 v0.10.9 (available: v0.11.0)
    Checking hono_probe v0.1.0 (/tmp/claude-0/hono26c/dist-smelt)
error: could not compile `hono_probe` (bin "hono_probe") due to 924 previous errors
```
