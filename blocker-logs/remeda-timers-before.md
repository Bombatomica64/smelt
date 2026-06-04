# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `1`
- Full suite executed: `false`

## Focused Runs

- `debounce_test`: `failed` - `test result: FAILED. 30 passed; 32 failed; 0 ignored; 0 measured; 1727 filtered out; finished in 0.33s`

```text

thread '__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_queue_a_trailing_call_for_subsequent_debounced_calls_after_maxwait' (1897364) panicked at src/funnel_lodash_debounce_test.rs:27:81:
missing field

thread '__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_leading_option' (1897365) panicked at src/funnel_lodash_debounce_test.rs:32:70:
missing field

thread '__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_maxwait_option' (1897366) panicked at src/funnel_lodash_debounce_test.rs:27:81:
missing field

thread '__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_a_trailing_option' (1897367) panicked at src/funnel_lodash_debounce_test.rs:27:81:
missing field

thread '__smelt_module_funnel_lodash_debounce_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l4187_should_support_maxwait_in_a_tight_loop' (1897368) panicked at src/funnel_lodash_debounce_test.rs:27:81:
missing field
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_634' (1897376) panicked at src/funnel_remeda_debounce_test.rs:1040:49:
optional value was absent after narrowing
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `funnel_test`: `failed` - `test result: FAILED. 38 passed; 2 failed; 0 ignored; 0 measured; 1749 filtered out; finished in 0.01s`

```text

running 40 tests
.................................... 36/40
__smelt_module_funnel_test::test_utility_functions_isidle_isidle_reflects_the_funnel_s_state --- FAILED
__smelt_module_funnel_test::test_utility_functions_isidle_isidle_works_when_burst_duration_is_0 --- FAILED
..
failures:

failures:
    __smelt_module_funnel_test::test_utility_functions_isidle_isidle_reflects_the_funnel_s_state
    __smelt_module_funnel_test::test_utility_functions_isidle_isidle_works_when_burst_duration_is_0

test result: FAILED. 38 passed; 2 failed; 0 ignored; 0 measured; 1749 filtered out; finished in 0.01s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `funnel_remeda_debounce_test`: `failed` - `test result: FAILED. 11 passed; 12 failed; 0 ignored; 0 measured; 1766 filtered out; finished in 0.33s`

```text
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_cancel_before_the_timer_starts_627
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_cancel_the_timer_628
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_check_for_inflight_timers_leading_633
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_check_for_inflight_timers_trailing_632
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_after_a_cool_down_636
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_634
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_during_a_cool_down_635
    __smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_return_a_cached_value_631
    __smelt_module_funnel_remeda_debounce_test::test_main_functionality_should_debounce_a_function_615
    __smelt_module_funnel_remeda_debounce_test::test_main_functionality_subsequent_debounced_calls_return_the_last_func_result_616
    __smelt_module_funnel_remeda_debounce_test::test_main_functionality_subsequent_leading_debounced_calls_return_the_last_func_result_620

test result: FAILED. 11 passed; 12 failed; 0 ignored; 0 measured; 1766 filtered out; finished in 0.33s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }

thread '__smelt_module_funnel_remeda_debounce_test::test_additional_functionality_can_flush_before_a_cool_down_634' (1897466) panicked at src/funnel_remeda_debounce_test.rs:1040:49:
optional value was absent after narrowing
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `map_test`: `passed` - `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1777 filtered out; finished in 0.01s`

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `253`

## Summary By Code

1. **warning** `unused_parens` - 114 diagnostics
2. **warning** `unused_mut` - 101 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 101 occurrences
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.63s
```
