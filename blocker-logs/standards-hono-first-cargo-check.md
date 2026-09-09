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
     - `src/main.rs:12130`
     - `src/main.rs:12131`
     - `src/main.rs:12132`
     - `src/main.rs:12401`
     - `src/main.rs:12402`
2. **error** `E0107` - 164 occurrences
   - Message: struct takes 0 generic arguments but 3 generic arguments were supplied
   - Examples:
     - `src/main.rs:6194`
     - `src/compose.rs:7`
     - `src/compose.rs:7`
     - `src/main.rs:8896`
     - `src/main.rs:11744`
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
     - `src/main.rs:6053`
     - `src/main.rs:6054`
     - `src/main.rs:6055`
     - `src/main.rs:6056`
     - `src/compose.rs:7`
5. **error** `E0599` - 20 occurrences
   - Message: no method named `_new_response` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:9272`
     - `src/main.rs:9281`
     - `src/main.rs:9328`
     - `src/main.rs:9346`
     - `src/main.rs:9368`
6. **error** `E0609` - 20 occurrences
   - Message: no field `finalized` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8990`
     - `src/main.rs:9315`
     - `src/main.rs:9355`
     - `src/main.rs:9399`
     - `src/main.rs:9439`
7. **error** `E0560` - 12 occurrences
   - Message: struct `ContextInner` has no field named `_smelt_phantom`
   - Examples:
     - `src/main.rs:8903`
     - `src/main.rs:10650`
     - `src/main.rs:10666`
     - `src/main.rs:10732`
     - `src/main.rs:10760`
8. **error** `E0609` - 12 occurrences
   - Message: no field `env` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
9. **error** `E0609` - 11 occurrences
   - Message: no field `new_response` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9726`
     - `src/main.rs:9735`
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
10. **error** `E0609` - 10 occurrences
   - Message: no field `header` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9720`
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
11. **error** `E0609` - 10 occurrences
   - Message: no field `html` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8919`
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
12. **error** `E0609` - 10 occurrences
   - Message: no field `text` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/hono_base.rs:9`
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
13. **error** `E0609` - 9 occurrences
   - Message: no field `body` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
14. **error** `E0609` - 9 occurrences
   - Message: no field `error` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
15. **error** `E0609` - 9 occurrences
   - Message: no field `get_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
16. **error** `E0609` - 9 occurrences
   - Message: no field `get` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
17. **error** `E0609` - 9 occurrences
   - Message: no field `json` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
18. **error** `E0609` - 9 occurrences
   - Message: no field `not_found` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
19. **error** `E0609` - 9 occurrences
   - Message: no field `redirect` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
20. **error** `E0609` - 9 occurrences
   - Message: no field `render` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
21. **error** `E0609` - 9 occurrences
   - Message: no field `set_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
22. **error** `E0609` - 9 occurrences
   - Message: no field `set_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
23. **error** `E0609` - 9 occurrences
   - Message: no field `set` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
24. **error** `E0609` - 9 occurrences
   - Message: no field `status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9778`
     - `src/main.rs:10769`
     - `src/main.rs:10862`
     - `src/main.rs:10875`
     - `src/main.rs:10916`
25. **error** `E0609` - 8 occurrences
   - Message: no field `_var` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9205`
     - `src/main.rs:9210`
     - `src/main.rs:9213`
     - `src/main.rs:9222`
     - `src/main.rs:9225`
26. **error** `E0609` - 6 occurrences
   - Message: no field `_res` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8996`
     - `src/main.rs:8998`
     - `src/main.rs:9000`
     - `src/main.rs:9002`
     - `src/main.rs:9092`
27. **error** `E0609` - 5 occurrences
   - Message: no field `_prepared_headers` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9031`
     - `src/main.rs:9063`
     - `src/main.rs:9123`
     - `src/main.rs:9155`
     - `src/main.rs:9304`
28. **error** `E0599` - 4 occurrences
   - Message: no method named `__smelt_get_res` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:10869`
     - `src/main.rs:10923`
     - `src/main.rs:11633`
     - `src/main.rs:11705`
29. **error** `E0277` - 3 occurrences
   - Message: the trait bound `(SmeltUnknown, RouterRoute): SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6075`
     - `src/main.rs:10502`
     - `src/main.rs:49449`
30. **error** `E0382` - 3 occurrences
   - Message: use of moved value: `str`
   - Examples:
     - `src/html.rs:357`
     - `src/html.rs:368`
     - `src/html.rs:349`
31. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_execution_ctx` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11009`
     - `src/main.rs:11187`
     - `src/main.rs:11356`
32. **error** `E0599` - 3 occurrences
   - Message: no method named `__smelt_get_req` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11119`
     - `src/main.rs:11297`
     - `src/main.rs:11466`
33. **error** `E0609` - 3 occurrences
   - Message: no field `_not_found_handler` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9751`
     - `src/main.rs:9759`
     - `src/main.rs:9763`
34. **error** `E0609` - 3 occurrences
   - Message: no field `_renderer` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8914`
     - `src/main.rs:8926`
     - `src/main.rs:8930`
35. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for functions
   - Examples:
     - `src/request.rs:7`
     - `src/request.rs:7`
36. **error** `E0121` - 2 occurrences
   - Message: the placeholder `_` is not allowed within types on item signatures for variants
   - Examples:
     - `src/main.rs:7382`
     - `src/main.rs:7382`
37. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:29562`
     - `src/main.rs:29565`
38. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
39. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
40. **error** `E0277` - 2 occurrences
   - Message: the trait bound `BasePath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
41. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
42. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
43. **error** `E0277` - 2 occurrences
   - Message: the trait bound `CurrentPath: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
44. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
45. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
46. **error** `E0277` - 2 occurrences
   - Message: the trait bound `E: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
47. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: Clone` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
48. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
49. **error** `E0277` - 2 occurrences
   - Message: the trait bound `S: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:6082`
     - `src/main.rs:6083`
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
     - `src/main.rs:9758`
     - `src/main.rs:9778`
53. **error** `E0609` - 2 occurrences
   - Message: no field `_prepared_headers` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9035`
     - `src/main.rs:9127`
54. **error** `E0609` - 2 occurrences
   - Message: no field `_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8925`
     - `src/main.rs:8955`
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
     - `src/main.rs:8905`
     - `src/main.rs:9776`
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
65. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `r`
   - Examples:
     - `src/html.rs:63`
66. **error** `E0425` - 1 occurrence
   - Message: cannot find function `__smelt_fn_value_627` in this scope
   - Examples:
     - `src/main.rs:8153`
67. **error** `E0425` - 1 occurrence
   - Message: cannot find value `dispatch` in this scope
   - Examples:
     - `src/compose.rs:12`
68. **error** `E0599` - 1 occurrence
   - Message: no associated function or constant named `new` found for struct `Context` in the current scope
   - Examples:
     - `src/main.rs:11598`
69. **error** `E0599` - 1 occurrence
   - Message: the associated function or constant `default` exists for struct `Hono_1Inner<_, _, _, _>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6108`
70. **error** `E0599` - 1 occurrence
   - Message: the method `into_smelt_unknown` exists for struct `Router<(SmeltUnknown, RouterRoute)>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6132`
71. **error** `E0609` - 1 occurrence
   - Message: no field `_execution_ctx` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9774`
72. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8940`
73. **error** `E0609` - 1 occurrence
   - Message: no field `_layout` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8948`
74. **error** `E0609` - 1 occurrence
   - Message: no field `_match_result` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9782`
75. **error** `E0609` - 1 occurrence
   - Message: no field `_path` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9780`
76. **error** `E0609` - 1 occurrence
   - Message: no field `_raw_request` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9770`
77. **error** `E0609` - 1 occurrence
   - Message: no field `_res` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8999`
78. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9190`
79. **error** `E0609` - 1 occurrence
   - Message: no field `_status` on type `std::cell::Ref<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9306`
80. **error** `E0609` - 1 occurrence
   - Message: no field `_var` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9209`
81. **error** `E0609` - 1 occurrence
   - Message: no field `body` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9286`
82. **error** `E0609` - 1 occurrence
   - Message: no field `finalized` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8906`
83. **error** `E0609` - 1 occurrence
   - Message: no field `get_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8951`
84. **error** `E0609` - 1 occurrence
   - Message: no field `get` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9267`
85. **error** `E0609` - 1 occurrence
   - Message: no field `header` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9186`
86. **error** `E0609` - 1 occurrence
   - Message: no field `html` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9707`
87. **error** `E0609` - 1 occurrence
   - Message: no field `json` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9669`
88. **error** `E0609` - 1 occurrence
   - Message: no field `new_response` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9276`
89. **error** `E0609` - 1 occurrence
   - Message: no field `not_found` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9769`
90. **error** `E0609` - 1 occurrence
   - Message: no field `redirect` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9743`
91. **error** `E0609` - 1 occurrence
   - Message: no field `render` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8936`
92. **error** `E0609` - 1 occurrence
   - Message: no field `router` on type `Hono<E, S, BasePath>`
   - Examples:
     - `src/main.rs:49450`
93. **error** `E0609` - 1 occurrence
   - Message: no field `set_layout` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8944`
94. **error** `E0609` - 1 occurrence
   - Message: no field `set_renderer` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:8959`
95. **error** `E0609` - 1 occurrence
   - Message: no field `set` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9236`
96. **error** `E0609` - 1 occurrence
   - Message: no field `status` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9194`
97. **error** `E0609` - 1 occurrence
   - Message: no field `text` on type `RefMut<'_, ContextInner>`
   - Examples:
     - `src/main.rs:9653`

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
