# es-toolkit codegen misc error classes

Fixing three semantically-distinct generated-Rust compile-error classes in the
es-toolkit whole-crate transpile (`<es-toolkit>/dist-smelt`). Diagnosed with
`smelt rust-diagnostics`. The sibling agent owns the large `E0308` class, which
was not touched here.

## Totals

- Whole-crate errors: **548 → 524** (−24).
- `E0308` stable at 396 (394 at the original baseline; +2 is newly-revealed
  code unblocked by the `E0004` fix, not a regression introduced here — it is
  the sibling's lane).

## Fixed (per class, root cause)

### E0004 — non-exhaustive patterns: `16 → 0`

Message: `non-exhaustive patterns: Some(SmeltUnknown::Undefined) not covered`
(e.g. `ary.rs:15`, `cloneWith.rs`, `curry.rs`, `defaults.rs`, ...).

Root cause: `optional_truthy_text` in `emitter/types.rs` builds the JS-truthiness
`match` for an `Option<SmeltUnknown>` value. Its falsy arm was
`Some(SmeltUnknown::Null) => false`, omitting the dedicated `Undefined` variant,
so the `match` was non-exhaustive. The non-optional coercion path
(`extract_value_text`, `Some(Type::Bool)`) already grouped `Null | Undefined`;
the optional path had drifted.

Fix (one site): falsy arm is now
`Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false`.
Regression test: `optional_unknown_truthiness_covers_undefined`.

### E0596 — cannot borrow behind `&` reference: `12 → 6`

Message: `cannot borrow *mapper as mutable, as it is behind a & reference`
(`unionBy_1.rs`, `xorBy_1.rs`, `xorWith_1.rs`, `isSubsetWith.rs`).

Root cause: `function_shape_adapter_text` in `emitter/core.rs` builds a wrapper
closure when a borrowed callback parameter is forwarded to a helper expecting a
different callback arity. When the source is a function parameter it reborrowed
it as `&mut *mapper`. Callback parameters are always bound as immutable
`&dyn Fn` (see `param_type_text`; `parameter_needs_mutable_reference` never
applies to function types), so a `&mut` reborrow through that shared reference
cannot compile. The fresh `move` wrapper closure supplies whatever `FnMut` shape
the callee needs on its own.

Fix (one site): reborrow immutably with `&*mapper`.
Regression test: `callback_parameter_adapter_reborrows_immutably`.

### E0599 — method not found (tuple `.length`): `20 → 16` (4 fixed)

Message: `no method named len found for tuple (SmeltUnknown, SmeltUnknown)` /
`(String, SmeltUnknown)` (`dropWhile.rs`, `findIndex.rs`, `dropRightWhile.rs`,
`findLastIndex.rs`).

Root cause: `len_text` in `emitter/numeric.rs` lowers a `.length` read
(`Rvalue::Len`). When TypeScript narrows a value to a fixed tuple (e.g. the
`[key, value]` matches-property shorthand) and reads `.length`, the fallback arm
emitted `{receiver}.len()`, but Rust tuples have no `.len()` method.

Fix (one arm): a `Type::Tuple(items)` receiver now emits the compile-time arity
constant (`items.len()`), matching JavaScript's fixed tuple `.length`.
Regression test: `tuple_length_emits_constant_arity`.

## Deferred (with reason)

- **E0599 remaining (16):**
  - `SmeltUnknown is not an iterator` (6, `isEqualWith.rs`): entangled with the
    known `isEqualWith` inlining blocker (see `Smelt.toml`); single complex file,
    also carries the `E0282` cluster.
  - `SmeltJsMap<T, String>` method/trait-bound failures (9, `main.rs`): the map
    key type parameter is unbounded; needs `Hash`/`Eq` bound propagation on the
    generated map, adjacent to the trait-bound work below.
  - `into_smelt_unknown` on `Rc<dyn Fn>` (2), `unwrap_or_else` on `()` (1):
    isolated shape issues.
- **E0277 (19):** heterogeneous.
  - `T/f64/SmeltUnion359: Hash + Eq` (uniq, pullAt): JS `Set`/dedup over
    arbitrary values (incl. `f64`, unbounded generics, unions) cannot key a Rust
    `HashSet`/`HashMap`. This is a runtime-container redesign (SameValueZero
    semantics), not a localized emitter fix.
  - `can't compare Option<SmeltUnknown>/f64` (repeat, range, rangeRight):
    comparison-operator numeric coercion — deliberately left alone to avoid
    colliding with the sibling's `E0308` coercion paths.
  - `SmeltUnknown: AsRef<str>` (escape/unescape): `String.replace` with a
    function replacer must coerce the closure's `SmeltUnknown` return to a
    string; localized but non-trivial.
  - Misc `Debug`/`Deserialize`/`Default`/`Display` derive gaps: one-offs.
- **E0596 remaining (6):** class-field mutation through `&self`
  (`self.__data__`, `self.semaphore`) and `Rc`-interior mutation
  (`after.rs`, `before.rs`) — distinct class/`Rc` root causes.
- **E0609 (10):** `no field apply on DebouncedFunction<...>` (debounce/throttle)
  and similar — entangled with returned-object-literal struct modeling
  (adjacent to `E0308` struct-shape work).
- **E0425 (7):** closure-capture name bugs — `smelt_capture_self` in async class
  methods (4), a double `smelt_capture_` prefix in nested-closure assignment
  targets (2, `debounce_1.rs`), and one `smelt_callback` scope issue. Entangled
  with async-method self-capture and nested-closure capture aliasing.

## Validation

- `cargo check --workspace`: clean.
- `cargo clippy -p smelt-codegen-rust --lib -W clippy::pedantic`: no new
  warnings in touched files (the 2 remaining pedantic warnings are pre-existing
  in `list_mutation.rs`).
- `cargo test --workspace --exclude smelt-gui`: all pass, incl. 3 new tests.
- es-toolkit `dist-smelt` regenerated with the release binary after each change.

## SmeltUnknown

No new `SmeltUnknown` conversions introduced; all three fixes narrow or correct
existing generated forms.
