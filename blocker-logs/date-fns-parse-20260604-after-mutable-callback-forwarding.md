# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_test::test_parse_era_abbreviated`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.04s`

```text

running 1 test
__smelt_module_test::test_parse_era_abbreviated --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_era_abbreviated

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.04s

Error: Custom { kind: Other, error: "expect(...).toEqual(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```

## Regression Guards

- `__smelt_module_test::test_parse_era_narrow`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s`

```text

running 1 test
__smelt_module_test::test_parse_era_narrow --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_era_narrow

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s

Error: Custom { kind: Other, error: "expect(...).toEqual(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```
- `__smelt_module_test::test_parse_era_with_week_numbering_year`: `failed` - `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s`

```text

running 1 test
Use `y` instead of `Y` (in `Y GGGGG`) for formatting years to the input `44 B`; see: https://github.com/date-fns/date-fns/blob/master/docs/unicodeTokens.md
__smelt_module_test::test_parse_era_with_week_numbering_year --- FAILED

failures:

failures:
    __smelt_module_test::test_parse_era_with_week_numbering_year

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.03s

Error: Custom { kind: Other, error: "expect(...).toEqual(...) failed" }
error: test failed, to rerun pass `--bin date_fns_parse_probe`
```
- `__smelt_module_test::test_parse_month_formatting_abbreviated`: `passed` - `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.06s`
