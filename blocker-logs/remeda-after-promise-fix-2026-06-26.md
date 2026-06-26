# Generated Rust Test Report

- Cargo manifest: `target/compat-repos/remeda/dist-smelt/Cargo.toml`
- Focused runs: `5`
- Guard runs: `0`
- Full suite executed: `false`

## Focused Runs

- `isPromise_test`: `passed` - `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1787 filtered out; finished in 0.00s`
- `isShallowEqual_test`: `passed` - `test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 1771 filtered out; finished in 0.00s`
- `sortBy_test`: `failed` - `test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 1779 filtered out; finished in 0.15s`

```text

running 10 tests
......... 9/10
__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc --- FAILED

failures:

failures:
    __smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc

test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 1779 filtered out; finished in 0.15s


thread '__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc' (24858) panicked at src/sortBy_test.rs:452:986:
unknown is not iterable
stack backtrace:
   0: __rustc::rust_begin_unwind
             at /rustc/e408947bfd200af42db322daf0fadfe7e26d3bd1/library/std/src/panicking.rs:689:5
   1: core::panicking::panic_fmt
             at /rustc/e408947bfd200af42db322daf0fadfe7e26d3bd1/library/core/src/panicking.rs:80:14
   2: remeda_smelt_probe::__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc
             at ./src/sortBy_test.rs:452:986
   3: remeda_smelt_probe::__smelt_module_sortBy_test::test_data_last_sort_objects_correctly_by_weight_asc_then_color_desc::{{closure}}
             at ./src/sortBy_test.rs:418:88
   4: core::ops::function::FnOnce::call_once
             at /rustc/e408947bfd200af42db322daf0fadfe7e26d3bd1/library/core/src/ops/function.rs:250:5
   5: core::ops::function::FnOnce::call_once
             at /rustc/e408947bfd200af42db322daf0fadfe7e26d3bd1/library/core/src/ops/function.rs:250:5
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `constant_test`: `failed` - `test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s`

```text

running 7 tests
.. 2/7
__smelt_module_constant_test::test_returns_identity_doesn_t_clone --- FAILED
....
failures:

failures:
    __smelt_module_constant_test::test_returns_identity_doesn_t_clone

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
- `mapWithFeedback_test`: `failed` - `test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s`

```text

running 7 tests
...... 6/7
__smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator --- FAILED

failures:

failures:
    __smelt_module_mapWithFeedback_test::test_data_first_should_use_the_same_accumulator_on_every_iteration_if_it_s_mutable_therefore_returning_an_array_containing_array_length_references_to_the_accumulator

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 1782 filtered out; finished in 0.00s

Error: Custom { kind: Other, error: "expect(...).toStrictEqual(...) failed" }
error: test failed, to rerun pass `--bin remeda_smelt_probe`
```
