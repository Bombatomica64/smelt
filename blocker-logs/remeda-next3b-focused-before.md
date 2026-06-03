# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_concat_test`: `failed` - `test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`

```text

running 2 tests
__smelt_module_concat_test::test_data_first_concat --- FAILED
__smelt_module_concat_test::test_data_last_concat --- FAILED

failures:

failures:
    __smelt_module_concat_test::test_data_first_concat
    __smelt_module_concat_test::test_data_last_concat

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_constant_test`: `failed` - `test result: FAILED. 4 passed; 3 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s`

```text

running 7 tests
... 3/7
__smelt_module_constant_test::test_can_completely_change_the_type_of_the_pipe --- FAILED
__smelt_module_constant_test::test_returns_identity_doesn_t_clone --- FAILED
. 6/7
__smelt_module_constant_test::test_can_be_put_in_a_pipe_144 --- FAILED

failures:

failures:
    __smelt_module_constant_test::test_can_be_put_in_a_pipe_144
    __smelt_module_constant_test::test_can_completely_change_the_type_of_the_pipe
    __smelt_module_constant_test::test_returns_identity_doesn_t_clone

test result: FAILED. 4 passed; 3 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `__smelt_module_debounce_test`: `failed` - `test result: FAILED. 19 passed; 4 failed; 0 ignored; 0 measured; 1766 filtered out; finished in 0.32s`

```text

running 23 tests
__smelt_module_debounce_test::test_additional_functionality_can_check_for_inflight_timers_leading_195 --- FAILED
... 4/23
__smelt_module_debounce_test::test_additional_functionality_can_check_for_inflight_timers_trailing_194 --- FAILED
.. 7/23
__smelt_module_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_196 --- FAILED
__smelt_module_debounce_test::test_additional_functionality_can_return_a_cached_value_193 --- FAILED
..............
failures:

failures:
    __smelt_module_debounce_test::test_additional_functionality_can_check_for_inflight_timers_leading_195
    __smelt_module_debounce_test::test_additional_functionality_can_check_for_inflight_timers_trailing_194
    __smelt_module_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_196
    __smelt_module_debounce_test::test_additional_functionality_can_return_a_cached_value_193

test result: FAILED. 19 passed; 4 failed; 0 ignored; 0 measured; 1766 filtered out; finished in 0.32s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_196' (75430) panicked at src/debounce_test.rs:854:49:
optional value was absent after narrowing
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_addProp_test`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`
- `__smelt_module_binarySearchCutoffIndex_test`: `passed` - `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1778 filtered out; finished in 0.00s`
- `__smelt_module_ceil_test`: `passed` - `test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 1768 filtered out; finished in 0.00s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `252`

## Summary By Code

1. **warning** `unused_parens` - 114 diagnostics
2. **warning** `unused_mut` - 100 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 100 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:105`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_parens` - 26 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:21`
     - `src/filter.rs:22`
     - `src/find.rs:22`
4. **warning** `unused_unsafe` - 23 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
     - `src/debounce.rs:266`
5. **warning** `unused_parens` - 15 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:86`
     - `src/allPass_test.rs:87`
     - `src/anyPass_test.rs:86`
     - `src/anyPass_test.rs:87`
     - `src/purryOrderRules.rs:148`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:111`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:96`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:92`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:77`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:109`
     - `src/funnel_lodash_debounce_test.rs:96`
     - `src/funnel_lodash_throttle_test.rs:90`
     - `src/funnel_lodash_throttle_test.rs:77`
8. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:47`
     - `src/toCamelCase.rs:79`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
10. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:670`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
18. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.02s
```
