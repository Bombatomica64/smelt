# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `2`
- Full suite executed: `true`

## Focused Runs

- `__smelt_module_isEmptyish_test`: `failed` - `no test-result line`

```text
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
  75 |     _smelt_tmp_13 = SmeltRecord::from([("array".to_owned(), SmeltUnknown::Array(_smelt_tmp_1.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("bigint".to_owned(), SmeltUnknown::Number(1.0 as f64)), ("boolean".to_owned(), SmeltUnknown::Bool(false)), ("date".to_owned(), match _smelt_tmp_3.clone().clone() { SmeltUnknown::Object(value) if value.contains_key("__smelt_date") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([("__smelt_date".to_owned(), SmeltUnknown::Number(value))]))), value => value }), ("error".to_owned(), SmeltUnknown::String("asd".to_owned())), ("function".to_owned(), SmeltUnknown::Function({ let smelt_callback = _smelt_tmp_4.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((smelt_callback)())) })), ("instance".to_owned(), { let smelt_object_value = _smelt_tmp_5.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = ::std::collections::HashMap::new(); smelt_object_entries.insert("foo".to_owned(), SmeltUnknown::String(smelt_object_value.foo)); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }), ("map".to_owned(), { let smelt_record = (_smelt_tmp_6.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("null".to_owned(), SmeltUnknown::Null), ("number".to_owned(), SmeltUnknown::Number(5.0 as f64)), ("object".to_owned(), { let smelt_record = (_smelt_tmp_7.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("promise".to_owned(), SmeltUnknown::Null), ("regex".to_owned(), (_smelt_tmp_9.clone()).clone().into_smelt_unknown()), ("set".to_owned(), { let mut values = _smelt_tmp_10.clone().clone().into_iter().map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }), ("string".to_owned(), SmeltUnknown::String("text".to_owned())), ("symbol".to_owned(), SmeltUnknown::Symbol("Symbol(symbol)@802".to_owned().to_owned())), ("tuple".to_owned(), SmeltUnknown::Array(vec![SmeltUnknown::Number(_smelt_tmp_11.clone().0.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().1.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().2.clone() as f64)].into())), ("typedArray".to_owned(), SmeltUnknown::Array(_smelt_tmp_12.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("undefined".to_owned(), SmeltUnknown::Null)]);
     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            +++++++

error[E0308]: mismatched types
    --> src/main.rs:1866:29
     |
1866 |         SmeltUnknown::Array(values)
     |         ------------------- ^^^^^^ expected `SmeltArray`, found `Vec<SmeltUnknown>`
     |         |
     |         arguments to this enum variant are incorrect
     |
     = note: expected struct `SmeltArray`
                found struct `Vec<SmeltUnknown>`
note: tuple variant defined here
    --> src/main.rs:1402:5
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
1866 |         SmeltUnknown::Array(values.into())
     |                                   +++++++

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 60 previous errors
```

## Regression Guards

- `__smelt_module_isEmpty_test`: `failed` - `no test-result line`

```text
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
  75 |     _smelt_tmp_13 = SmeltRecord::from([("array".to_owned(), SmeltUnknown::Array(_smelt_tmp_1.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("bigint".to_owned(), SmeltUnknown::Number(1.0 as f64)), ("boolean".to_owned(), SmeltUnknown::Bool(false)), ("date".to_owned(), match _smelt_tmp_3.clone().clone() { SmeltUnknown::Object(value) if value.contains_key("__smelt_date") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([("__smelt_date".to_owned(), SmeltUnknown::Number(value))]))), value => value }), ("error".to_owned(), SmeltUnknown::String("asd".to_owned())), ("function".to_owned(), SmeltUnknown::Function({ let smelt_callback = _smelt_tmp_4.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((smelt_callback)())) })), ("instance".to_owned(), { let smelt_object_value = _smelt_tmp_5.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = ::std::collections::HashMap::new(); smelt_object_entries.insert("foo".to_owned(), SmeltUnknown::String(smelt_object_value.foo)); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }), ("map".to_owned(), { let smelt_record = (_smelt_tmp_6.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("null".to_owned(), SmeltUnknown::Null), ("number".to_owned(), SmeltUnknown::Number(5.0 as f64)), ("object".to_owned(), { let smelt_record = (_smelt_tmp_7.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("promise".to_owned(), SmeltUnknown::Null), ("regex".to_owned(), (_smelt_tmp_9.clone()).clone().into_smelt_unknown()), ("set".to_owned(), { let mut values = _smelt_tmp_10.clone().clone().into_iter().map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }), ("string".to_owned(), SmeltUnknown::String("text".to_owned())), ("symbol".to_owned(), SmeltUnknown::Symbol("Symbol(symbol)@802".to_owned().to_owned())), ("tuple".to_owned(), SmeltUnknown::Array(vec![SmeltUnknown::Number(_smelt_tmp_11.clone().0.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().1.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().2.clone() as f64)].into())), ("typedArray".to_owned(), SmeltUnknown::Array(_smelt_tmp_12.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("undefined".to_owned(), SmeltUnknown::Null)]);
     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            +++++++

error[E0308]: mismatched types
    --> src/main.rs:1866:29
     |
1866 |         SmeltUnknown::Array(values)
     |         ------------------- ^^^^^^ expected `SmeltArray`, found `Vec<SmeltUnknown>`
     |         |
     |         arguments to this enum variant are incorrect
     |
     = note: expected struct `SmeltArray`
                found struct `Vec<SmeltUnknown>`
note: tuple variant defined here
    --> src/main.rs:1402:5
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
1866 |         SmeltUnknown::Array(values.into())
     |                                   +++++++

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 60 previous errors
```
- `__smelt_module_hasProp_test`: `failed` - `no test-result line`

```text
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
  75 |     _smelt_tmp_13 = SmeltRecord::from([("array".to_owned(), SmeltUnknown::Array(_smelt_tmp_1.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("bigint".to_owned(), SmeltUnknown::Number(1.0 as f64)), ("boolean".to_owned(), SmeltUnknown::Bool(false)), ("date".to_owned(), match _smelt_tmp_3.clone().clone() { SmeltUnknown::Object(value) if value.contains_key("__smelt_date") => SmeltUnknown::Object(value), SmeltUnknown::Number(value) => SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([("__smelt_date".to_owned(), SmeltUnknown::Number(value))]))), value => value }), ("error".to_owned(), SmeltUnknown::String("asd".to_owned())), ("function".to_owned(), SmeltUnknown::Function({ let smelt_callback = _smelt_tmp_4.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((smelt_callback)())) })), ("instance".to_owned(), { let smelt_object_value = _smelt_tmp_5.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = ::std::collections::HashMap::new(); smelt_object_entries.insert("foo".to_owned(), SmeltUnknown::String(smelt_object_value.foo)); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }), ("map".to_owned(), { let smelt_record = (_smelt_tmp_6.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("null".to_owned(), SmeltUnknown::Null), ("number".to_owned(), SmeltUnknown::Number(5.0 as f64)), ("object".to_owned(), { let smelt_record = (_smelt_tmp_7.clone()).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::String(value))).collect())) }), ("promise".to_owned(), SmeltUnknown::Null), ("regex".to_owned(), (_smelt_tmp_9.clone()).clone().into_smelt_unknown()), ("set".to_owned(), { let mut values = _smelt_tmp_10.clone().clone().into_iter().map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) }), ("string".to_owned(), SmeltUnknown::String("text".to_owned())), ("symbol".to_owned(), SmeltUnknown::Symbol("Symbol(symbol)@802".to_owned().to_owned())), ("tuple".to_owned(), SmeltUnknown::Array(vec![SmeltUnknown::Number(_smelt_tmp_11.clone().0.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().1.clone() as f64), SmeltUnknown::Number(_smelt_tmp_11.clone().2.clone() as f64)].into())), ("typedArray".to_owned(), SmeltUnknown::Array(_smelt_tmp_12.clone().into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect())), ("undefined".to_owned(), SmeltUnknown::Null)]);
     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            +++++++

error[E0308]: mismatched types
    --> src/main.rs:1866:29
     |
1866 |         SmeltUnknown::Array(values)
     |         ------------------- ^^^^^^ expected `SmeltArray`, found `Vec<SmeltUnknown>`
     |         |
     |         arguments to this enum variant are incorrect
     |
     = note: expected struct `SmeltArray`
                found struct `Vec<SmeltUnknown>`
note: tuple variant defined here
    --> src/main.rs:1402:5
     |
1402 |     Array(SmeltArray),
     |     ^^^^^
help: call `Into::into` on this expression to convert `Vec<SmeltUnknown>` into `SmeltArray`
     |
1866 |         SmeltUnknown::Array(values.into())
     |                                   +++++++

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 60 previous errors
```

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/remeda-isemptyish-after-set-coercion-2026-06-05.md`
- Resolved tests: `176`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `failed`
- Errors: `1`
- Warnings: `271`

## Summary By Code

1. **warning** `unused_mut` - 133 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **error** `E0308` - 1 diagnostic
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 133 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
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
4. **warning** `unused_parens` - 23 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:116`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:98`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:97`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:79`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
7. **warning** `unused_parens` - 2 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
8. **error** `E0308` - 1 occurrence
   - Message: mismatched types
   - Examples:
     - `src/main.rs:1866`
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
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:37`
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
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe") due to 1 previous error; 271 warnings emitted
```
