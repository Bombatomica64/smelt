# Generated Rust Test Report

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 1055 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s`
- Failing tests: `4`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 1 | `__smelt_module_at_spec` |
| 1 | `__smelt_module_isBrowser_spec` |
| 1 | `__smelt_module_isPlainObject_spec` |
| 1 | `__smelt_module_mergeWith_spec` |

### Delta From Baseline

- Baseline report: `blocker-logs/estk-current.md`
- Resolved tests: `41`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_at_spec::test_at_should_return_undefined_for_non_integer_indices`
- `__smelt_module_isBrowser_spec::test_isbrowser_should_return_true_in_browser_environment`
- `__smelt_module_isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects`
- `__smelt_module_mergeWith_spec::test_mergewith_should_respect_null_returned_from_customizer`

</details>
