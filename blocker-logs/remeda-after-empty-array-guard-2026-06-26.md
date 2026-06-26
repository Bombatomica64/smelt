# Generated Rust Test Report

- Cargo manifest: `target/compat-repos/remeda/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1784 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.72s`
- Failing tests: `5`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 3 | `__smelt_module_funnel_reference_batch_test` |
| 1 | `__smelt_module_constant_test` |
| 1 | `__smelt_module_mapWithFeedback_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-current-state-2026-06-26.md`
- Resolved tests: `3`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_constant_test::test_returns_identity_doesn_t_clone`
- `__smelt_module_funnel_reference_batch_test::test_showcase_error_handling`
- `__smelt_module_funnel_reference_batch_test::test_showcase_results_as_array`
- `__smelt_module_funnel_reference_batch_test::test_showcase_results_as_object`
- `__smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator`

</details>
