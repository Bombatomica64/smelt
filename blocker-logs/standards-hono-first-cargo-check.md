# Rust Diagnostics

- Cargo manifest: `/tmp/claude-0/hono26c/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `883`
- Warnings: `0`

## Summary By Code

1. **error** `E0609` - 437 diagnostics
2. **error** `E0107` - 222 diagnostics
3. **error** `E0308` - 135 diagnostics
4. **error** `E0599` - 33 diagnostics
5. **error** `E0277` - 29 diagnostics
6. **error** `E0560` - 12 diagnostics
7. **error** `E0382` - 8 diagnostics
8. **error** `E0121` - 4 diagnostics
9. **error** `E0425` - 2 diagnostics
10. **error** `E0063` - 1 diagnostic

## Groups

1. **error** `E0609` - 180 occurrences
   - Message: no field `var_index` on type `Context`
   - Examples:
     - `src/main.rs:12167`
     - `src/main.rs:12168`
     - `src/main.rs:12169`
     - `src/main.rs:12438`
     - `src/main.rs:12439`
2. **error** `E0107` - 164 occurrences
   - Message: struct takes 0 generic arguments but 3 generic arguments were supplied
   - Examples:
     - `src/main.rs:6231`
     - `src/compose.rs:7`
     - `src/compose.rs:7`
     - `src/main.rs:8933`
     - `src/main.rs:11781`
3. **error** `E0308` - 135 occurrences
   - Message: mismatched types
   - Examples:
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/body.rs:77`
     - `src/buffer.rs:327`
4. **error** `E0107` - 58 occurrences
   - Message: struct takes 0 generic arguments but 1 generic argument was supplied
   - Examples:
     - `src/main.rs:6090`
     - `src/main.rs:6091`
     - `src/main.rs:6092`
     - `src/main.rs:6093`
     - `src/compose.rs:7`
5. **error** `E0599` - 20 occurrences
   - Message: no method named `_new_response` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:9309`
     - `src/main.rs:9318`
     - `src/main.rs:9365`
     - `src/main.rs:9383`
     - `src/main.rs:9405`
6. **error** `E0609` - 20 occurrences
   - Message: no field `finalized` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9027`
     - `src/main.rs:9352`
     - `src/main.rs:9392`
     - `src/main.rs:9436`
     - `src/main.rs:9476`
7. **error** `E0560` - 12 occurrences
   - Message: struct `ContextInner` has no field named `_smelt_phantom`
   - Examples:
     - `src/main.rs:8940`
     - `src/main.rs:10687`
     - `src/main.rs:10703`
     - `src/main.rs:10769`
     - `src/main.rs:10797`
8. **error** `E0609` - 12 occurrences
   - Message: no field `env` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
9. **error** `E0609` - 11 occurrences
   - Message: no field `new_response` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9763`
     - `src/main.rs:9772`
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
10. **error** `E0609` - 10 occurrences
   - Message: no field `header` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9757`
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
11. **error** `E0609` - 10 occurrences
   - Message: no field `html` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8956`
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
12. **error** `E0609` - 10 occurrences
   - Message: no field `text` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/hono_base.rs:9`
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
13. **error** `E0609` - 9 occurrences
   - Message: no field `body` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
14. **error** `E0609` - 9 occurrences
   - Message: no field `error` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
15. **error** `E0609` - 9 occurrences
   - Message: no field `get_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
16. **error** `E0609` - 9 occurrences
   - Message: no field `get` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
17. **error** `E0609` - 9 occurrences
   - Message: no field `json` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
18. **error** `E0609` - 9 occurrences
   - Message: no field `not_found` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
19. **error** `E0609` - 9 occurrences
   - Message: no field `redirect` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
20. **error** `E0609` - 9 occurrences
   - Message: no field `render` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
21. **error** `E0609` - 9 occurrences
   - Message: no field `set_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
22. **error** `E0609` - 9 occurrences
   - Message: no field `set_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
23. **error** `E0609` - 9 occurrences
   - Message: no field `set` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
24. **error** `E0609` - 9 occurrences
   - Message: no field `status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9815`
     - `src/main.rs:10806`
     - `src/main.rs:10899`
     - `src/main.rs:10912`
     - `src/main.rs:10953`
25. **error** `E0609` - 8 occurrences
   - Message: no field `_var` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9242`
     - `src/main.rs:9247`
     - `src/main.rs:9250`
     - `src/main.rs:9259`
     - `src/main.rs:9262`
26. **error** `E0609` - 6 occurrences
   - Message: no field `_res` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9033`
     - `src/main.rs:9035`
     - `src/main.rs:9037`
     - `src/main.rs:9039`
     - `src/main.rs:9129`
27. **error** `E0609` - 5 occurrences
   - Message: no field `_prepared_headers` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9068`
     - `src/main.rs:9100`
     - `src/main.rs:9160`
     - `src/main.rs:9192`
     - `src/main.rs:9341`
28. **error** `E0599` - 4 occurrences
   - Message: no method named `__smelt_get_res` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10906`
     - `src/main.rs:10960`
     - `src/main.rs:11670`
     - `src/main.rs:11742`
29. **error** `E0277` - 3 occurrences
   - Message: the trait bound `(SmeltUnknown, RouterRoute): SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6112`
     - `src/main.rs:10539`
     - `src/main.rs:49486`
30. **error** `E0382` - 3 occurrences
   - Message: use of moved value: `str`
   - Examples:
     - `src/html.rs:357`
     - `src/html.rs:368`
     - `src/html.rs:349`
31. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_execution_ctx` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11046`
     - `src/main.rs:11224`
     - `src/main.rs:11393`
32. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_req` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11156`
     - `src/main.rs:11334`
     - `src/main.rs:11503`
33. **error** `E0609` - 3 occurrences
   - Message: no field `_not_found_handler` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9788`
     - `src/main.rs:9796`
     - `src/main.rs:9800`
34. **error** `E0609` - 3 occurrences
   - Message: no field `_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8951`
     - `src/main.rs:8963`
     - `src/main.rs:8967`
35. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for functions
   - Examples:
     - `src/request.rs:7`
     - `src/request.rs:7`
36. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for variants
   - Examples:
     - `src/main.rs:7419`
     - `src/main.rs:7419`
37. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:29599`
     - `src/main.rs:29602`
38. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
39. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
40. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
41. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
42. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
43. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
44. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
45. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
46. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
47. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
48. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
49. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6119`
     - `src/main.rs:6120`
50. **error** `E0382` - 2 occurrences
   - Message: borrow of moved value
   - Examples:
     - `src/html.rs:378`
     - `src/html.rs:480`
51. **error** `E0382` - 2 occurrences
   - Message: use of moved value
   - Examples:
     - `src/html.rs:95`
     - `src/html.rs:125`
52. **error** `E0609` - 2 occurrences
   - Message: no field `_not_found_handler` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9795`
     - `src/main.rs:9815`
53. **error** `E0609` - 2 occurrences
   - Message: no field `_prepared_headers` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9072`
     - `src/main.rs:9164`
54. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8962`
     - `src/main.rs:8992`
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
     - `src/main.rs:8942`
     - `src/main.rs:9813`
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
     - `src/main.rs:7431`
65. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `r`
   - Examples:
     - `src/html.rs:63`
66. **error** `E0425` - 1 occurrence
   - Message: cannot find function `__smelt_fn_value_627` in this scope
   - Examples:
     - `src/main.rs:8190`
67. **error** `E0425` - 1 occurrence
   - Message: cannot find value `dispatch` in this scope
   - Examples:
     - `src/compose.rs:12`
68. **error** `E0599` - 1 occurrence
   - Message: no associated function or constant named `new` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11635`
69. **error** `E0599` - 1 occurrence
   - Message: the associated function or constant `default` exists for struct `Hono_1Inner<_, _, _, _>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6145`
70. **error** `E0599` - 1 occurrence
   - Message: the method `into_smelt_unknown` exists for struct `Router<(SmeltUnknown, RouterRoute)>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6169`
71. **error** `E0609` - 1 occurrence
   - Message: no field `_execution_ctx` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9811`
72. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8977`
73. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8985`
74. **error** `E0609` - 1 occurrence
   - Message: no field `_match_result` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9819`
75. **error** `E0609` - 1 occurrence
   - Message: no field `_path` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9817`
76. **error** `E0609` - 1 occurrence
   - Message: no field `_raw_request` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9807`
77. **error** `E0609` - 1 occurrence
   - Message: no field `_res` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9036`
78. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9227`
79. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9343`
80. **error** `E0609` - 1 occurrence
   - Message: no field `_var` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9246`
81. **error** `E0609` - 1 occurrence
   - Message: no field `body` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9323`
82. **error** `E0609` - 1 occurrence
   - Message: no field `finalized` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8943`
83. **error** `E0609` - 1 occurrence
   - Message: no field `get_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8988`
84. **error** `E0609` - 1 occurrence
   - Message: no field `get` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9304`
85. **error** `E0609` - 1 occurrence
   - Message: no field `header` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9223`
86. **error** `E0609` - 1 occurrence
   - Message: no field `html` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9744`
87. **error** `E0609` - 1 occurrence
   - Message: no field `json` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9706`
88. **error** `E0609` - 1 occurrence
   - Message: no field `new_response` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9313`
89. **error** `E0609` - 1 occurrence
   - Message: no field `not_found` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9806`
90. **error** `E0609` - 1 occurrence
   - Message: no field `redirect` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9780`
91. **error** `E0609` - 1 occurrence
   - Message: no field `render` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8973`
92. **error** `E0609` - 1 occurrence
   - Message: no field `router` on type `Hono<E, S, BasePath>`
   - Examples:
     - `src/main.rs:49487`
93. **error** `E0609` - 1 occurrence
   - Message: no field `set_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8981`
94. **error** `E0609` - 1 occurrence
   - Message: no field `set_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8996`
95. **error** `E0609` - 1 occurrence
   - Message: no field `set` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9273`
96. **error** `E0609` - 1 occurrence
   - Message: no field `status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9231`
97. **error** `E0609` - 1 occurrence
   - Message: no field `text` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9690`

## Cargo Stderr

```text
Updating crates.io index
     Locking 98 packages to latest Rust 1.96.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.19.1)
      Adding generic-array v0.14.7 (available: v0.14.9)
      Adding sha1 v0.10.7 (available: v0.11.0)
      Adding sha2 v0.10.9 (available: v0.11.0)
    Checking hono_probe v0.1.0 (/tmp/claude-0/hono26c/dist-smelt)
error: could not compile `hono_probe` (bin "hono_probe") due to 883 previous errors
```
