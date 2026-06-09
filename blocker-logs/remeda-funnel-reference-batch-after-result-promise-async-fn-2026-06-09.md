# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_funnel_reference_batch_test`: `failed` - `no test-result line`

```text
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0507]: cannot move out of `smelt_callback`, a captured variable in an `Fn` closure
   --> src/funnel_reference_batch_test.rs:120:390
    |
120 | ...et smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin(async move { let smelt_async_output = (smelt_callback)(sm...
    |       --------------   --------------------                     ------------------------------------                                                             ^^^^^^^^^^                            ---------------- variable moved due to use in coroutine
    |       |                |                                        |                                                                                                |
    |       |                |                                        |                                                                                                `smelt_callback` is moved here
    |       |                |                                        captured by this `Fn` closure
    |       |                move occurs because `smelt_callback` has type `Rc<dyn Fn(Vec<SmeltUnknown>) -> Pin<Box<dyn Future<Output = ...>>>>`, which does not implement the `Copy` trait
    |       captured outer variable
    |
    = help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
    = note: the full name for the type has been written to '/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/target/debug/deps/remeda_smelt_probe-7c24d41c88c48d02.long-type-1624247646523003618.txt'
    = note: consider using `--verbose` to print the full type name to the console
help: consider cloning the value before moving it into the closure
    |
120 ~     _smelt_tmp_10 = SmeltRecord::from([("call".to_owned(), { let smelt_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>> = { let smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin({
121 +     let value = smelt_callback.clone();
122 ~     async move { let smelt_async_output = value(smelt_args.iter().skip(0).cloned().collect::<Vec<_>>()).await?; Ok::<_, Box<dyn std::error::Error>>(smelt_async_output) }
123 ~     }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> }) }; smelt_fn })]);
    |

Some errors have detailed explanations: E0069, E0271, E0277, E0507.
For more information about an error, try `rustc --explain E0069`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 111 previous errors
```

## Regression Guards

- `__smelt_module_funnel_lodash_throttle_with_cached_value_test`: `failed` - `no test-result line`

```text
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0507]: cannot move out of `smelt_callback`, a captured variable in an `Fn` closure
   --> src/funnel_reference_batch_test.rs:120:390
    |
120 | ...et smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin(async move { let smelt_async_output = (smelt_callback)(sm...
    |       --------------   --------------------                     ------------------------------------                                                             ^^^^^^^^^^                            ---------------- variable moved due to use in coroutine
    |       |                |                                        |                                                                                                |
    |       |                |                                        |                                                                                                `smelt_callback` is moved here
    |       |                |                                        captured by this `Fn` closure
    |       |                move occurs because `smelt_callback` has type `Rc<dyn Fn(Vec<SmeltUnknown>) -> Pin<Box<dyn Future<Output = ...>>>>`, which does not implement the `Copy` trait
    |       captured outer variable
    |
    = help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
    = note: the full name for the type has been written to '/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/target/debug/deps/remeda_smelt_probe-7c24d41c88c48d02.long-type-3156626409030596837.txt'
    = note: consider using `--verbose` to print the full type name to the console
help: consider cloning the value before moving it into the closure
    |
120 ~     _smelt_tmp_10 = SmeltRecord::from([("call".to_owned(), { let smelt_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>> = { let smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin({
121 +     let value = smelt_callback.clone();
122 ~     async move { let smelt_async_output = value(smelt_args.iter().skip(0).cloned().collect::<Vec<_>>()).await?; Ok::<_, Box<dyn std::error::Error>>(smelt_async_output) }
123 ~     }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> }) }; smelt_fn })]);
    |

Some errors have detailed explanations: E0069, E0271, E0277, E0507.
For more information about an error, try `rustc --explain E0069`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 111 previous errors
```
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test`: `failed` - `no test-result line`

```text
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0507]: cannot move out of `smelt_callback`, a captured variable in an `Fn` closure
   --> src/funnel_reference_batch_test.rs:120:390
    |
120 | ...et smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin(async move { let smelt_async_output = (smelt_callback)(sm...
    |       --------------   --------------------                     ------------------------------------                                                             ^^^^^^^^^^                            ---------------- variable moved due to use in coroutine
    |       |                |                                        |                                                                                                |
    |       |                |                                        |                                                                                                `smelt_callback` is moved here
    |       |                |                                        captured by this `Fn` closure
    |       |                move occurs because `smelt_callback` has type `Rc<dyn Fn(Vec<SmeltUnknown>) -> Pin<Box<dyn Future<Output = ...>>>>`, which does not implement the `Copy` trait
    |       captured outer variable
    |
    = help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
    = note: the full name for the type has been written to '/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/target/debug/deps/remeda_smelt_probe-7c24d41c88c48d02.long-type-18148751483214412499.txt'
    = note: consider using `--verbose` to print the full type name to the console
help: consider cloning the value before moving it into the closure
    |
120 ~     _smelt_tmp_10 = SmeltRecord::from([("call".to_owned(), { let smelt_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>> = { let smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin({
121 +     let value = smelt_callback.clone();
122 ~     async move { let smelt_async_output = value(smelt_args.iter().skip(0).cloned().collect::<Vec<_>>()).await?; Ok::<_, Box<dyn std::error::Error>>(smelt_async_output) }
123 ~     }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> }) }; smelt_fn })]);
    |

Some errors have detailed explanations: E0069, E0271, E0277, E0507.
For more information about an error, try `rustc --explain E0069`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 111 previous errors
```
- `__smelt_module_randomBigInt_test`: `failed` - `no test-result line`

```text
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0507]: cannot move out of `smelt_callback`, a captured variable in an `Fn` closure
   --> src/funnel_reference_batch_test.rs:120:390
    |
120 | ...et smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin(async move { let smelt_async_output = (smelt_callback)(sm...
    |       --------------   --------------------                     ------------------------------------                                                             ^^^^^^^^^^                            ---------------- variable moved due to use in coroutine
    |       |                |                                        |                                                                                                |
    |       |                |                                        |                                                                                                `smelt_callback` is moved here
    |       |                |                                        captured by this `Fn` closure
    |       |                move occurs because `smelt_callback` has type `Rc<dyn Fn(Vec<SmeltUnknown>) -> Pin<Box<dyn Future<Output = ...>>>>`, which does not implement the `Copy` trait
    |       captured outer variable
    |
    = help: `Fn` and `FnMut` closures require captured values to be able to be consumed multiple times, but `FnOnce` closures may consume them only once
    = note: the full name for the type has been written to '/home/lollo/Playground/smelt/third_party/remeda/dist-smelt/target/debug/deps/remeda_smelt_probe-7c24d41c88c48d02.long-type-13989712949604763480.txt'
    = note: consider using `--verbose` to print the full type name to the console
help: consider cloning the value before moving it into the closure
    |
120 ~     _smelt_tmp_10 = SmeltRecord::from([("call".to_owned(), { let smelt_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>> = { let smelt_callback = _smelt_tmp_9.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let smelt_async_callback = _smelt_tmp_9.clone(); Box::pin({
121 +     let value = smelt_callback.clone();
122 ~     async move { let smelt_async_output = value(smelt_args.iter().skip(0).cloned().collect::<Vec<_>>()).await?; Ok::<_, Box<dyn std::error::Error>>(smelt_async_output) }
123 ~     }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> }) }; smelt_fn })]);
    |

Some errors have detailed explanations: E0069, E0271, E0277, E0507.
For more information about an error, try `rustc --explain E0069`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 111 previous errors
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `4`
- Warnings: `267`

## Summary By Code

1. **warning** `unused_mut` - 133 diagnostics
2. **warning** `unused_parens` - 93 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **error** `E0069` - 3 diagnostics
6. **error** `E0507` - 1 diagnostic
7. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 133 occurrences
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
6. **error** `E0069` - 3 occurrences
   - Message: `return;` in a function whose return type is not `()`
   - Examples:
     - `src/debounce_test.rs:10`
     - `src/funnel_lodash_debounce_test.rs:121`
     - `src/sleep.rs:10`
7. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
8. **error** `E0507` - 1 occurrence
   - Message: cannot move out of `smelt_callback`, a captured variable in an `Fn` closure
   - Examples:
     - `src/funnel_reference_batch_test.rs:120`
9. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
16. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 4 previous errors; 267 warnings emitted
```
