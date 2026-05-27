# Generated Rust Test Report

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Focused runs: `1`
- Guard runs: `3`
- Full suite executed: `false`

## Focused Runs

- `__smelt_module_funnel_lodash_throttle_test`: `failed` - `no test-result line`

```text
    |

error[E0599]: no method named `borrow_mut` found for reference `&{closure@src/zipWith.rs:19:200: 19:287}` in the current scope
   --> src/zipWith.rs:19:308
    |
 19 | ... f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
    |                                                                  ^^^^^^^^^^
    |
   ::: /home/lollo/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/borrow.rs:207:8
    |
207 |     fn borrow_mut(&mut self) -> &mut Borrowed;
    |        ---------- the method is available for `&{closure@src/zipWith.rs:19:200: 19:287}` here
    |
    = help: items from traits can only be used if the trait is in scope
help: use parentheses to call this closure
    |
 19 |     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *((&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null))(/* SmeltUnknown */, /* SmeltUnknown */, /* f64 */, /* (SmeltUnknown, SmeltUnknown) */).borrow_mut()) }
    |                                                                                                                                                                                                      +                                                                                                             ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
help: trait `BorrowMut` which provides `borrow_mut` is implemented but not in scope; perhaps you want to import it
    |
  5 + use std::borrow::BorrowMut;
    |
help: there is a method `borrow` with a similar name
    |
 19 -     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
 19 +     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow()) }
    |

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```

## Regression Guards

- `__smelt_module_truncate_test`: `failed` - `no test-result line`

```text
    |

error[E0599]: no method named `borrow_mut` found for reference `&{closure@src/zipWith.rs:19:200: 19:287}` in the current scope
   --> src/zipWith.rs:19:308
    |
 19 | ... f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
    |                                                                  ^^^^^^^^^^
    |
   ::: /home/lollo/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/borrow.rs:207:8
    |
207 |     fn borrow_mut(&mut self) -> &mut Borrowed;
    |        ---------- the method is available for `&{closure@src/zipWith.rs:19:200: 19:287}` here
    |
    = help: items from traits can only be used if the trait is in scope
help: use parentheses to call this closure
    |
 19 |     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *((&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null))(/* SmeltUnknown */, /* SmeltUnknown */, /* f64 */, /* (SmeltUnknown, SmeltUnknown) */).borrow_mut()) }
    |                                                                                                                                                                                                      +                                                                                                             ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
help: trait `BorrowMut` which provides `borrow_mut` is implemented but not in scope; perhaps you want to import it
    |
  5 + use std::borrow::BorrowMut;
    |
help: there is a method `borrow` with a similar name
    |
 19 -     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
 19 +     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow()) }
    |

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```
- `__smelt_module_clone_test::test_objects_clones_objects_with_circular_references`: `failed` - `no test-result line`

```text
    |

error[E0599]: no method named `borrow_mut` found for reference `&{closure@src/zipWith.rs:19:200: 19:287}` in the current scope
   --> src/zipWith.rs:19:308
    |
 19 | ... f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
    |                                                                  ^^^^^^^^^^
    |
   ::: /home/lollo/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/borrow.rs:207:8
    |
207 |     fn borrow_mut(&mut self) -> &mut Borrowed;
    |        ---------- the method is available for `&{closure@src/zipWith.rs:19:200: 19:287}` here
    |
    = help: items from traits can only be used if the trait is in scope
help: use parentheses to call this closure
    |
 19 |     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *((&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null))(/* SmeltUnknown */, /* SmeltUnknown */, /* f64 */, /* (SmeltUnknown, SmeltUnknown) */).borrow_mut()) }
    |                                                                                                                                                                                                      +                                                                                                             ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
help: trait `BorrowMut` which provides `borrow_mut` is implemented but not in scope; perhaps you want to import it
    |
  5 + use std::borrow::BorrowMut;
    |
help: there is a method `borrow` with a similar name
    |
 19 -     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
 19 +     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow()) }
    |

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```
- `__smelt_module_difference_test`: `failed` - `no test-result line`

```text
    |

error[E0599]: no method named `borrow_mut` found for reference `&{closure@src/zipWith.rs:19:200: 19:287}` in the current scope
   --> src/zipWith.rs:19:308
    |
 19 | ... f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
    |                                                                  ^^^^^^^^^^
    |
   ::: /home/lollo/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/borrow.rs:207:8
    |
207 |     fn borrow_mut(&mut self) -> &mut Borrowed;
    |        ---------- the method is available for `&{closure@src/zipWith.rs:19:200: 19:287}` here
    |
    = help: items from traits can only be used if the trait is in scope
help: use parentheses to call this closure
    |
 19 |     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *((&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null))(/* SmeltUnknown */, /* SmeltUnknown */, /* f64 */, /* (SmeltUnknown, SmeltUnknown) */).borrow_mut()) }
    |                                                                                                                                                                                                      +                                                                                                             ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
help: trait `BorrowMut` which provides `borrow_mut` is implemented but not in scope; perhaps you want to import it
    |
  5 + use std::borrow::BorrowMut;
    |
help: there is a method `borrow` with a similar name
    |
 19 -     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow_mut()) }
 19 +     move |arg0: Vec<SmeltUnknown>, arg1: Vec<SmeltUnknown>| -> Vec<SmeltUnknown> { zip_with_implementation(SmeltUnknown::Array(arg0.clone().into()), SmeltUnknown::Array(arg1.clone().into()), &mut *(&|arg0: SmeltUnknown, arg1: SmeltUnknown, arg2: f64, arg3: (SmeltUnknown, SmeltUnknown)| SmeltUnknown::Null).borrow()) }
    |

For more information about this error, try `rustc --explain E0599`.
error: could not compile `remeda_smelt_probe` (bin "remeda_smelt_probe" test) due to 2 previous errors
```
