# Generated Rust Test Report

- Cargo manifest: `third_party/es-toolkit/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `9`
- Full suite executed: `false`

## Focused Runs

- `isEqualWith_spec`: `passed` - `test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 1021 filtered out; finished in 0.01s`
- `isPlainObject_spec`: `failed` - `test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 1056 filtered out; finished in 0.00s`

```text

running 3 tests
__smelt_module_isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects --- FAILED
..
failures:

failures:
    __smelt_module_isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 1056 filtered out; finished in 0.00s

Error: SmeltThrown { value: String("expect(...).toBe(...) failed: expect(isPlainObject(runInNewContext('({})'))).toBe(true) (third_party/es-toolkit/src/predicate/isPlainObject.spec.ts:100:5)") }
error: test failed, to rerun pass `--bin es_toolkit_probe`
```
- `cloneDeep_spec`: `passed` - `test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 1022 filtered out; finished in 0.02s`

## Regression Guards

- `cloneDeepWith_spec`: `passed` - `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1048 filtered out; finished in 0.00s`
- `clone_spec`: `failed` - `test result: FAILED. 21 passed; 1 failed; 0 ignored; 0 measured; 1037 filtered out; finished in 0.01s`

```text

running 22 tests
....... 7/22
__smelt_module_clone_spec::test_clone_should_clone_custom_error --- FAILED
..............
failures:

failures:
    __smelt_module_clone_spec::test_clone_should_clone_custom_error

test result: FAILED. 21 passed; 1 failed; 0 ignored; 0 measured; 1037 filtered out; finished in 0.01s

Error: SmeltThrown { value: String("expect(...).toEqual(...) failed: expect(clonedError).toEqual(error) (third_party/es-toolkit/src/object/clone.spec.ts:257:5)") }
error: test failed, to rerun pass `--bin es_toolkit_probe`
```
- `isEqual_spec`: `passed` - `test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 1043 filtered out; finished in 0.01s`
- `object_create`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1057 filtered out; finished in 0.00s`
- `keys_spec`: `passed` - `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1059 filtered out; finished in 0.00s`
- `merge_spec`: `passed` - `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1048 filtered out; finished in 0.00s`
- `toSnakeCaseKeys_spec`: `passed` - `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1047 filtered out; finished in 0.05s`
- `invert_spec`: `passed` - `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1050 filtered out; finished in 0.01s`
- `isMatch_spec`: `passed` - `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1059 filtered out; finished in 0.00s`
