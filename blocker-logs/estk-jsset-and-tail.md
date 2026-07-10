# es-toolkit: SmeltJsSet container + tail E0277/E0631 fixes

Whole-crate es-toolkit probe (pinned `e008a2818cd8`), diagnostics via
`smelt rust-diagnostics` with the release binary built from this branch.

## Headline totals (es-toolkit whole-crate)

| metric | before | after | delta |
| --- | --- | --- | --- |
| total errors | 296 | 256 | **-40** |

Per-class movement (this lane and its cascades):

| code | before | after | delta | owner/notes |
| --- | --- | --- | --- | --- |
| E0277 | 19 | 9 | **-10** | my lane |
| E0631 | 5 | 3 | -2 | my lane |
| E0308 | 187 | 168 | -19 | sibling's class; reduced as a cascade of the Set representation fix |
| E0425 | 7 | 1 | -6 | cascade of the Set fix |
| E0596 | 6 | 4 | -2 | cascade |
| E0599 | 16 | 16 | 0 | deferred (see below) |
| E0609 | 10 | 10 | 0 | deferred |
| E0282 | 7 | 7 | 0 | untouched |
| E0382 | 4 | 4 | 0 | untouched |

Direct in-lane elimination (E0277 + E0631) is **-12**; the Set representation
fix additionally drove -19 E0308 / -6 E0425 / -2 E0596 cascades, for **-40
combined**. The remaining in-lane clusters (E0599 generic `SmeltJsMap`,
E0609 struct-field gaps) are entangled with other subsystems and are deferred
with reasons below rather than converted into sibling-lane errors.

## SmeltJsSet design

`Type::Set` previously emitted `HashSet<T>` when the element type was judged
"key-safe" and a bare `Vec<T>` otherwise. Both were wrong:

- `HashSet<T>` demands `T: Eq + Hash`, impossible for `f64`, generated unions
  (`SmeltUnion*`), and generic type parameters — the source of the headline
  E0277s in `uniq`, `pullAt`, and the `[...new Set(list)]` dedup pattern. It is
  also semantically wrong: JS `Set` uses SameValueZero (objects/functions by
  reference identity, `NaN` equal to itself), not Rust structural `Eq`.
- The `Vec<T>` fallback compiled but did not dedup and used `==` membership
  (so `NaN` was never found, `+0`/`-0` diverged, objects compared structurally).
- `list_to_set_text` ignored the split entirely and always emitted
  `collect::<HashSet<_>>()`, so `[...new Set(list)]` broke for every non-safe
  element type.

New container `SmeltJsSet<T>` (runtime prelude, in the `needs_unknown` block
next to `SmeltJsMap`): an insertion-ordered `Vec<T>` whose membership projects
each element through its `IntoSmeltUnknown` erasure and compares the resulting
runtime values with `SmeltJsKeyEq::same_js_key` — the same erased-key projection
`SmeltJsMap` uses for keys. This makes one uniform, JS-correct container work
for **every** element type that can be erased (no per-element `Eq + Hash`
bound), including generics whose only bound is `IntoSmeltUnknown`. It exposes
`new/len/is_empty/contains/insert/remove/iter/extend`, the set-algebra methods
(`union`/`intersection`/`difference`/`symmetric_difference` returning `&T`
iterators, `is_disjoint`/`is_subset`/`is_superset`), `Default`, `From<[T;N]>`,
`FromIterator`, `IntoIterator` (by value and by ref), `PartialEq`, and
`IntoSmeltUnknown` (erases to a sorted `SmeltUnknown::Array`, matching the old
`HashSet`/`Vec` erasure).

Split rule (`type_is_hash_set_key_safe`, now the single source of truth):
`HashSet` only for value-equality primitives (`bool`/`i64`/`String`, and
`Optional`/`Union` recursively of those) where `Eq + Hash` both exists and
matches SameValueZero. Everything else routes through `SmeltJsSet`.

Wiring: `type_text`, `default_value`, the `Rvalue::Set` literal, and
`list_to_set_text` all emit the chosen container; `set_add_text` dropped its
`f64` `push` special-case (both containers now dedup on `insert`); membership
(`set_contains_text`) routes uniformly through `.contains`, which is
SameValueZero on `SmeltJsSet` and value-equality on `HashSet` (this fixed the
`NaN` membership bug).

Gating: a `Set` whose element type is not a value-equality primitive is emitted
as `SmeltJsSet`, whose erased projection needs the `SmeltUnknown` carrier and
its traits. `needs_unknown_type` therefore returns true when such a set exists
(module-level `module_hash_set_key_safe` mirror of the emitter predicate), so
the carrier is always present when `SmeltJsSet` is used. Consequence: a small
crate that mixes a non-primitive `Set` with a string-keyed `Map` now backs that
`Map` with identity-preserving `SmeltRecord` (functionally a superset). Tests
`part_5_tests::emits_map_and_set_projection_methods` and the
`set_collection_emission` snapshot were updated accordingly; the snapshot input
switched to `Set<string>` (stays `HashSet`, small prelude) and dedicated
`SmeltJsSet` coverage moved to `emits_smelt_js_set_container`.

### Verification

`emits_smelt_js_set_container` (codegen unit test) asserts `SmeltJsSet<f64>`,
`SmeltJsSet::from([...])`, and `collect::<SmeltJsSet<_>>()`. End-to-end fixture
(built + run with the branch binary) asserts:

- dedup on construction (`new Set([1,1,2,3,2]).size === 3`)
- insertion-order iteration (`1,2,3`)
- `NaN` membership (`new Set([NaN]).has(NaN) === true`)
- `+0`/`-0` SameValueZero (`new Set([0]).has(-0) === true`)
- object identity membership (`has(a) === true`, `has({...a}) === false`)
- `add` dedup + `delete`

All observed outputs correct.

## Other E0277 fixes

- **escape/unescape `SmeltUnknown: AsRef<str>` (-2).** The regex
  `replace_all` `Replacer` closure must yield a `String`, but a callback typed
  to return `unknown` (here an erased `Record` lookup) yields `SmeltUnknown`.
  `regex_replace_callback_text` now coerces the callback result to `String`
  (JS ToString via erase + String-extract) when the callback's declared return
  type is not already `String`; the `String` fast path is unchanged. Regression
  test `coerces_non_string_regex_replace_callback_result`.
- **`SmeltJsMap` `Debug` (-2).** `SmeltJsMap` derived only `Clone`; struct
  fields holding a `SmeltJsMap` failed `#[derive(Debug)]`. Added `Debug` to the
  derive.

## Deferrals (with reasons)

- **E0599 generic `SmeltJsMap<T, String>` cluster (`CustomCache` in main.rs,
  ~5 + related E0277).** The class is generic over `T` but its field is
  `SmeltJsMap<T, String>` while its constructor builds
  `SmeltJsMap<SmeltUnknown, String>` and its methods take `SmeltUnknown` keys.
  The `same_js_key`/`SmeltFromUnknown` method-bound failures are a symptom of a
  frontend key-erasure bug (`T` should be erased to `SmeltUnknown` in the field
  type). Relaxing `SmeltJsMap` method bounds to the erased projection would
  make the methods resolve but expose the underlying `T`-vs-`SmeltUnknown`
  mismatch as E0308 (sibling's lane) — net-neutral and risks the just-landed
  reference-class `SmeltJsMap` usage. Left for the frontend erasure fix.
- **E0609 (10): debounce/throttle `.apply`, `unzipWith .length` on
  `SmeltList`, `truncate .result` on `SmeltMatch`, `HttpError.name`.** Each is a
  distinct struct/field-modeling or reference-class field-access gap
  (`self.name` should be `self.0.borrow().name` on a reference class;
  `.length` on a list value was not lowered to a `Len` op inside a callback;
  `.result`/`.apply` are missing fields on `SmeltMatch`/`DebouncedFunction`).
  None is a clean isolated emitter change; they touch reference-class field
  access and frontend lowering. Deferred.
- **E0282 (7) / E0382 (4) / remaining E0631 (3).** Untyped `Default::default()`,
  moved-value, and closure-arg mismatches; time-bounded, left for a follow-up.

## Validation

- `cargo test -p smelt-codegen-rust --lib`: 544 passed.
- `cargo check --workspace`: clean.
- `cargo test -p smelt-transpiler` incl. `end_to_end_examples_match_expected_outputs`:
  passing. `examples/.../27_optional_chains/expected.rs` (embeds the full runtime
  prelude as golden output) regenerated to include the `SmeltJsSet` block and the
  `SmeltJsMap` `Debug` derive; MIR and stdout unchanged.
- `cargo clippy -p smelt-codegen-rust --lib -W clippy::pedantic`: no new warnings
  from this change (the two shadow warnings I introduced were renamed away).
- Full `cargo test --workspace --exclude smelt-gui` could not complete in this
  environment due to ENOSPC on the shared disk; the codegen + transpiler suites
  above (the surfaces this change touches) all pass.

### Remeda no-regression spot-check

Remeda pinned `3c80f28bb394edbf89f1fc9978571dec8ed20edc` with
`.github/compat/remeda/Smelt.toml`: `smelt build` succeeded and
`cargo check` on the generated crate reported **0 errors** (warnings only).
`SmeltJsSet` is exercised in 12 generated files (`unique`, `uniqueBy`,
`sample`, `isDeepEqual`, `isShallowEqual`, ...), confirming the Set rewiring
compiles cleanly on remeda's Set usage. Generated `cargo test` (baseline
1789/1789) was not run — the shared disk was too low to build the full remeda
test crate; `cargo check` at 0 errors establishes no compile regression.
