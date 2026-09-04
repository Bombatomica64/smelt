# Generated Rust Test Report

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1049 passed; 10 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s`
- Failing tests: `10`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 1 | `__smelt_module_at_spec` |
| 1 | `__smelt_module_clone_spec` |
| 1 | `__smelt_module_isBrowser_spec` |
| 1 | `__smelt_module_isPlainObject_spec` |
| 1 | `__smelt_module_memoize_spec` |
| 1 | `__smelt_module_mergeWith_spec` |
| 1 | `__smelt_module_partialRight_spec` |
| 1 | `__smelt_module_partial_spec` |
| 1 | `__smelt_module_throttle_spec` |
| 1 | `__smelt_module_withTimeout_spec` |

### Delta From Baseline

- Baseline report: `/home/user/smelt/blocker-logs/estk-current.md`
- Resolved tests: `36`
- Newly failing tests: `1`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_at_spec::test_at_should_return_undefined_for_non_integer_indices`
- `__smelt_module_clone_spec::test_clone_should_clone_custom_error`
- `__smelt_module_isBrowser_spec::test_isbrowser_should_return_true_in_browser_environment`
- `__smelt_module_isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects`
- `__smelt_module_memoize_spec::test_memoize_should_use_this_context_for_resolver_function`
- `__smelt_module_mergeWith_spec::test_mergewith_should_respect_null_returned_from_customizer`
- `__smelt_module_partialRight_spec::test_partialright_partialright_ensures_new_par_is_an_instance_of_func`
- `__smelt_module_partial_spec::test_partial_partial_ensures_new_par_is_an_instance_of_func`
- `__smelt_module_throttle_spec::test_throttle_should_preserve_this_context_when_called_as_a_method`
- `__smelt_module_withTimeout_spec::test_withtimeout_lifts_the_time_limit_when_the_signal_is_aborted_resolving_with_the_run_result`

</details>
