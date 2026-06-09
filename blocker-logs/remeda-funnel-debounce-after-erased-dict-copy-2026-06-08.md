# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `2`
- Guard runs: `3`
- Full suite executed: `true`

## Focused Runs

- `__smelt_module_funnel_lodash_debounce_test`: `failed` - `no test-result line`

```text
error[E0599]: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
    --> src/omit.rs:108:92
     |
 108 | ...t(map) => SmeltRecord::from_unknown_record(map), _ => Default::default() };
     |                           ^^^^^^^^^^^^^^^^^^^ function or associated item not found in `SmeltRecord<_, _>`
     |
    ::: src/main.rs:1206:1
     |
1206 | pub struct SmeltRecord<K, V> {
     | ---------------------------- function or associated item `from_unknown_record` not found for this struct
     |
note: if you're trying to build a new `SmeltRecord<_, _>` consider using one of the following associated functions:
      SmeltRecord::<K, V>::new
      SmeltRecord::<K, V>::with_id
      SmeltRecord::<K, V>::with_id_from_entries
    --> src/main.rs:1253:5
     |
1253 |     fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections:...
     |     ^^^^^^^^^^^^^^^^
1254 |     fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1255 |     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::r...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```
- `__smelt_module_randomBigInt_test`: `failed` - `no test-result line`

```text
error[E0599]: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
    --> src/omit.rs:108:92
     |
 108 | ...t(map) => SmeltRecord::from_unknown_record(map), _ => Default::default() };
     |                           ^^^^^^^^^^^^^^^^^^^ function or associated item not found in `SmeltRecord<_, _>`
     |
    ::: src/main.rs:1206:1
     |
1206 | pub struct SmeltRecord<K, V> {
     | ---------------------------- function or associated item `from_unknown_record` not found for this struct
     |
note: if you're trying to build a new `SmeltRecord<_, _>` consider using one of the following associated functions:
      SmeltRecord::<K, V>::new
      SmeltRecord::<K, V>::with_id
      SmeltRecord::<K, V>::with_id_from_entries
    --> src/main.rs:1253:5
     |
1253 |     fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections:...
     |     ^^^^^^^^^^^^^^^^
1254 |     fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1255 |     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::r...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```

## Regression Guards

- `__smelt_module_isError_test`: `failed` - `no test-result line`

```text
error[E0599]: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
    --> src/omit.rs:108:92
     |
 108 | ...t(map) => SmeltRecord::from_unknown_record(map), _ => Default::default() };
     |                           ^^^^^^^^^^^^^^^^^^^ function or associated item not found in `SmeltRecord<_, _>`
     |
    ::: src/main.rs:1206:1
     |
1206 | pub struct SmeltRecord<K, V> {
     | ---------------------------- function or associated item `from_unknown_record` not found for this struct
     |
note: if you're trying to build a new `SmeltRecord<_, _>` consider using one of the following associated functions:
      SmeltRecord::<K, V>::new
      SmeltRecord::<K, V>::with_id
      SmeltRecord::<K, V>::with_id_from_entries
    --> src/main.rs:1253:5
     |
1253 |     fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections:...
     |     ^^^^^^^^^^^^^^^^
1254 |     fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1255 |     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::r...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```
- `__smelt_module_isEmptyish_test`: `failed` - `no test-result line`

```text
error[E0599]: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
    --> src/omit.rs:108:92
     |
 108 | ...t(map) => SmeltRecord::from_unknown_record(map), _ => Default::default() };
     |                           ^^^^^^^^^^^^^^^^^^^ function or associated item not found in `SmeltRecord<_, _>`
     |
    ::: src/main.rs:1206:1
     |
1206 | pub struct SmeltRecord<K, V> {
     | ---------------------------- function or associated item `from_unknown_record` not found for this struct
     |
note: if you're trying to build a new `SmeltRecord<_, _>` consider using one of the following associated functions:
      SmeltRecord::<K, V>::new
      SmeltRecord::<K, V>::with_id
      SmeltRecord::<K, V>::with_id_from_entries
    --> src/main.rs:1253:5
     |
1253 |     fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections:...
     |     ^^^^^^^^^^^^^^^^
1254 |     fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1255 |     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::r...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```
- `__smelt_module_isEmpty_test`: `failed` - `no test-result line`

```text
error[E0599]: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
    --> src/omit.rs:108:92
     |
 108 | ...t(map) => SmeltRecord::from_unknown_record(map), _ => Default::default() };
     |                           ^^^^^^^^^^^^^^^^^^^ function or associated item not found in `SmeltRecord<_, _>`
     |
    ::: src/main.rs:1206:1
     |
1206 | pub struct SmeltRecord<K, V> {
     | ---------------------------- function or associated item `from_unknown_record` not found for this struct
     |
note: if you're trying to build a new `SmeltRecord<_, _>` consider using one of the following associated functions:
      SmeltRecord::<K, V>::new
      SmeltRecord::<K, V>::with_id
      SmeltRecord::<K, V>::with_id_from_entries
    --> src/main.rs:1253:5
     |
1253 |     fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections:...
     |     ^^^^^^^^^^^^^^^^
1254 |     fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1255 |     fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::r...
     |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-funnel-debounce-randombigint-before-2026-06-08.md`
- Resolved tests: `159`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `1`
- Warnings: `265`

## Summary By Code

1. **warning** `unused_mut` - 128 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 13 diagnostics
5. **error** `E0599` - 1 diagnostic
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 128 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
     - `src/clamp.rs:8`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_unsafe` - 31 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
4. **warning** `unused_parens` - 23 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:116`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:98`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:97`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:79`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
7. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
8. **error** `E0599` - 1 occurrence
   - Message: no function or associated item named `from_unknown_record` found for struct `SmeltRecord<K, V>` in the current scope
   - Examples:
     - `src/omit.rs:108`
9. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:37`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
16. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 1 previous error; 265 warnings emitted
```
