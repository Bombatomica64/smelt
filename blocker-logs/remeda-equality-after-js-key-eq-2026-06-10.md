# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `4`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_isStrictEqual_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```
- `__smelt_module_isShallowEqual_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```
- `__smelt_module_isDeepEqual_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```
- `__smelt_module_isIncludedIn_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```

## Regression Guards

- `__smelt_module_split_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```
- `__smelt_module_setPath_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```
- `__smelt_module_flat_test`: `failed` - `no test-result line`

```text
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/range_test.rs:1435:193
     |
1435 | ...hs = _smelt_tmp_47.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `same_js_key` found for struct `Vec<SmeltUnknown>` in the current scope
    --> src/splice_test.rs:177:190
     |
 177 | ...rhs = _smelt_tmp_4.clone(); smelt_lhs.same_js_key(&smelt_rhs) });
     |                                          ^^^^^^^^^^^ method not found in `Vec<SmeltUnknown>`
     |
     = help: items from traits can only be used if the trait is implemented and in scope
note: `SmeltJsKeyEq` defines an item `same_js_key`, perhaps you need to implement it
    --> src/main.rs:1338:1
     |
1338 | pub trait SmeltJsKeyEq {
     | ^^^^^^^^^^^^^^^^^^^^^^

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 13 previous errors
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `271`

## Summary By Code

1. **warning** `unused_mut` - 136 diagnostics
2. **warning** `unused_parens` - 93 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic
6. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 136 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
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
4. **warning** `unused_parens` - 24 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
6. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
7. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
13. **warning** `unused_must_use` - 1 occurrence
   - Message: unused `Result` that must be used
   - Examples:
     - `src/funnel_reference_batch_test.rs:159`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.95s
```
