# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `4`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_isStrictEqual_test`: `failed` - `test result: FAILED. 10 passed; 7 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.00s`

```text
__smelt_module_isStrictEqual_test::test_built_ins_dates_1272 --- FAILED
.. 5/17
__smelt_module_isStrictEqual_test::test_objects_objects_1267 --- FAILED
__smelt_module_isStrictEqual_test::test_objects_uint_arrays_1268 --- FAILED
... 10/17
__smelt_module_isStrictEqual_test::test_objects_sets_1270 --- FAILED
.... 15/17
__smelt_module_isStrictEqual_test::test_objects_maps_1269 --- FAILED
.
failures:

failures:
    __smelt_module_isStrictEqual_test::test_built_ins_dates_1272
    __smelt_module_isStrictEqual_test::test_built_ins_promises_1273
    __smelt_module_isStrictEqual_test::test_objects_arrays_1266
    __smelt_module_isStrictEqual_test::test_objects_maps_1269
    __smelt_module_isStrictEqual_test::test_objects_objects_1267
    __smelt_module_isStrictEqual_test::test_objects_sets_1270
    __smelt_module_isStrictEqual_test::test_objects_uint_arrays_1268

test result: FAILED. 10 passed; 7 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_isShallowEqual_test`: `failed` - `test result: FAILED. 12 passed; 6 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.02s`

```text
... 3/18
__smelt_module_isShallowEqual_test::test_built_ins_regex_1253 --- FAILED
__smelt_module_isShallowEqual_test::test_built_ins_dates_1254 --- FAILED
....... 12/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays --- FAILED
. 14/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays --- FAILED
__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects --- FAILED
. 17/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_objects --- FAILED

failures:

failures:
    __smelt_module_isShallowEqual_test::test_built_ins_dates_1254
    __smelt_module_isShallowEqual_test::test_built_ins_regex_1253
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_objects

test result: FAILED. 12 passed; 6 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.02s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_isDeepEqual_test`: `failed` - `test result: FAILED. 68 passed; 5 failed; 0 ignored; 0 measured; 1716 filtered out; finished in 0.02s`

```text

running 73 tests
................ 16/73
__smelt_module_isDeepEqual_test::test_functions_same_function_is_equal --- FAILED
......... 26/73
__smelt_module_isDeepEqual_test::test_objects_empty_array_and_empty_object_are_not_equal --- FAILED
........ 35/73
__smelt_module_isDeepEqual_test::test_null_prototype_objects_objects_with_different_non_null_prototypes_are_not_equal --- FAILED
. 37/73
__smelt_module_isDeepEqual_test::test_objects_null_and_undefined_are_not_equal --- FAILED
.................................. 72/73
__smelt_module_isDeepEqual_test::test_sample_objects_big_object --- FAILED

failures:

failures:
    __smelt_module_isDeepEqual_test::test_functions_same_function_is_equal
    __smelt_module_isDeepEqual_test::test_null_prototype_objects_objects_with_different_non_null_prototypes_are_not_equal
    __smelt_module_isDeepEqual_test::test_objects_empty_array_and_empty_object_are_not_equal
    __smelt_module_isDeepEqual_test::test_objects_null_and_undefined_are_not_equal
    __smelt_module_isDeepEqual_test::test_sample_objects_big_object

test result: FAILED. 68 passed; 5 failed; 0 ignored; 0 measured; 1716 filtered out; finished in 0.02s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_isIncludedIn_test`: `failed` - `test result: FAILED. 17 passed; 3 failed; 0 ignored; 0 measured; 1769 filtered out; finished in 0.00s`

```text

running 20 tests
. 1/20
__smelt_module_isIncludedIn_test::test_datafirst_only_tests_reference_equality_arrays --- FAILED
..... 7/20
__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_arrays --- FAILED
.......... 18/20
__smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_objects --- FAILED
.
failures:

failures:
    __smelt_module_isIncludedIn_test::test_datafirst_only_tests_reference_equality_arrays
    __smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_arrays
    __smelt_module_isIncludedIn_test::test_datalast_only_tests_reference_equality_objects

test result: FAILED. 17 passed; 3 failed; 0 ignored; 0 measured; 1769 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_split_test`: `passed` - `test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 1767 filtered out; finished in 0.01s`
- `__smelt_module_setPath_test`: `passed` - `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1780 filtered out; finished in 0.02s`
- `__smelt_module_flat_test`: `passed` - `test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 1761 filtered out; finished in 0.01s`

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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.75s
```
