# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_split_test`: `failed` - `test result: FAILED. 16 passed; 6 failed; 0 ignored; 0 measured; 1767 filtered out; finished in 0.00s`

```text
__smelt_module_split_test::test_empty_string_separator --- FAILED
... 13/22
__smelt_module_split_test::test_multiple_types_of_separators --- FAILED
. 15/22
__smelt_module_split_test::test_negative_limit --- FAILED
......
failures:

failures:
    __smelt_module_split_test::test_datalast_limited_split
    __smelt_module_split_test::test_datalast_regex_with_limit
    __smelt_module_split_test::test_empty_string_empty_separator
    __smelt_module_split_test::test_empty_string_separator
    __smelt_module_split_test::test_multiple_types_of_separators
    __smelt_module_split_test::test_negative_limit

test result: FAILED. 16 passed; 6 failed; 0 ignored; 0 measured; 1767 filtered out; finished in 0.00s


thread '__smelt_module_split_test::test_datalast_limited_split' (151488) panicked at src/split_test.rs:328:414:
unknown is not iterable
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '__smelt_module_split_test::test_datalast_regex_with_limit' (151490) panicked at src/split_test.rs:349:414:
unknown is not iterable
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_setPath_test`: `passed` - `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1780 filtered out; finished in 0.01s`
- `__smelt_module_isShallowEqual_test`: `failed` - `test result: FAILED. 12 passed; 6 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s`

```text
. 1/18
__smelt_module_isShallowEqual_test::test_built_ins_regex_1253 --- FAILED
.. 4/18
__smelt_module_isShallowEqual_test::test_built_ins_dates_1254 --- FAILED
........ 13/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays --- FAILED
. 15/18
__smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects --- FAILED
__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays --- FAILED
__smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_objects --- FAILED

failures:

failures:
    __smelt_module_isShallowEqual_test::test_built_ins_dates_1254
    __smelt_module_isShallowEqual_test::test_built_ins_regex_1253
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_arrays_of_objects
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_arrays
    __smelt_module_isShallowEqual_test::test_shallow_inequality_objects_of_objects

test result: FAILED. 12 passed; 6 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.01s
```
