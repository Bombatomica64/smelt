# Generated Rust Test Report

- Cargo manifest: `/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `funnel_reference_batch_test`: `failed` - `test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 1786 filtered out; finished in 0.00s`

```text

running 3 tests
__smelt_module_funnel_reference_batch_test::test_showcase_error_handling --- FAILED
__smelt_module_funnel_reference_batch_test::test_showcase_results_as_array --- FAILED
__smelt_module_funnel_reference_batch_test::test_showcase_results_as_object --- FAILED

failures:

failures:
    __smelt_module_funnel_reference_batch_test::test_showcase_error_handling
    __smelt_module_funnel_reference_batch_test::test_showcase_results_as_array
    __smelt_module_funnel_reference_batch_test::test_showcase_results_as_object

test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 1786 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).rejects.toThrow(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `isStrictEqual_test`: `passed` - `test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.00s`
- `isShallowEqual_test`: `failed` - `test result: FAILED. 15 passed; 3 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s`

```text

running 18 tests
__smelt_module_isShallowEqual_test::test_built_ins_promises_1255 --- FAILED
............. 14/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays --- FAILED
. 16/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays --- FAILED
.
failures:

failures:
    __smelt_module_isShallowEqual_test::test_built_ins_promises_1255
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays

test result: FAILED. 15 passed; 3 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `274`

## Summary By Code

1. **warning** `unused_mut` - 140 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic
6. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 140 occurrences
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
6. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:108`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/isShallowEqual.rs:259`
12. **warning** `unused_must_use` - 1 occurrence
   - Message: unused `Result` that must be used
   - Examples:
     - `src/funnel_reference_batch_test.rs:159`
13. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.48s
```
