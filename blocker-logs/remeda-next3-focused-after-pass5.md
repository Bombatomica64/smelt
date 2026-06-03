# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `3`
- Guard runs: `2`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_addProp_test`: `failed` - `no test-result line`

```text
error[E0308]: mismatched types
    --> src/doNothing_test.rs:4307:138
     |
4307 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4307 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4308 ~     Ok(()) } });
     |

error[E0308]: mismatched types
    --> src/doNothing_test.rs:4352:138
     |
4352 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4352 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4353 ~     Ok(()) } });
     |

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 123 previous errors
```
- `__smelt_module_binarySearchCutoffIndex_test`: `failed` - `no test-result line`

```text
error[E0308]: mismatched types
    --> src/doNothing_test.rs:4307:138
     |
4307 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4307 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4308 ~     Ok(()) } });
     |

error[E0308]: mismatched types
    --> src/doNothing_test.rs:4352:138
     |
4352 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4352 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4353 ~     Ok(()) } });
     |

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 123 previous errors
```
- `__smelt_module_ceil_test`: `failed` - `no test-result line`

```text
error[E0308]: mismatched types
    --> src/doNothing_test.rs:4307:138
     |
4307 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4307 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4308 ~     Ok(()) } });
     |

error[E0308]: mismatched types
    --> src/doNothing_test.rs:4352:138
     |
4352 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4352 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4353 ~     Ok(()) } });
     |

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 123 previous errors
```

## Regression Guards

- `__smelt_module_entries_test`: `failed` - `no test-result line`

```text
error[E0308]: mismatched types
    --> src/doNothing_test.rs:4307:138
     |
4307 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4307 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4308 ~     Ok(()) } });
     |

error[E0308]: mismatched types
    --> src/doNothing_test.rs:4352:138
     |
4352 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4352 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4353 ~     Ok(()) } });
     |

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 123 previous errors
```
- `__smelt_module_invert_test`: `failed` - `no test-result line`

```text
error[E0308]: mismatched types
    --> src/doNothing_test.rs:4307:138
     |
4307 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4307 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4308 ~     Ok(()) } });
     |

error[E0308]: mismatched types
    --> src/doNothing_test.rs:4352:138
     |
4352 | ...ve || { (_smelt_adapted_callback)(); () } });
     |                                         ^^ expected `Result<(), Box<dyn Error>>`, found `()`
     |
     = note:   expected enum `Result<(), Box<dyn StdError>>`
             found unit type `()`
help: try adding an expression at the end of the block
     |
4352 ~     _smelt_tmp_20 = ::std::rc::Rc::new({ let mut _smelt_adapted_callback = _smelt_tmp_19.clone(); move || { (_smelt_adapted_callback)(); ();
4353 ~     Ok(()) } });
     |

For more information about this error, try `rustc --explain E0308`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 123 previous errors
```

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `240`

## Summary By Code

1. **warning** `unused_parens` - 106 diagnostics
2. **warning** `unused_mut` - 96 diagnostics
3. **warning** `unused_unsafe` - 23 diagnostics
4. **warning** `unused_assignments` - 14 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 96 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:105`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_unsafe` - 23 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
     - `src/debounce.rs:266`
4. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/find.rs:23`
     - `src/findIndex.rs:18`
5. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/allPass_test.rs:86`
     - `src/allPass_test.rs:87`
     - `src/anyPass_test.rs:86`
     - `src/anyPass_test.rs:87`
     - `src/purryOrderRules.rs:148`
6. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `call` is never read
   - Examples:
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:111`
     - `src/funnel_lodash_debounce_with_cached_value_test.rs:96`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:92`
     - `src/funnel_lodash_throttle_with_cached_value_test.rs:77`
7. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:109`
     - `src/funnel_lodash_debounce_test.rs:96`
     - `src/funnel_lodash_throttle_test.rs:90`
     - `src/funnel_lodash_throttle_test.rs:77`
8. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:47`
     - `src/toCamelCase.rs:79`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
10. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `batch_funnel` is never read
   - Examples:
     - `src/funnel_reference_batch_test.rs:29`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:670`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
18. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:20`

## Cargo Stderr

```text
Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.85s
```
