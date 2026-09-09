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
     - `src/main.rs:11938`
     - `src/main.rs:11939`
     - `src/main.rs:11940`
     - `src/main.rs:12190`
     - `src/main.rs:12191`
2. **error** `E0308` - 173 occurrences
   - Message: mismatched types
   - Examples:
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/buffer.rs:327`
3. **error** `E0107` - 162 occurrences
   - Message: struct takes 0 generic arguments but 3 generic arguments were supplied
   - Examples:
     - `src/main.rs:6194`
     - `src/compose.rs:7`
     - `src/compose.rs:7`
     - `src/main.rs:8882`
     - `src/main.rs:11569`
4. **error** `E0107` - 53 occurrences
   - Message: struct takes 0 generic arguments but 1 generic argument was supplied
   - Examples:
     - `src/main.rs:6053`
     - `src/main.rs:6054`
     - `src/main.rs:6055`
     - `src/main.rs:6056`
     - `src/compose.rs:7`
5. **error** `E0599` - 20 occurrences
   - Message: no method named `_new_response` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:9129`
     - `src/main.rs:9138`
     - `src/main.rs:9185`
     - `src/main.rs:9203`
     - `src/main.rs:9225`
6. **error** `E0609` - 20 occurrences
   - Message: no field `finalized` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8956`
     - `src/main.rs:9172`
     - `src/main.rs:9212`
     - `src/main.rs:9256`
     - `src/main.rs:9296`
7. **error** `E0424` - 17 occurrences
   - Message: expected unit struct, unit variant or constant, found local variable `self`
   - Examples:
     - `src/main.rs:10842`
     - `src/main.rs:10859`
     - `src/main.rs:10895`
     - `src/main.rs:11022`
     - `src/main.rs:11039`
8. **error** `E0560` - 12 occurrences
   - Message: struct `ContextInner` has no field named `_smelt_phantom`
   - Examples:
     - `src/main.rs:8889`
     - `src/main.rs:10470`
     - `src/main.rs:10486`
     - `src/main.rs:10552`
     - `src/main.rs:10580`
9. **error** `E0609` - 12 occurrences
   - Message: no field `env` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
10. **error** `E0609` - 11 occurrences
   - Message: no field `new_response` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9583`
     - `src/main.rs:9592`
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
11. **error** `E0609` - 10 occurrences
   - Message: no field `header` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9577`
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
12. **error** `E0609` - 10 occurrences
   - Message: no field `html` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8899`
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
13. **error** `E0609` - 10 occurrences
   - Message: no field `text` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/hono_base.rs:9`
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
14. **error** `E0609` - 9 occurrences
   - Message: no field `body` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
15. **error** `E0609` - 9 occurrences
   - Message: no field `error` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
16. **error** `E0609` - 9 occurrences
   - Message: no field `get_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
17. **error** `E0609` - 9 occurrences
   - Message: no field `get` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
18. **error** `E0609` - 9 occurrences
   - Message: no field `json` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
19. **error** `E0609` - 9 occurrences
   - Message: no field `not_found` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
20. **error** `E0609` - 9 occurrences
   - Message: no field `redirect` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
21. **error** `E0609` - 9 occurrences
   - Message: no field `render` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
22. **error** `E0609` - 9 occurrences
   - Message: no field `set_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
23. **error** `E0609` - 9 occurrences
   - Message: no field `set_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
24. **error** `E0609` - 9 occurrences
   - Message: no field `set` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
25. **error** `E0609` - 9 occurrences
   - Message: no field `status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9624`
     - `src/main.rs:10589`
     - `src/main.rs:10682`
     - `src/main.rs:10695`
     - `src/main.rs:10736`
26. **error** `E0609` - 6 occurrences
   - Message: no field `_res` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8962`
     - `src/main.rs:8964`
     - `src/main.rs:8966`
     - `src/main.rs:8968`
     - `src/main.rs:9013`
27. **error** `E0609` - 6 occurrences
   - Message: no field `_var` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9078`
     - `src/main.rs:9080`
     - `src/main.rs:9083`
     - `src/main.rs:9103`
     - `src/main.rs:9105`
28. **error** `E0599` - 4 occurrences
   - Message: no method named `__smelt_get_res` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10689`
     - `src/main.rs:10743`
     - `src/main.rs:11458`
     - `src/main.rs:11530`
29. **error** `E0277` - 3 occurrences
   - Message: the trait bound `(SmeltUnknown, RouterRoute): SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6075`
     - `src/main.rs:10322`
     - `src/main.rs:47135`
30. **error** `E0382` - 3 occurrences
   - Message: use of moved value: `str`
   - Examples:
     - `src/html.rs:359`
     - `src/html.rs:370`
     - `src/html.rs:351`
31. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_execution_ctx` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10828`
     - `src/main.rs:11008`
     - `src/main.rs:11179`
32. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_req` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10940`
     - `src/main.rs:11120`
     - `src/main.rs:11291`
33. **error** `E0609` - 3 occurrences
   - Message: no field `_prepared_headers` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8991`
     - `src/main.rs:9038`
     - `src/main.rs:9161`
34. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for functions
   - Examples:
     - `src/request.rs:7`
     - `src/request.rs:7`
35. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for variants
   - Examples:
     - `src/main.rs:7382`
     - `src/main.rs:7382`
36. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:28169`
     - `src/main.rs:28172`
37. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
38. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
39. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
40. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
41. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
42. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
43. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
44. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
45. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
46. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
47. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
48. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
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
     - `src/main.rs:9609`
     - `src/main.rs:9624`
52. **error** `E0609` - 2 occurrences
   - Message: no field `_not_found_handler` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9608`
     - `src/main.rs:9610`
53. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8905`
     - `src/main.rs:8929`
54. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8904`
     - `src/main.rs:8906`
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
     - `src/main.rs:8891`
     - `src/main.rs:9622`
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
     - `src/main.rs:7394`
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
     - `src/main.rs:8153`
68. **error** `E0425` - 1 occurrence
   - Message: cannot find value `dispatch` in this scope
   - Examples:
     - `src/compose.rs:12`
69. **error** `E0599` - 1 occurrence
   - Message: no associated function or constant named `new` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11423`
70. **error** `E0599` - 1 occurrence
   - Message: the associated function or constant `default` exists for struct `Hono_1Inner<_, _, _, _>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6108`
71. **error** `E0599` - 1 occurrence
   - Message: the method `into_smelt_unknown` exists for struct `Router<(SmeltUnknown, RouterRoute)>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6132`
72. **error** `E0609` - 1 occurrence
   - Message: no field `_execution_ctx` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9620`
73. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8922`
74. **error** `E0609` - 1 occurrence
   - Message: no field `_match_result` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9628`
75. **error** `E0609` - 1 occurrence
   - Message: no field `_path` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9626`
76. **error** `E0609` - 1 occurrence
   - Message: no field `_raw_request` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9616`
77. **error** `E0609` - 1 occurrence
   - Message: no field `_res` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8965`
78. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9066`
79. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9163`
80. **error** `E0609` - 1 occurrence
   - Message: no field `_var` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9079`
81. **error** `E0609` - 1 occurrence
   - Message: no field `body` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9143`
82. **error** `E0609` - 1 occurrence
   - Message: no field `finalized` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8892`
83. **error** `E0609` - 1 occurrence
   - Message: no field `get_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8925`
84. **error** `E0609` - 1 occurrence
   - Message: no field `get` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9124`
85. **error** `E0609` - 1 occurrence
   - Message: no field `header` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9062`
86. **error** `E0609` - 1 occurrence
   - Message: no field `html` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9564`
87. **error** `E0609` - 1 occurrence
   - Message: no field `json` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9526`
88. **error** `E0609` - 1 occurrence
   - Message: no field `new_response` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9133`
89. **error** `E0609` - 1 occurrence
   - Message: no field `not_found` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9615`
90. **error** `E0609` - 1 occurrence
   - Message: no field `redirect` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9600`
91. **error** `E0609` - 1 occurrence
   - Message: no field `render` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8911`
92. **error** `E0609` - 1 occurrence
   - Message: no field `router` on type `Hono<E, S, BasePath>`
   - Examples:
     - `src/main.rs:47136`
93. **error** `E0609` - 1 occurrence
   - Message: no field `set_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8918`
94. **error** `E0609` - 1 occurrence
   - Message: no field `set_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8933`
95. **error** `E0609` - 1 occurrence
   - Message: no field `set` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9093`
96. **error** `E0609` - 1 occurrence
   - Message: no field `status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9070`
97. **error** `E0609` - 1 occurrence
   - Message: no field `text` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9510`
98. **error** `E0615` - 1 occurrence
   - Message: attempted to take value of method `build_all_matchers` on type `&__smelt_anon_class_3050<T>`
   - Examples:
     - `src/main.rs:30120`

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
