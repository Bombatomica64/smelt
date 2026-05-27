# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_funnel_lodash_throttle_test`: `failed` - `test result: FAILED. 0 passed; 17 failed; 0 ignored; 0 measured; 1772 filtered out; finished in 0.05s`

```text
thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_should_support_a_trailing_option_601' (2617512) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_noop_cancel_and_flush_when_nothing_is_queued_608' (2617518) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_should_trigger_a_call_when_invoked_repeatedly_and_leading_is_false' (2617516) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l22768_should_trigger_a_second_throttled_call_as_soon_as_possible' (2617517) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_cancelling_delayed_calls_605' (2617520) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_support_flushing_delayed_calls_607' (2617521) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_reset_lastcalled_after_cancelling_606' (2617519) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_should_use_a_default_wait_of_0_603' (2617522) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_not_tested_by_lodash_should_do_nothing_when_leading_and_trailing_are_both_disabled' (2617524) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field

thread '__smelt_module_funnel_lodash_throttle_test::test_https_github_com_lodash_lodash_blob_4_17_21_test_test_js_l23038_supports_recursive_calls_604' (2617523) panicked at src/funnel_lodash_throttle_test.rs:21:66:
missing field
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```

## Regression Guards

- `__smelt_module_truncate_test`: `passed` - `test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 1765 filtered out; finished in 0.03s`
- `__smelt_module_clone_test::test_objects_clones_objects_with_circular_references`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1788 filtered out; finished in 0.01s`
- `__smelt_module_difference_test`: `passed` - `test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1776 filtered out; finished in 0.01s`
