# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_isShallowEqual_test`: `failed` - `test result: FAILED. 10 passed; 8 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s`

```text
    __smelt_module_isShallowEqual_test::test_built_ins_regex_1253
    __smelt_module_isShallowEqual_test::test_objects_arrays_1248
    __smelt_module_isShallowEqual_test::test_objects_sets_1252
    __smelt_module_isShallowEqual_test::test_objects_uint_arrays_1250
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays

test result: FAILED. 10 passed; 8 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s

Error: Custom { kind: Other
thread '__smelt_module_isShallowEqual_test::test_objects_arrays_1248' (145991) panicked at src/isShallowEqual.rs:126:26:
unknown is not object
, errornote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
: "expect(...).toBe(...) failed" }

thread '__smelt_module_isShallowEqual_test::test_objects_sets_1252' (145994) panicked at src/isShallowEqual.rs:126:26:
unknown is not object
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_isShallowEqual_test::test_objects_uint_arrays_1250' (145995) panicked at src/isShallowEqual.rs:126:26:
unknown is not object

thread '__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays' (146002) panicked at src/isShallowEqual.rs:126:26:
unknown is not object

thread '__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects' (146003) panicked at src/isShallowEqual.rs:126:26:
unknown is not object
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_isStrictEqual_test`: `failed` - `test result: FAILED. 12 passed; 5 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.00s`

```text

running 17 tests
__smelt_module_isStrictEqual_test::test_built_ins_promises_1273 --- FAILED
. 2/17
__smelt_module_isStrictEqual_test::test_objects_arrays_1266 --- FAILED
..... 8/17
__smelt_module_isStrictEqual_test::test_built_ins_regex_1271 --- FAILED
.... 13/17
__smelt_module_isStrictEqual_test::test_objects_uint_arrays_1268 --- FAILED
. 15/17
__smelt_module_isStrictEqual_test::test_objects_sets_1270 --- FAILED
.
failures:

failures:
    __smelt_module_isStrictEqual_test::test_built_ins_promises_1273
    __smelt_module_isStrictEqual_test::test_built_ins_regex_1271
    __smelt_module_isStrictEqual_test::test_objects_arrays_1266
    __smelt_module_isStrictEqual_test::test_objects_sets_1270
    __smelt_module_isStrictEqual_test::test_objects_uint_arrays_1268

test result: FAILED. 12 passed; 5 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_isDeepEqual_test`: `failed` - `test result: FAILED. 51 passed; 22 failed; 0 ignored; 0 measured; 1716 filtered out; finished in 0.01s`

```text
    __smelt_module_isDeepEqual_test::test_sets_two_empty_sets_should_be_equal
    __smelt_module_isDeepEqual_test::test_sets_two_sets_of_maps_with_the_same_values_should_be_equal
    __smelt_module_isDeepEqual_test::test_sets_two_sets_of_with_the_same_primitive_content_should_equal
    __smelt_module_isDeepEqual_test::test_sets_two_sets_with_more_than_two_items_that_are_all_equal_should_be_equal

test result: FAILED. 51 passed; 22 failed; 0 ignored; 0 measured; 1716 filtered out; finished in 0.01s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_setPath_test`: `passed` - `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1780 filtered out; finished in 0.01s`
- `__smelt_module_funnel_reference_batch_test`: `passed` - `test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1786 filtered out; finished in 0.00s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `275`

## Summary By Code

1. **warning** `unused_mut` - 136 diagnostics
2. **warning** `unused_parens` - 97 diagnostics
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
5. **warning** `unused_parens` - 6 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/toCamelCase.rs:66`
     - `src/toCamelCase.rs:150`
     - `src/uniqueWith.rs:28`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.38s
```
