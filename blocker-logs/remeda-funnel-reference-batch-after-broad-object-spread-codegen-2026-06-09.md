# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_funnel_reference_batch_test`: `failed` - `no test-result line`

```text
help: the following other types implement trait `Default`
    --> /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/rc.rs:2562:1
     |
2562 | / impl<T> Default for Pin<Rc<T>>
2563 | | where
2564 | |     T: ?Sized,
2565 | |     Rc<T>: Default,
     | |___________________^ `Pin<Rc<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/boxed.rs:1943:1
     |
1943 | / impl<T> Default for Pin<Box<T>>
1944 | | where
1945 | |     T: ?Sized,
1946 | |     Box<T>: Default,
     | |____________________^ `Pin<Box<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/sync.rs:3828:1
     |
3828 | / impl<T> Default for Pin<Arc<T>>
3829 | | where
3830 | |     T: ?Sized,
3831 | |     Arc<T>: Default,
     | |____________________^ `Pin<Arc<T>>`
     = note: required for `Box<dyn Future<Output = SmeltUnknown>>` to implement `Default`
     = note: 1 redundant requirement hidden
     = note: required for `Pin<Box<dyn Future<Output = SmeltUnknown>>>` to implement `Default`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```

## Regression Guards

- `__smelt_module_funnel_lodash_throttle_with_cached_value_test`: `failed` - `no test-result line`

```text
help: the following other types implement trait `Default`
    --> /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/rc.rs:2562:1
     |
2562 | / impl<T> Default for Pin<Rc<T>>
2563 | | where
2564 | |     T: ?Sized,
2565 | |     Rc<T>: Default,
     | |___________________^ `Pin<Rc<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/boxed.rs:1943:1
     |
1943 | / impl<T> Default for Pin<Box<T>>
1944 | | where
1945 | |     T: ?Sized,
1946 | |     Box<T>: Default,
     | |____________________^ `Pin<Box<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/sync.rs:3828:1
     |
3828 | / impl<T> Default for Pin<Arc<T>>
3829 | | where
3830 | |     T: ?Sized,
3831 | |     Arc<T>: Default,
     | |____________________^ `Pin<Arc<T>>`
     = note: required for `Box<dyn Future<Output = SmeltUnknown>>` to implement `Default`
     = note: 1 redundant requirement hidden
     = note: required for `Pin<Box<dyn Future<Output = SmeltUnknown>>>` to implement `Default`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test`: `failed` - `no test-result line`

```text
help: the following other types implement trait `Default`
    --> /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/rc.rs:2562:1
     |
2562 | / impl<T> Default for Pin<Rc<T>>
2563 | | where
2564 | |     T: ?Sized,
2565 | |     Rc<T>: Default,
     | |___________________^ `Pin<Rc<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/boxed.rs:1943:1
     |
1943 | / impl<T> Default for Pin<Box<T>>
1944 | | where
1945 | |     T: ?Sized,
1946 | |     Box<T>: Default,
     | |____________________^ `Pin<Box<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/sync.rs:3828:1
     |
3828 | / impl<T> Default for Pin<Arc<T>>
3829 | | where
3830 | |     T: ?Sized,
3831 | |     Arc<T>: Default,
     | |____________________^ `Pin<Arc<T>>`
     = note: required for `Box<dyn Future<Output = SmeltUnknown>>` to implement `Default`
     = note: 1 redundant requirement hidden
     = note: required for `Pin<Box<dyn Future<Output = SmeltUnknown>>>` to implement `Default`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```
- `__smelt_module_randomBigInt_test`: `failed` - `no test-result line`

```text
help: the following other types implement trait `Default`
    --> /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/rc.rs:2562:1
     |
2562 | / impl<T> Default for Pin<Rc<T>>
2563 | | where
2564 | |     T: ?Sized,
2565 | |     Rc<T>: Default,
     | |___________________^ `Pin<Rc<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/boxed.rs:1943:1
     |
1943 | / impl<T> Default for Pin<Box<T>>
1944 | | where
1945 | |     T: ?Sized,
1946 | |     Box<T>: Default,
     | |____________________^ `Pin<Box<T>>`
     |
    ::: /home/lollo/.rustup/toolchains/1.94.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/alloc/src/sync.rs:3828:1
     |
3828 | / impl<T> Default for Pin<Arc<T>>
3829 | | where
3830 | |     T: ?Sized,
3831 | |     Arc<T>: Default,
     | |____________________^ `Pin<Arc<T>>`
     = note: required for `Box<dyn Future<Output = SmeltUnknown>>` to implement `Default`
     = note: 1 redundant requirement hidden
     = note: required for `Pin<Box<dyn Future<Output = SmeltUnknown>>>` to implement `Default`

For more information about this error, try `rustc --explain E0277`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 1 previous error
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `1`
- Warnings: `259`

## Summary By Code

1. **warning** `unused_mut` - 125 diagnostics
2. **warning** `unused_parens` - 93 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **error** `E0277` - 1 diagnostic
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 125 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
     - `src/clamp.rs:8`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_unsafe` - 31 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
4. **warning** `unused_parens` - 24 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
6. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
7. **error** `E0277` - 1 occurrence
   - Message: the trait bound `dyn Future<Output = SmeltUnknown>: Default` is not satisfied
   - Examples:
     - `src/funnel_reference_batch_test.rs:116`
8. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 1 previous error; 259 warnings emitted
```
