# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_test::test_parse_custom_locale_allows_to_pass_a_custom_locale`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.12s`

## Regression Guards

- `__smelt_module_test::test_parse_context_allows_to_specify_the_context`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.16s`

```text

running 1 test
__smelt_module_test::test_parse_context_allows_to_specify_the_context --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_context_allows_to_specify_the_context

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.16s

Error: Custom { kind: Other, error: "expect(...).toBe(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```
- `__smelt_module_test::test_parse_era_abbreviated`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.11s`
