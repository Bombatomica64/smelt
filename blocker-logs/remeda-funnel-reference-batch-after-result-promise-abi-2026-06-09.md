# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_funnel_reference_batch_test`: `failed` - `no test-result line`

```text
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isString_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:30:132: 30:142}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:30:123
   |
30 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:30:132: 30:142}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:74:133: 74:143}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:74:124
   |
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

Some errors have detailed explanations: E0271, E0277, E0599.
For more information about an error, try `rustc --explain E0271`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 277 previous errors
```

## Regression Guards

- `__smelt_module_funnel_lodash_throttle_with_cached_value_test`: `failed` - `no test-result line`

```text
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isString_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:30:132: 30:142}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:30:123
   |
30 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:30:132: 30:142}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:74:133: 74:143}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:74:124
   |
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

Some errors have detailed explanations: E0271, E0277, E0599.
For more information about an error, try `rustc --explain E0271`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 277 previous errors
```
- `__smelt_module_funnel_lodash_debounce_with_cached_value_test`: `failed` - `no test-result line`

```text
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isString_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:30:132: 30:142}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:30:123
   |
30 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:30:132: 30:142}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:74:133: 74:143}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:74:124
   |
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

Some errors have detailed explanations: E0271, E0277, E0599.
For more information about an error, try `rustc --explain E0271`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 277 previous errors
```
- `__smelt_module_randomBigInt_test`: `failed` - `no test-result line`

```text
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isString_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:30:132: 30:142}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:30:123
   |
30 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:30:132: 30:142}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

error[E0271]: expected `{async block@src/isSymbol_test.rs:74:133: 74:143}` to be a future that resolves to `Result<f64, Box<dyn Error>>`, but it resolves to `f64`
  --> src/isSymbol_test.rs:74:124
   |
74 | ... = Box::pin(async move { Box::pin(async move { smelt_sleep_ms(0.0 as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }).await; 0.0 }) as...
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
   |
   = note: expected enum `Result<f64, Box<dyn StdError>>`
              found type `f64`
   = note: required for the cast from `Pin<Box<{async block@src/isSymbol_test.rs:74:133: 74:143}>>` to `Pin<Box<dyn Future<Output = Result<f64, Box<dyn StdError>>>>>`

Some errors have detailed explanations: E0271, E0277, E0599.
For more information about an error, try `rustc --explain E0271`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 277 previous errors
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `5`
- Warnings: `259`

## Summary By Code

1. **warning** `unused_mut` - 125 diagnostics
2. **warning** `unused_parens` - 93 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **error** `E0277` - 4 diagnostics
6. **error** `E0599` - 1 diagnostic
7. **warning** `unreachable_code` - 1 diagnostic

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
6. **error** `E0277` - 3 occurrences
   - Message: the `?` operator can only be used in an async function that returns `Result` or `Option` (or another type that implements `FromResidual`)
   - Examples:
     - `src/debounce_test.rs:9`
     - `src/funnel_lodash_debounce_test.rs:120`
     - `src/sleep.rs:9`
7. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
8. **error** `E0277` - 1 occurrence
   - Message: the `?` operator can only be applied to values that implement `Try`
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:120`
9. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for enum `Result<T, E>` in the current scope
   - Examples:
     - `src/funnel_reference_batch_test.rs:117`
10. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:104`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:63`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:62`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:861`
16. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:24`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 5 previous errors; 259 warnings emitted
```
